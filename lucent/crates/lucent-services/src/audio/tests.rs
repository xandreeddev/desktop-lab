use super::*;
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
};

#[derive(Default)]
struct Stop(AtomicBool);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn wait(&self, duration: Duration) {
        let until = Instant::now() + duration;
        while !self.cancelled() && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
    }
}
pub(super) struct Server {
    directory: PathBuf,
    children: Vec<Child>,
    pub server: String,
}
impl Server {
    pub fn start() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "lucent-audio-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(directory.join("pipewire.conf"), r#"
context.properties = { core.daemon = true core.name = pipewire-0 }
context.spa-libs = { audio.convert.* = audioconvert/libspa-audioconvert support.* = support/libspa-support }
context.modules = [
 { name = libpipewire-module-protocol-native }
 { name = libpipewire-module-metadata }
 { name = libpipewire-module-spa-node-factory }
 { name = libpipewire-module-client-node }
 { name = libpipewire-module-access }
 { name = libpipewire-module-adapter }
 { name = libpipewire-module-link-factory }
]
context.objects = [ { factory = metadata args = { metadata.name = default } } ]
"#).unwrap();
        fs::write(directory.join("pipewire-pulse.conf"), r#"
context.spa-libs = { audio.convert.* = audioconvert/libspa-audioconvert support.* = support/libspa-support }
context.modules = [
 { name = libpipewire-module-protocol-native }
 { name = libpipewire-module-client-node }
 { name = libpipewire-module-adapter }
 { name = libpipewire-module-metadata }
 { name = libpipewire-module-protocol-pulse }
]
pulse.properties = { server.address = [ "unix:native" ] }
"#).unwrap();
        let server = format!("unix:{}/pulse/native", directory.display());
        let mut result = Self {
            directory,
            children: vec![],
            server,
        };
        result.boot();
        result
    }
    fn boot(&mut self) {
        // Separate runtime/config/state/cache paths; no session manager or ALSA
        // discovery. These servers cannot connect to the host audio socket.
        for binary in ["pipewire", "pipewire-pulse"] {
            let log = fs::File::create(self.directory.join(format!("{binary}.log"))).unwrap();
            let child = Command::new(binary)
                .arg("--config")
                .arg(self.directory.join(format!("{binary}.conf")))
                .env("XDG_RUNTIME_DIR", &self.directory)
                .env("PIPEWIRE_RUNTIME_DIR", &self.directory)
                .env("PULSE_RUNTIME_PATH", self.directory.join("pulse"))
                .env("XDG_CONFIG_HOME", self.directory.join("config"))
                .env("XDG_STATE_HOME", self.directory.join("state"))
                .env("XDG_CACHE_HOME", self.directory.join("cache"))
                .env_remove("PIPEWIRE_REMOTE")
                .env_remove("PIPEWIRE_CONFIG_DIR")
                .env_remove("PIPEWIRE_CONFIG_NAME")
                .env_remove("PULSE_SERVER")
                .env_remove("DBUS_SESSION_BUS_ADDRESS")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::from(log))
                .spawn()
                .expect("isolated audio tests require pipewire and pipewire-pulse");
            self.children.push(child);
            let socket = self.directory.join(if binary == "pipewire" {
                "pipewire-0"
            } else {
                "pulse/native"
            });
            let start = Instant::now();
            while !socket.exists() {
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "{binary} socket was not created; see {}",
                    self.directory.display()
                );
                assert!(
                    self.children
                        .last_mut()
                        .unwrap()
                        .try_wait()
                        .unwrap()
                        .is_none(),
                    "{binary} exited"
                );
                thread::sleep(Duration::from_millis(20));
            }
        }
        self.pactl(&["info"]);
    }
    fn stop(&mut self) {
        for child in self.children.iter_mut().rev() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.children.clear();
        // A force-stopped private daemon can leave only its own socket files.
        for relative in [
            "pipewire-0",
            "pipewire-0.lock",
            "pipewire-0-manager",
            "pipewire-0-manager.lock",
            "pulse/native",
            "pulse/pid",
        ] {
            let _ = fs::remove_file(self.directory.join(relative));
        }
    }
    fn apply_policy(&self, direction: &str, name: &str) {
        let remote = self.directory.join("pipewire-0");
        let remote = remote.to_str().unwrap();
        let metadata =
            crate::command("pw-metadata", &["--remote", remote, "--name", "default"]).unwrap();
        assert!(metadata.contains(&format!("default.configured.audio.{direction}")));
        // PipeWire records preferred devices; WirePlumber normally resolves
        // that preference. This hardware-free fixture supplies the policy event
        // explicitly, and does not claim to test WirePlumber's selection policy.
        let target = name.trim_end_matches(".monitor");
        assert!(metadata.contains(target));
        crate::command(
            "pw-metadata",
            &[
                "--remote",
                remote,
                "--name",
                "default",
                "0",
                &format!("default.audio.{direction}"),
                &format!(r#"{{"name":"{target}"}}"#),
                "Spa:String:JSON",
            ],
        )
        .unwrap();
    }
    fn pactl(&self, args: &[&str]) -> String {
        let mut full = vec!["--server", self.server.as_str()];
        full.extend(args);
        crate::command_with_timeout("pactl", &full, Duration::from_secs(5)).unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
        let _ = fs::remove_dir_all(&self.directory);
    }
}
struct Watch {
    adapter: Arc<PulseAudio>,
    stop: Arc<Stop>,
    receive: Receiver<Result<AudioSnapshot>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Watch {
    fn start(server: &Server) -> Self {
        let adapter = Arc::new(PulseAudio {
            server: Some(server.server.clone()),
            ..Default::default()
        });
        let stop = Arc::new(Stop::default());
        let (send, receive) = mpsc::channel();
        let a = adapter.clone();
        let s = stop.clone();
        let thread = thread::spawn(move || {
            a.watch(
                &mut |v| {
                    let _ = send.send(v);
                },
                &*s,
            )
        });
        Self {
            adapter,
            stop,
            receive,
            thread: Some(thread),
        }
    }
    fn until(&self, predicate: impl Fn(&AudioSnapshot) -> bool) -> AudioSnapshot {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let value = self
                .receive
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("audio observation deadline");
            match value {
                Ok(value) if predicate(&value) => return value,
                Ok(_) => {}
                Err(e) => eprintln!("audio observer: {e}"),
            }
        }
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.0.store(true, Ordering::Release);
        if let Some(online) = self.adapter.online.lock().unwrap().as_ref() {
            online.wake();
        }
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
#[ignore = "requires private PipeWire and pipewire-pulse daemons; never uses host audio"]
fn audio_protocol_controls_observation_identity_reconnect_and_shutdown() {
    let mut server = Server::start();
    let a = server.pactl(&["load-module", "module-null-sink", "sink_name=lucent_a"]);
    let b = server.pactl(&["load-module", "module-null-sink", "sink_name=lucent_b"]);
    let watch = Watch::start(&server);
    let state = watch.until(|s| {
        s.devices.iter().any(|d| d.key.as_str() == "lucent_a")
            && s.devices.iter().any(|d| d.key.as_str() == "lucent_b")
    });
    let first = state
        .devices
        .iter()
        .find(|d| d.key.as_str() == "lucent_a")
        .unwrap()
        .id;
    let second = state
        .devices
        .iter()
        .find(|d| d.key.as_str() == "lucent_b")
        .unwrap()
        .id;
    watch
        .adapter
        .control(AudioCommand::SetVolume(
            first,
            AudioVolume::new(0.42).unwrap(),
        ))
        .unwrap();
    watch.until(|s| {
        s.device(first)
            .is_some_and(|d| (d.volume.ratio() - 0.42).abs() < 0.001)
    });
    assert!(
        server
            .pactl(&["get-sink-volume", "lucent_a"])
            .contains("42%")
    );
    watch
        .adapter
        .control(AudioCommand::SetMute(first, true))
        .unwrap();
    watch.until(|s| s.device(first).is_some_and(|d| d.muted));
    assert!(server.pactl(&["get-sink-mute", "lucent_a"]).contains("yes"));
    watch
        .adapter
        .control(AudioCommand::SetDefault(second))
        .unwrap();
    server.apply_policy("sink", "lucent_b");
    watch.until(|s| s.default_output == Some(second));
    assert_eq!(server.pactl(&["get-default-sink"]), "lucent_b");
    // Relative commands see each preceding acknowledged state, not one stale snapshot.
    watch
        .adapter
        .control(AudioCommand::SetVolume(
            second,
            AudioVolume::new(0.30).unwrap(),
        ))
        .unwrap();
    for _ in 0..3 {
        watch.adapter.control(AudioCommand::Raise).unwrap();
    }
    watch.until(|s| {
        s.device(second)
            .is_some_and(|d| (d.volume.ratio() - 0.45).abs() < 0.001)
    });
    server.pactl(&["set-sink-volume", "lucent_b", "73%"]);
    watch.until(|s| {
        s.device(second)
            .is_some_and(|d| (d.volume.ratio() - 0.73).abs() < 0.001)
    });
    let input = state
        .devices
        .iter()
        .find(|d| d.key.as_str() == "lucent_a.monitor")
        .unwrap()
        .id;
    watch
        .adapter
        .control(AudioCommand::SetVolume(
            input,
            AudioVolume::new(0.55).unwrap(),
        ))
        .unwrap();
    watch.until(|s| {
        s.device(input)
            .is_some_and(|d| (d.volume.ratio() - 0.55).abs() < 0.001)
    });
    watch
        .adapter
        .control(AudioCommand::SetMute(input, true))
        .unwrap();
    watch.until(|s| s.device(input).is_some_and(|d| d.muted));
    watch
        .adapter
        .control(AudioCommand::SetDefault(input))
        .unwrap();
    server.apply_policy("source", "lucent_a.monitor");
    watch.until(|s| s.default_input == Some(input));
    server.pactl(&["unload-module", &a]);
    watch.until(|s| s.device(first).is_none());
    assert!(
        watch
            .adapter
            .control(AudioCommand::SetMute(first, false))
            .is_err()
    );
    server.pactl(&["load-module", "module-null-sink", "sink_name=lucent_a"]);
    let replaced = watch.until(|s| s.devices.iter().any(|d| d.key.as_str() == "lucent_a"));
    assert_ne!(
        replaced
            .devices
            .iter()
            .find(|d| d.key.as_str() == "lucent_a")
            .unwrap()
            .id,
        first
    );
    server.pactl(&["unload-module", &b]);
    server.stop();
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if watch
            .receive
            .recv_timeout(until.saturating_duration_since(Instant::now()))
            .unwrap()
            .is_err()
        {
            break;
        }
    }
    assert!(
        watch
            .adapter
            .control(AudioCommand::SetMute(second, false))
            .is_err()
    );
    server.boot();
    server.pactl(&["load-module", "module-null-sink", "sink_name=lucent_b"]);
    let state = watch.until(|s| s.devices.iter().any(|d| d.key.as_str() == "lucent_b"));
    let fresh = state
        .devices
        .iter()
        .find(|d| d.key.as_str() == "lucent_b")
        .unwrap()
        .id;
    assert_ne!(fresh, second);
    assert!(
        watch
            .adapter
            .control(AudioCommand::SetMute(second, true))
            .is_err()
    );
    watch
        .adapter
        .control(AudioCommand::SetMute(fresh, true))
        .unwrap();
    watch.until(|s| s.device(fresh).is_some_and(|d| d.muted));
    while watch.receive.try_recv().is_ok() {}
    assert!(
        matches!(
            watch.receive.recv_timeout(Duration::from_secs(6)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ),
        "idle observation must not poll snapshots or reconnect after a finished query deadline"
    );
    let start = Instant::now();
    drop(watch);
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn audio_command_admission_is_bounded_and_offline_is_explicit() {
    let adapter = PulseAudio::default();
    assert!(adapter.control(AudioCommand::Raise).is_err());
    let (send, _receive) = mpsc::sync_channel(REQUEST_LIMIT);
    let (_reader, writer) = UnixStream::pair().unwrap();
    for _ in 0..REQUEST_LIMIT {
        let (reply, _) = mpsc::sync_channel(1);
        send.try_send(Request {
            command: AudioCommand::Raise,
            reply,
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + OPERATION_TIMEOUT,
        })
        .unwrap();
    }
    *adapter.online.lock().unwrap() = Some(Online {
        send,
        wake: Arc::new(writer),
    });
    let start = Instant::now();
    assert!(adapter.control(AudioCommand::Raise).is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
    adapter.watching.store(true, Ordering::Release);
    let mut observation = None;
    adapter.watch(&mut |v| observation = Some(v), &Stop::default());
    assert!(
        observation.unwrap().is_err(),
        "a second watcher must not steal the native connection"
    );
}
