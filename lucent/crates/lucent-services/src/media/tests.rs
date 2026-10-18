//! Real private session-bus fixtures. No test falls back to the host session.
use super::*;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    thread,
};
use zbus::{
    Connection,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

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
struct Stop(AtomicBool);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn wait(&self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while !self.cancelled() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
    }
}
struct Watch {
    adapter: Arc<Mpris>,
    stop: Arc<Stop>,
    values: mpsc::Receiver<Result<MediaSnapshot>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Watch {
    fn new(bus: &Bus, timeout: Duration) -> Self {
        let mut adapter = Mpris::with_bus_address(bus.address.clone());
        adapter.timeout = timeout;
        let adapter = Arc::new(adapter);
        let stop = Arc::new(Stop(AtomicBool::new(false)));
        let (send, values) = mpsc::channel();
        let (a, s) = (adapter.clone(), stop.clone());
        let worker = thread::spawn(move || {
            a.watch(
                &mut |value| {
                    let _ = send.send(value);
                },
                s.as_ref(),
            )
        });
        Self {
            adapter,
            stop,
            values,
            worker: Some(worker),
        }
    }
    fn state(&self, predicate: impl Fn(&MediaSnapshot) -> bool) -> MediaSnapshot {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            match self
                .values
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap()
            {
                Ok(value) if predicate(&value) => return value,
                _ => (),
            }
        }
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.0.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}
#[derive(Debug, PartialEq, Eq)]
enum Call {
    Method(&'static str),
    Seek(String, i64),
}
struct State {
    status: String,
    track: String,
    position: i64,
    length: i64,
    rate: f64,
    can_control: bool,
    can_seek: bool,
    can_pause: bool,
    delay: Duration,
    reads: usize,
    calls: Vec<Call>,
    started: async_channel::Sender<()>,
    read_delay: Duration,
    metadata_reads: usize,
    read_started: async_channel::Sender<()>,
    read_notices: async_channel::Receiver<()>,
    read_block: Option<async_channel::Receiver<()>>,
}
type Shared = Arc<Mutex<State>>;
fn fixture() -> (Shared, async_channel::Receiver<()>) {
    let (started, receive) = async_channel::bounded(8);
    let (read_started, read_notices) = async_channel::bounded(16);
    (
        Arc::new(Mutex::new(State {
            status: "Playing".into(),
            track: "/fixture/a".into(),
            position: 2_000_000,
            length: 120_000_000,
            rate: 1.,
            can_control: true,
            can_seek: true,
            can_pause: true,
            delay: Duration::ZERO,
            reads: 0,
            calls: vec![],
            started,
            read_delay: Duration::ZERO,
            metadata_reads: 0,
            read_started,
            read_notices,
            read_block: None,
        })),
        receive,
    )
}
struct Root;
#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> &str {
        "Native media fixture"
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "org.example.MediaFixture"
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    fn raise(&self) {}
}
struct Player(Shared);
#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.0.lock().unwrap().status.clone()
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        self.0.lock().unwrap().rate
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        let mut state = self.0.lock().unwrap();
        state.reads += 1;
        state.position
    }
    #[zbus(property)]
    async fn metadata(&self) -> HashMap<String, OwnedValue> {
        let (delay, block) = {
            let mut state = self.0.lock().unwrap();
            state.metadata_reads += 1;
            let _ = state.read_started.try_send(());
            (state.read_delay, state.read_block.clone())
        };
        if let Some(block) = block {
            let _ = block.recv().await;
        }
        if !delay.is_zero() {
            async_io::Timer::after(delay).await;
        }
        let state = self.0.lock().unwrap();
        let mut data = HashMap::new();
        data.insert(
            "mpris:trackid".into(),
            Value::from(OwnedObjectPath::try_from(state.track.clone()).unwrap())
                .try_to_owned()
                .unwrap(),
        );
        data.insert(
            "mpris:length".into(),
            Value::from(state.length).try_to_owned().unwrap(),
        );
        data.insert(
            "xesam:title".into(),
            Value::from("Synthetic track").try_to_owned().unwrap(),
        );
        data.insert(
            "xesam:artist".into(),
            Value::from(vec!["Fixture artist"]).try_to_owned().unwrap(),
        );
        data.insert(
            "mpris:artUrl".into(),
            Value::from("file:///tmp/synthetic-media-art.png")
                .try_to_owned()
                .unwrap(),
        );
        data
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        self.0.lock().unwrap().can_control
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.0.lock().unwrap().can_pause
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.0.lock().unwrap().can_seek
    }
    fn play(&self) {
        let mut state = self.0.lock().unwrap();
        state.calls.push(Call::Method("Play"));
        state.status = "Playing".into();
    }
    async fn pause(&self) {
        let delay = {
            let mut state = self.0.lock().unwrap();
            state.calls.push(Call::Method("Pause"));
            let _ = state.started.try_send(());
            state.delay
        };
        if !delay.is_zero() {
            async_io::Timer::after(delay).await;
        }
        self.0.lock().unwrap().status = "Paused".into();
    }
    fn next(&self) {
        self.0.lock().unwrap().calls.push(Call::Method("Next"));
    }
    fn previous(&self) {
        self.0.lock().unwrap().calls.push(Call::Method("Previous"));
    }
    fn stop(&self) {
        let mut state = self.0.lock().unwrap();
        state.calls.push(Call::Method("Stop"));
        state.status = "Stopped".into();
    }
    fn set_position(&self, track_id: OwnedObjectPath, position: i64) {
        let mut state = self.0.lock().unwrap();
        state.calls.push(Call::Seek(track_id.to_string(), position));
        if state.track == track_id.as_str() {
            state.position = position;
        }
    }
}
fn application(bus: &Bus, suffix: &str, state: Shared) -> Connection {
    futures_lite::future::block_on(async {
        zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(format!("org.mpris.MediaPlayer2.{suffix}"))
            .unwrap()
            .serve_at(native::PATH, Root)
            .unwrap()
            .serve_at(native::PATH, Player(state))
            .unwrap()
            .build()
            .await
            .unwrap()
    })
}
fn changed(connection: &Connection, fields: &[&str]) {
    futures_lite::future::block_on(connection.emit_signal(
        None::<&str>,
        native::PATH,
        "org.freedesktop.DBus.Properties",
        "PropertiesChanged",
        &(
            native::PLAYER,
            HashMap::<String, OwnedValue>::new(),
            fields.to_vec(),
        ),
    ))
    .unwrap();
}
fn ready(snapshot: &MediaSnapshot) -> &MediaPlayer {
    snapshot
        .players
        .iter()
        .find(|player| matches!(player.state, MediaPlayerState::Ready(_)))
        .unwrap()
}
fn playback(player: &MediaPlayer) -> &MediaPlayback {
    match &player.state {
        MediaPlayerState::Ready(playback) => playback,
        _ => panic!("Expected native ready state"),
    }
}
fn next_read(state: &Shared) -> async_channel::Receiver<()> {
    let receive = state.lock().unwrap().read_notices.clone();
    while receive.try_recv().is_ok() {}
    receive
}
fn read_started(receive: &async_channel::Receiver<()>) {
    futures_lite::future::block_on(async {
        futures_lite::future::or(
            async {
                receive.recv().await.unwrap();
            },
            async {
                async_io::Timer::after(Duration::from_secs(3)).await;
                panic!("Native metadata read did not begin");
            },
        )
        .await;
    });
}
#[test]
fn native_metadata_bounds_paths_and_optional_values_have_no_fabricated_authority() {
    use snapshot::{Properties, Raw};
    let adapter = Mpris::default();
    let properties = |status: &str, rate: f64| {
        let mut values = Properties::new();
        values.insert(
            "PlaybackStatus".into(),
            Value::from(status).try_to_owned().unwrap(),
        );
        values.insert("Rate".into(), Value::from(rate).try_to_owned().unwrap());
        values
    };
    let root = Properties::new();
    let (player, path) = snapshot::decode(
        &adapter,
        MediaPlayerId::new(1).unwrap(),
        Raw {
            root,
            player: properties("Stopped", 1.),
            sampled_at: Instant::now(),
        },
        None,
        None,
    )
    .unwrap();
    assert!(path.is_none());
    let state = playback(&player);
    assert!(state.track.is_none());
    assert!(state.position.is_none());
    assert!(!state.capabilities.can_play);
    for rate in [f64::NAN, f64::INFINITY, 0.] {
        assert!(
            snapshot::decode(
                &adapter,
                player.id,
                Raw {
                    root: Properties::new(),
                    player: properties("Playing", rate),
                    sampled_at: Instant::now()
                },
                None,
                None
            )
            .is_err()
        );
    }
    let mut data = Properties::new();
    data.insert(
        "mpris:trackid".into(),
        Value::from(
            OwnedObjectPath::try_from("/org/mpris/MediaPlayer2/TrackList/NoTrack").unwrap(),
        )
        .try_to_owned()
        .unwrap(),
    );
    assert!(snapshot::track_path(&data).is_none());
    data.insert(
        "mpris:trackid".into(),
        Value::from(OwnedObjectPath::try_from("/org/mpris/MediaPlayer2/Track/1").unwrap())
            .try_to_owned()
            .unwrap(),
    );
    assert_eq!(
        snapshot::track_path(&data).unwrap().as_str(),
        "/org/mpris/MediaPlayer2/Track/1"
    );
}

#[test]
fn queued_owner_events_never_regress_a_newer_discovery_result() {
    assert!(!native::current_owner_transition(Some(":1.2"), "", ":1.1"));
    assert!(!native::current_owner_transition(
        Some(":1.2"),
        ":1.1",
        ":1.2"
    ));
    assert!(!native::current_owner_transition(Some(":1.2"), ":1.1", ""));
    assert!(native::current_owner_transition(
        Some(":1.2"),
        ":1.2",
        ":1.3"
    ));
    assert!(native::current_owner_transition(Some(":1.2"), ":1.2", ""));
    assert!(native::current_owner_transition(None, "", ":1.1"));
}

#[test]
#[ignore = "owned private MPRIS D-Bus fixture; no host session bus"]
fn media_protocol_discovery_current_controls_and_no_position_polling() {
    let bus = Bus::new();
    let watch = Watch::new(&bus, Duration::from_secs(2));
    watch.state(|snapshot| snapshot.players.is_empty());
    let (state, _) = fixture();
    let _first = application(&bus, "first", state.clone());
    let (other, _) = fixture();
    let _second = application(&bus, "second", other);
    let snapshot = watch.state(|snapshot| {
        snapshot.players.len() == 2
            && snapshot
                .players
                .iter()
                .all(|p| matches!(p.state, MediaPlayerState::Ready(_)))
    });
    let player = ready(&snapshot);
    let selected = snapshot
        .players
        .iter()
        .find(|player| {
            // Native IDs are independent from names and native object paths.
            player.id == snapshot.players[0].id
        })
        .unwrap();
    assert_eq!(
        player.desktop_entry.as_ref().unwrap().as_str(),
        "org.example.MediaFixture.desktop"
    );
    let id = selected.id;
    let track = playback(selected).track.as_ref().unwrap();
    assert_eq!(track.title, "Synthetic track");
    watch.adapter.control(MediaCommand::Play(id)).unwrap();
    watch.state(|s| {
        s.players
            .iter()
            .any(|p| p.id == id && matches!(p.state, MediaPlayerState::Ready(_)))
    });
    watch
        .adapter
        .control(MediaCommand::SetPosition {
            player: id,
            track: track.id,
            position: MediaTime::new(6_000_000).unwrap(),
        })
        .unwrap();
    let snapshot = watch.state(|s| s.players.iter().any(|p| p.id == id && matches!(&p.state, MediaPlayerState::Ready(value) if value.position.is_some_and(|sample| sample.position.micros() == 6_000_000))));
    assert_eq!(
        playback(snapshot.players.iter().find(|p| p.id == id).unwrap())
            .track
            .as_ref()
            .unwrap()
            .id,
        track.id
    );
    // Both initial and post-control reads have finished; quiet position advances
    // must not trigger additional GetAll/Get traffic from the adapter.
    let reads = state.lock().unwrap().reads;
    thread::sleep(Duration::from_millis(350));
    assert_eq!(state.lock().unwrap().reads, reads);
    assert!(
        state
            .lock()
            .unwrap()
            .calls
            .contains(&Call::Seek("/fixture/a".into(), 6_000_000))
    );
}

#[test]
#[ignore = "owned private MPRIS D-Bus fixture; no host session bus"]
fn media_protocol_track_retirement_capability_changes_seeked_and_per_player_failure() {
    let bus = Bus::new();
    let (state, _) = fixture();
    let app = application(&bus, "track", state.clone());
    let watch = Watch::new(&bus, Duration::from_secs(2));
    let initial = watch.state(|snapshot| {
        snapshot
            .players
            .iter()
            .any(|p| matches!(p.state, MediaPlayerState::Ready(_)))
    });
    let id = ready(&initial).id;
    let old_track = playback(ready(&initial)).track.as_ref().unwrap().id;
    state.lock().unwrap().track = "/fixture/b".into();
    changed(&app, &["Metadata"]);
    let changed_snapshot = watch.state(|s| s.players.iter().any(|p| p.id == id && matches!(&p.state, MediaPlayerState::Ready(value) if value.track.as_ref().is_some_and(|track| track.id != old_track))));
    let current_track = playback(ready(&changed_snapshot))
        .track
        .as_ref()
        .unwrap()
        .id;
    assert!(
        watch
            .adapter
            .control(MediaCommand::SetPosition {
                player: id,
                track: old_track,
                position: MediaTime::default()
            })
            .is_err()
    );
    assert!(state.lock().unwrap().calls.is_empty());
    state.lock().unwrap().can_seek = false;
    changed(&app, &["CanSeek"]);
    watch.state(|s| {
        s.players.iter().any(
            |p| matches!(&p.state, MediaPlayerState::Ready(value) if !value.capabilities.can_seek),
        )
    });
    assert!(
        watch
            .adapter
            .control(MediaCommand::SetPosition {
                player: id,
                track: current_track,
                position: MediaTime::default()
            })
            .is_err()
    );
    state.lock().unwrap().position = 9_000_000;
    futures_lite::future::block_on(app.emit_signal(
        None::<&str>,
        native::PATH,
        native::PLAYER,
        "Seeked",
        &(9_000_000i64,),
    ))
    .unwrap();
    watch.state(|s| s.players.iter().any(|p| matches!(&p.state, MediaPlayerState::Ready(value) if value.position.is_some_and(|sample| sample.position.micros() == 9_000_000))));
    state.lock().unwrap().rate = f64::NAN;
    changed(&app, &["Rate"]);
    watch.state(|s| {
        s.players
            .iter()
            .any(|p| p.id == id && matches!(p.state, MediaPlayerState::Unavailable(_)))
    });
    assert!(watch.adapter.control(MediaCommand::Play(id)).is_err());
    state.lock().unwrap().rate = 1.;
    changed(&app, &["Rate"]);
    let recovered = watch.state(|s| {
        s.players
            .iter()
            .any(|p| p.id == id && matches!(p.state, MediaPlayerState::Ready(_)))
    });
    assert_ne!(
        playback(ready(&recovered)).track.as_ref().unwrap().id,
        current_track
    );
    // Same native path revisited is a new observed incarnation.
    state.lock().unwrap().track = "/fixture/a".into();
    changed(&app, &["Metadata"]);
    let revisited = watch.state(|s| s.players.iter().any(|p| matches!(&p.state, MediaPlayerState::Ready(value) if value.track.as_ref().is_some_and(|track| track.id != playback(ready(&recovered)).track.as_ref().unwrap().id))));
    assert_ne!(
        playback(ready(&revisited)).track.as_ref().unwrap().id,
        old_track
    );
}

#[test]
#[ignore = "owned private MPRIS D-Bus fixture; no host session bus"]
fn media_protocol_owner_retirement_during_slow_control_never_retargets_replacement() {
    let bus = Bus::new();
    let (state, started) = fixture();
    state.lock().unwrap().delay = Duration::from_secs(2);
    let app = application(&bus, "reused", state.clone());
    let watch = Watch::new(&bus, Duration::from_millis(900));
    let snapshot = watch.state(|s| {
        s.players
            .iter()
            .any(|p| matches!(p.state, MediaPlayerState::Ready(_)))
    });
    let old = ready(&snapshot).id;
    let adapter = watch.adapter.clone();
    let command = thread::spawn(move || adapter.control(MediaCommand::Pause(old)));
    futures_lite::future::block_on(async {
        futures_lite::future::or(
            async {
                started.recv().await.unwrap();
            },
            async {
                async_io::Timer::after(Duration::from_secs(3)).await;
                panic!("Native slow command did not begin");
            },
        )
        .await;
    });
    let retired_at = Instant::now();
    futures_lite::future::block_on(app.release_name("org.mpris.MediaPlayer2.reused")).unwrap();
    let (replacement, _) = fixture();
    let _new_app = application(&bus, "reused", replacement.clone());
    let current = watch.state(|s| {
        s.players
            .iter()
            .any(|p| p.id != old && matches!(p.state, MediaPlayerState::Ready(_)))
    });
    assert!(
        retired_at.elapsed() < Duration::from_millis(700),
        "Owner events waited for the old method"
    );
    assert!(command.join().unwrap().is_err());
    assert!(watch.adapter.control(MediaCommand::Next(old)).is_err());
    watch
        .adapter
        .control(MediaCommand::Next(ready(&current).id))
        .unwrap();
    assert_eq!(
        replacement.lock().unwrap().calls,
        vec![Call::Method("Next")]
    );
    assert_eq!(state.lock().unwrap().calls, vec![Call::Method("Pause")]);
}

#[test]
#[ignore = "owned private MPRIS D-Bus fixture; no host session bus"]
fn media_protocol_timeout_keeps_observation_live_and_bus_loss_is_not_no_player() {
    let mut bus = Bus::new();
    let (state, started) = fixture();
    state.lock().unwrap().delay = Duration::from_secs(2);
    let app = application(&bus, "slow", state.clone());
    let watch = Watch::new(&bus, Duration::from_millis(350));
    let initial = watch.state(|s| {
        s.players
            .iter()
            .any(|p| matches!(p.state, MediaPlayerState::Ready(_)))
    });
    let id = ready(&initial).id;
    let adapter = watch.adapter.clone();
    let start = Instant::now();
    let command = thread::spawn(move || adapter.control(MediaCommand::Pause(id)));
    futures_lite::future::block_on(async {
        futures_lite::future::or(
            async {
                started.recv().await.unwrap();
            },
            async {
                async_io::Timer::after(Duration::from_secs(3)).await;
                panic!("Native slow command did not begin");
            },
        )
        .await;
    });
    state.lock().unwrap().position = 17_000_000;
    futures_lite::future::block_on(app.emit_signal(
        None::<&str>,
        native::PATH,
        native::PLAYER,
        "Seeked",
        &(17_000_000i64,),
    ))
    .unwrap();
    watch.state(|s| s.players.iter().any(|p| matches!(&p.state, MediaPlayerState::Ready(value) if value.position.is_some_and(|sample| sample.position.micros() == 17_000_000))));
    assert!(command.join().unwrap().is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(state.lock().unwrap().calls, vec![Call::Method("Pause")]);
    bus.child.kill().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if watch
            .values
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
            .is_err()
        {
            break;
        }
    }
    assert!(watch.adapter.control(MediaCommand::Play(id)).is_err());
}

#[test]
#[ignore = "owned private MPRIS D-Bus fixture; no host session bus"]
fn media_protocol_delayed_seek_validation_cannot_reuse_retired_track_path() {
    let bus = Bus::new();
    let (state, _) = fixture();
    let app = application(&bus, "aba", state.clone());
    let watch = Watch::new(&bus, Duration::from_secs(2));
    let initial = watch.state(|s| {
        s.players
            .iter()
            .any(|p| matches!(p.state, MediaPlayerState::Ready(_)))
    });
    let player = ready(&initial);
    let id = player.id;
    let track = playback(player).track.as_ref().unwrap().id;
    let notices = next_read(&state);
    state.lock().unwrap().read_delay = Duration::from_millis(500);
    let adapter = watch.adapter.clone();
    let command = thread::spawn(move || {
        adapter.control(MediaCommand::SetPosition {
            player: id,
            track,
            position: MediaTime::new(25_000_000).unwrap(),
        })
    });
    read_started(&notices);
    // The validation GetAll is already dispatched but has not returned its
    // metadata. Reusing the same native path must not restore this old token.
    state.lock().unwrap().track = "/fixture/b".into();
    changed(&app, &["Metadata"]);
    watch.state(|s| {
        s.players
            .iter()
            .any(|p| matches!(&p.state, MediaPlayerState::Ready(value) if value.track.is_none()))
    });
    state.lock().unwrap().track = "/fixture/a".into();
    changed(&app, &["Metadata"]);
    assert!(command.join().unwrap().is_err());
    let recovered = watch.state(|s| s.players.iter().any(|p| matches!(&p.state, MediaPlayerState::Ready(value) if value.track.as_ref().is_some_and(|current| current.id != track))));
    assert_ne!(
        playback(ready(&recovered)).track.as_ref().unwrap().id,
        track
    );
    assert!(
        state.lock().unwrap().calls.is_empty(),
        "Old seek reached the reused native track path"
    );
}

#[test]
#[ignore = "owned private MPRIS D-Bus fixture; no host session bus"]
fn media_protocol_signal_burst_coalesces_behind_one_accepted_property_read() {
    let bus = Bus::new();
    let (state, _) = fixture();
    let app = application(&bus, "burst", state.clone());
    let watch = Watch::new(&bus, Duration::from_secs(3));
    watch.state(|s| {
        s.players
            .iter()
            .any(|p| matches!(p.state, MediaPlayerState::Ready(_)))
    });
    let notices = next_read(&state);
    let baseline = state.lock().unwrap().metadata_reads;
    let (release, block) = async_channel::bounded(1);
    state.lock().unwrap().read_block = Some(block);
    changed(&app, &["Rate"]);
    read_started(&notices);
    for _ in 0..80 {
        changed(&app, &["Rate"]);
    }
    // Let the owning loop consume queued property notices while GetAll remains
    // blocked in our peer. A cancel/restart implementation dispatches many calls.
    thread::sleep(Duration::from_millis(150));
    assert_eq!(state.lock().unwrap().metadata_reads, baseline + 1);
    {
        let mut state = state.lock().unwrap();
        state.position = 31_000_000;
        state.read_block = None;
    }
    release.send_blocking(()).unwrap();
    watch.state(|s| s.players.iter().any(|p| matches!(&p.state, MediaPlayerState::Ready(value) if value.position.is_some_and(|sample| sample.position.micros() == 31_000_000))));
    assert!(state.lock().unwrap().metadata_reads <= baseline + 3);
}
