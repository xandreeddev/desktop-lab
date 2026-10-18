//! Owned private-bus fixtures; never contact host power services or backlights.
use super::*;
use std::{
    io::{BufRead, BufReader},
    os::unix::fs::DirBuilderExt,
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
const DISPLAY: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const PROFILE_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const SESSION_PATH: &str = "/org/freedesktop/login1/session/fixture";
struct Bus {
    child: Child,
    address: String,
}
impl Bus {
    fn new() -> Self {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Self {
            child,
            address: address.trim().into(),
        }
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..1024 {
            let path = std::env::temp_dir().join(format!(
                "lucent-power-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("Power fixture directory creation failed: {error}"),
            }
        }
        panic!("Power fixture directory allocation exhausted");
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
struct State {
    percentage: f64,
    level: u32,
    active: String,
    profiles: Vec<String>,
    deny: bool,
    actions: usize,
    backlights: PathBuf,
    suspended: async_channel::Sender<()>,
    release: async_channel::Receiver<()>,
}
type Shared = Arc<Mutex<State>>;
fn state(
    root: &Directory,
) -> (
    Shared,
    async_channel::Receiver<()>,
    async_channel::Sender<()>,
) {
    let panel = root.0.join("panel");
    std::fs::create_dir(&panel).unwrap();
    std::fs::write(panel.join("max_brightness"), "1000\n").unwrap();
    std::fs::write(panel.join("brightness"), "500\n").unwrap();
    let (suspended, ready) = async_channel::bounded(1);
    let (done, release) = async_channel::bounded(1);
    (
        Arc::new(Mutex::new(State {
            percentage: 72.,
            level: 1,
            active: "balanced".into(),
            profiles: vec!["balanced".into(), "power-saver".into()],
            deny: false,
            actions: 0,
            backlights: root.0.clone(),
            suspended,
            release,
        })),
        ready,
        done,
    )
}
struct Supply;
#[zbus::interface(name = "org.freedesktop.UPower")]
impl Supply {
    #[zbus(property)]
    fn on_battery(&self) -> bool {
        true
    }
}
struct Battery(Shared);
#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl Battery {
    #[zbus(property)]
    fn is_present(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn percentage(&self) -> f64 {
        self.0.lock().unwrap().percentage
    }
    #[zbus(property)]
    fn battery_level(&self) -> u32 {
        self.0.lock().unwrap().level
    }
    #[zbus(property)]
    fn state(&self) -> u32 {
        2
    }
    #[zbus(property)]
    fn time_to_empty(&self) -> i64 {
        5400
    }
    #[zbus(property)]
    fn time_to_full(&self) -> i64 {
        0
    }
}
struct Profiles(Shared);
#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl Profiles {
    #[zbus(property)]
    fn profiles(&self) -> Vec<std::collections::HashMap<String, OwnedValue>> {
        self.0
            .lock()
            .unwrap()
            .profiles
            .iter()
            .map(|name| {
                std::collections::HashMap::from([(
                    "Profile".into(),
                    Value::from(name.clone()).try_to_owned().unwrap(),
                )])
            })
            .collect()
    }
    #[zbus(property)]
    fn active_profile(&self) -> String {
        self.0.lock().unwrap().active.clone()
    }
    #[zbus(property)]
    fn set_active_profile(&self, name: String) -> zbus::fdo::Result<()> {
        let mut state = self.0.lock().unwrap();
        if state.deny {
            return Err(zbus::fdo::Error::AccessDenied(
                "fixture private metadata".into(),
            ));
        }
        if !state.profiles.contains(&name) {
            return Err(zbus::fdo::Error::InvalidArgs("retired profile".into()));
        }
        state.active = name;
        Ok(())
    }
    #[zbus(property)]
    fn performance_degraded(&self) -> &str {
        ""
    }
}
struct Session(Shared);
#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Session {
    #[zbus(property)]
    fn user(&self) -> (u32, OwnedObjectPath) {
        (
            unsafe { libc::getuid() },
            "/org/freedesktop/login1/user/fixture".try_into().unwrap(),
        )
    }
    #[zbus(property)]
    fn active(&self) -> bool {
        true
    }
    #[zbus(property, name = "Type")]
    fn session_type(&self) -> &str {
        "wayland"
    }
    fn set_brightness(
        &self,
        subsystem: String,
        name: String,
        brightness: u32,
    ) -> zbus::fdo::Result<()> {
        let state = self.0.lock().unwrap();
        if subsystem != "backlight" || name != "panel" || brightness > 1000 {
            return Err(zbus::fdo::Error::InvalidArgs("bad fixture control".into()));
        }
        std::fs::write(
            state.backlights.join("panel/brightness"),
            brightness.to_string(),
        )
        .unwrap();
        Ok(())
    }
}
struct Manager(Shared);
#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
    fn can_suspend(&self) -> &str {
        "challenge"
    }
    fn can_hibernate(&self) -> &str {
        "na"
    }
    fn can_hybrid_sleep(&self) -> &str {
        "no"
    }
    fn can_reboot(&self) -> &str {
        "yes"
    }
    fn can_power_off(&self) -> &str {
        "yes"
    }
    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(&self, _pid: u32) -> OwnedObjectPath {
        SESSION_PATH.try_into().unwrap()
    }
    async fn suspend(
        &self,
        interactive: bool,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        if !interactive
            || !header
                .primary()
                .flags()
                .contains(zbus::message::Flags::AllowInteractiveAuth)
        {
            return Err(zbus::fdo::Error::AccessDenied(
                "interactive flag missing".into(),
            ));
        }
        let (ready, release) = {
            let mut state = self.0.lock().unwrap();
            state.actions += 1;
            (state.suspended.clone(), state.release.clone())
        };
        ready.send(()).await.unwrap();
        release.recv().await.unwrap();
        Ok(())
    }
}
async fn server(bus: &Bus, state: &Shared) -> zbus::Connection {
    zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name(native::UPOWER)
        .unwrap()
        .name(native::PROFILES)
        .unwrap()
        .name(native::LOGIN)
        .unwrap()
        .serve_at("/org/freedesktop/UPower", Supply)
        .unwrap()
        .serve_at(DISPLAY, Battery(state.clone()))
        .unwrap()
        .serve_at(PROFILE_PATH, Profiles(state.clone()))
        .unwrap()
        .serve_at("/org/freedesktop/login1", Manager(state.clone()))
        .unwrap()
        .serve_at(SESSION_PATH, Session(state.clone()))
        .unwrap()
        .build()
        .await
        .unwrap()
}
#[derive(Default)]
struct Stop(AtomicBool);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn wait(&self, duration: Duration) {
        let end = Instant::now() + duration;
        while !self.cancelled() && Instant::now() < end {
            thread::sleep(Duration::from_millis(10));
        }
    }
}
struct Observer {
    adapter: Arc<PowerServices>,
    stop: Arc<Stop>,
    thread: Option<thread::JoinHandle<()>>,
    events: mpsc::Receiver<Result<PowerSnapshot>>,
}
impl Observer {
    fn new(bus: &Bus, root: &Directory) -> Self {
        let adapter = Arc::new(PowerServices::with_fixture(
            bus.address.clone(),
            root.0.clone(),
        ));
        let stop = Arc::new(Stop::default());
        let (send, events) = mpsc::sync_channel(32);
        let a = adapter.clone();
        let s = stop.clone();
        let thread = thread::spawn(move || {
            a.watch(
                &mut |v| {
                    let _ = send.try_send(v);
                },
                s.as_ref(),
            )
        });
        Self {
            adapter,
            stop,
            thread: Some(thread),
            events,
        }
    }
    fn state(&self, predicate: impl Fn(&PowerSnapshot) -> bool) -> PowerSnapshot {
        let deadline = Instant::now() + Duration::from_secs(7);
        loop {
            let event = self
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("Power observation timed out");
            if let Ok(value) = event
                && predicate(&value)
            {
                return value;
            }
        }
    }
    fn finished(&self, id: PowerOperationId) -> PowerSnapshot {
        self.state(|s| {
            s.operation
                .as_ref()
                .is_some_and(|o| o.id == id && o.phase != PowerOperationPhase::InProgress)
        })
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        self.stop.0.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn ready(state: &PowerSnapshot) -> bool {
    matches!(state.profiles, PowerCapability::Ready(_))
        && matches!(state.brightness, PowerCapability::Ready(_))
}
async fn changed(
    server: &zbus::Connection,
    path: &str,
    interface: &str,
    fields: std::collections::HashMap<String, OwnedValue>,
) {
    server
        .emit_signal(
            None::<&str>,
            path,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(interface, fields, Vec::<String>::new()),
        )
        .await
        .unwrap();
}
#[test]
#[ignore = "owned isolated power D-Bus fixture; never host hardware"]
fn power_protocol_native_profile_brightness_and_permission_failure() {
    let bus = Bus::new();
    let root = Directory::new();
    let (state, _, _) = state(&root);
    let _server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, &root);
    let snapshot = watch.state(ready);
    let profile = match &snapshot.profiles {
        PowerCapability::Ready(p) => {
            p.profiles
                .iter()
                .find(|p| p.name == "power-saver")
                .unwrap()
                .id
        }
        _ => unreachable!(),
    };
    let brightness = match snapshot.brightness {
        PowerCapability::Ready(d) => d[0].id,
        _ => unreachable!(),
    };
    let id = watch
        .adapter
        .control(PowerCommand::SetProfile(profile))
        .unwrap();
    let completed = watch.finished(id);
    assert_eq!(
        completed.operation.unwrap().phase,
        PowerOperationPhase::Succeeded
    );
    assert!(matches!(completed.profiles,PowerCapability::Ready(p) if p.active==Some(profile)));
    let id = watch
        .adapter
        .control(PowerCommand::SetBrightness(
            brightness,
            Percentage::new(37.).unwrap(),
        ))
        .unwrap();
    let completed = watch.finished(id);
    assert!(
        matches!(completed.brightness,PowerCapability::Ready(d) if d[0].brightness.value()==37.)
    );
    state.lock().unwrap().deny = true;
    let id = watch
        .adapter
        .control(PowerCommand::SetProfile(profile))
        .unwrap();
    let completed = watch.finished(id);
    assert!(
        matches!(completed.operation.unwrap().phase,PowerOperationPhase::Failed(error) if error.contains("permission denied")&&!error.contains("private metadata"))
    );
    assert_eq!(
        state.lock().unwrap().actions,
        0,
        "Observation and profile/backlight controls must not request sleep"
    );
}
#[test]
#[ignore = "owned isolated power D-Bus fixture; never host hardware"]
fn power_protocol_pending_authorization_keeps_signals_and_retirement_live() {
    let bus = Bus::new();
    let root = Directory::new();
    let (state, ready_action, release) = state(&root);
    let server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, &root);
    watch.state(ready);
    let start = Instant::now();
    let id = watch
        .adapter
        .control(PowerCommand::Session(PowerAction::Suspend))
        .unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
    futures_lite::future::block_on(ready_action.recv()).unwrap();
    {
        let mut s = state.lock().unwrap();
        s.percentage = 30.;
        s.level = 4;
    }
    futures_lite::future::block_on(changed(
        &server,
        DISPLAY,
        native::DEVICE,
        std::collections::HashMap::from([(
            "Percentage".into(),
            Value::from(30.).try_to_owned().unwrap(),
        )]),
    ));
    let pending = watch
        .state(|s| matches!(&s.battery,PowerCapability::Ready(Some(b)) if b.charge.value()==30.));
    assert_eq!(
        pending.operation.unwrap().phase,
        PowerOperationPhase::InProgress
    );
    assert!(
        matches!(pending.battery,PowerCapability::Ready(Some(b)) if b.level==BatteryLevel::Critical)
    );
    assert!(
        watch
            .adapter
            .control(PowerCommand::Session(PowerAction::Suspend))
            .is_err()
    );
    release.try_send(()).unwrap();
    assert_eq!(
        watch.finished(id).operation.unwrap().phase,
        PowerOperationPhase::Succeeded
    );
    assert_eq!(state.lock().unwrap().actions, 1);
}
#[test]
#[ignore = "owned isolated power D-Bus fixture; never host hardware"]
fn power_protocol_absent_services_stay_observable_and_cancel_promptly() {
    let bus = Bus::new();
    let root = Directory::new();
    let watch = Observer::new(&bus, &root);
    let snapshot = watch.state(|_| true);
    assert!(matches!(snapshot.battery, PowerCapability::Unavailable(_)));
    assert!(matches!(snapshot.brightness,PowerCapability::Ready(d) if d.is_empty()));
    assert!(
        watch
            .adapter
            .control(PowerCommand::Session(PowerAction::Suspend))
            .is_err()
    );
    let start = Instant::now();
    drop(watch);
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
#[ignore = "owned isolated power D-Bus fixture; never host hardware"]
fn power_protocol_retired_profile_and_owner_handles_cannot_retarget() {
    let bus = Bus::new();
    let root = Directory::new();
    let (state, _, _) = state(&root);
    let server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, &root);
    let initial = watch.state(ready);
    let old = match initial.profiles {
        PowerCapability::Ready(p) => {
            p.profiles
                .iter()
                .find(|p| p.name == "power-saver")
                .unwrap()
                .id
        }
        _ => unreachable!(),
    };
    state.lock().unwrap().profiles = vec!["balanced".into()];
    futures_lite::future::block_on(changed(
        &server,
        PROFILE_PATH,
        native::PROFILES,
        std::collections::HashMap::new(),
    ));
    watch.state(|s| matches!(&s.profiles,PowerCapability::Ready(p) if p.profiles.len()==1));
    assert!(
        watch
            .adapter
            .control(PowerCommand::SetProfile(old))
            .is_err()
    );
    state.lock().unwrap().profiles.push("power-saver".into());
    futures_lite::future::block_on(changed(
        &server,
        PROFILE_PATH,
        native::PROFILES,
        std::collections::HashMap::new(),
    ));
    let current =
        watch.state(|s| matches!(&s.profiles,PowerCapability::Ready(p) if p.profiles.len()==2));
    let replacement = match current.profiles {
        PowerCapability::Ready(p) => {
            p.profiles
                .iter()
                .find(|p| p.name == "power-saver")
                .unwrap()
                .id
        }
        _ => unreachable!(),
    };
    assert_ne!(old, replacement);
    futures_lite::future::block_on(server.release_name(native::LOGIN)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let event = watch
            .events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        if event.is_err() {
            break;
        }
    }
    let reconnected = watch.state(|s| {
        matches!(s.profiles, PowerCapability::Ready(_))
            && matches!(s.session, PowerCapability::Unavailable(_))
    });
    assert!(matches!(
        reconnected.brightness,
        PowerCapability::Unavailable(_)
    ));
    assert!(
        watch
            .adapter
            .control(PowerCommand::SetProfile(replacement))
            .is_err()
    );
}
