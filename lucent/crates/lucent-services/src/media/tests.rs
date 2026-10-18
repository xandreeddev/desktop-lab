//! Real private session-bus fixtures. No test falls back to the host session.
use super::*;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
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
        Self::from_adapter(Arc::new(adapter))
    }
    fn from_adapter(adapter: Arc<Mpris>) -> Self {
        Self::with_hook(adapter, |_| {})
    }
    fn with_hook(
        adapter: Arc<Mpris>,
        mut hook: impl FnMut(&Result<MediaSnapshot>) + Send + 'static,
    ) -> Self {
        let stop = Arc::new(Stop(AtomicBool::new(false)));
        let (send, values) = mpsc::channel();
        let (a, s) = (adapter.clone(), stop.clone());
        let worker = thread::spawn(move || {
            a.watch(
                &mut |value| {
                    hook(&value);
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
    fn admit(&self, command: MediaCommand) -> MediaOperationId {
        // A receiver can observe the callback's channel send just before that
        // callback returns. Retry only local busy admission, never an admitted
        // command, while the owner marks the prior terminal as published.
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match self.adapter.start(command) {
                Ok(id) => return id,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
                Err(error) => panic!("Media admission did not reopen: {error:?}"),
            }
        }
    }
    fn terminal(&self, id: MediaOperationId) -> MediaSnapshot {
        self.state(|snapshot| {
            snapshot.operation.as_ref().is_some_and(|operation| {
                operation.id == id && operation.phase != MediaOperationPhase::InProgress
            })
        })
    }
    fn refreshed_after(&self, terminal: &MediaSnapshot, player: MediaPlayerId) -> MediaSnapshot {
        let sampled_at = terminal
            .players
            .iter()
            .find(|value| value.id == player)
            .and_then(|value| match &value.state {
                MediaPlayerState::Ready(playback) => {
                    playback.position.map(|sample| sample.sampled_at)
                }
                _ => None,
            })
            .expect("Fixture player must have a position sample");
        self.state(|snapshot| snapshot.operation == terminal.operation
            && snapshot.players.iter().any(|value| value.id == player && matches!(&value.state,
                MediaPlayerState::Ready(playback) if playback.position.is_some_and(|sample| sample.sampled_at > sampled_at))))
    }
    fn acknowledge(&self, command: MediaCommand) -> MediaSnapshot {
        let id = self.admit(command);
        let terminal = self.terminal(id);
        assert_eq!(
            terminal.operation.as_ref().unwrap().phase,
            MediaOperationPhase::Acknowledged
        );
        // Completion is separate from the follow-up native observation. This
        // also proves the exact terminal survives a property refresh.
        self.refreshed_after(&terminal, command.player())
    }
    fn fail(&self, command: MediaCommand) -> MediaOperation {
        let id = self.admit(command);
        let terminal = self.terminal(id).operation.unwrap();
        assert_eq!(terminal.command, command);
        assert!(matches!(terminal.phase, MediaOperationPhase::Failed(_)));
        terminal
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
    read_block_once: Option<async_channel::Receiver<()>>,
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
            read_block_once: None,
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
            let block = state
                .read_block_once
                .take()
                .or_else(|| state.read_block.clone());
            (state.read_delay, block)
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
fn media_admission_rejects_contended_locks_without_waiting_or_orphan_requests() {
    let adapter = Mpris::default();
    let player = MediaPlayerId::new(1).unwrap();
    let (send, receive) = async_channel::bounded(1);
    *adapter.online.lock().unwrap() = Some(send);
    let first = adapter.start(MediaCommand::Play(player)).unwrap();
    receive.try_recv().unwrap();
    adapter.finish(first, Ok(()));
    let terminal = adapter.operation().unwrap();
    adapter.published(Some(&terminal));
    {
        // Keep each mutex held by the caller itself. A blocking lock cannot
        // make progress here; local admission must reject rather than wait.
        let _online = adapter.online.lock().unwrap();
        assert!(adapter.start(MediaCommand::Next(player)).is_err());
        assert_eq!(adapter.operation(), Some(terminal.clone()));
        assert!(receive.try_recv().is_err());
    }
    {
        let operation = adapter.operation.lock().unwrap();
        assert!(adapter.start(MediaCommand::Next(player)).is_err());
        assert_eq!(operation.as_ref().unwrap().value, terminal);
        assert!(receive.try_recv().is_err());
    }
    let next = adapter.start(MediaCommand::Next(player)).unwrap();
    assert!(next.value() > first.value());
    assert_eq!(receive.try_recv().unwrap().id, next);
    assert!(receive.try_recv().is_err());
    assert_eq!(adapter.operation().unwrap().id, next);
}

#[test]
fn media_admission_rejects_pressure_until_the_exact_terminal_is_published() {
    let adapter = Mpris::default();
    let player = MediaPlayerId::new(1).unwrap();
    let pause = MediaCommand::Pause(player);
    assert!(adapter.start(pause).is_err());
    assert!(adapter.operation().is_none());
    let (send, receive) = async_channel::bounded(1);
    *adapter.online.lock().unwrap() = Some(send);
    let first = adapter.start(pause).unwrap();
    let request = receive.try_recv().unwrap();
    assert_eq!(request.id, first);
    assert_eq!(request.command, pause);
    for _ in 0..64 {
        assert!(adapter.start(MediaCommand::Next(player)).is_err());
    }
    assert!(
        receive.try_recv().is_err(),
        "Rejected pressure must never enqueue a native action"
    );
    adapter.finish(first, Ok(()));
    let terminal = adapter.operation().unwrap();
    assert_eq!(terminal.phase, MediaOperationPhase::Acknowledged);
    assert!(adapter.start(MediaCommand::Next(player)).is_err());
    let mut wrong = terminal.clone();
    wrong.phase = MediaOperationPhase::InProgress;
    adapter.published(Some(&wrong));
    wrong = terminal.clone();
    wrong.id = MediaOperationId::new(first.value() + 1).unwrap();
    adapter.published(Some(&wrong));
    adapter.published(None);
    assert!(adapter.start(MediaCommand::Next(player)).is_err());
    assert!(receive.try_recv().is_err());
    adapter.published(Some(&terminal));
    let next = adapter.start(MediaCommand::Next(player)).unwrap();
    assert!(next.value() > first.value());
    adapter.finish(first, Err(DomainError::Failed("Late prior result".into())));
    adapter.published(Some(&terminal));
    assert!(adapter.start(MediaCommand::Stop(player)).is_err());
    let current = adapter.operation().unwrap();
    assert_eq!(current.id, next);
    assert_eq!(current.phase, MediaOperationPhase::InProgress);
    assert_eq!(receive.try_recv().unwrap().id, next);
    assert!(receive.try_recv().is_err());
}

#[test]
fn media_admission_transport_failure_cannot_replace_a_published_outcome() {
    let adapter = Mpris::default();
    let player = MediaPlayerId::new(1).unwrap();
    let (send, receive) = async_channel::bounded(1);
    *adapter.online.lock().unwrap() = Some(send.clone());
    let first = adapter.start(MediaCommand::Play(player)).unwrap();
    receive.try_recv().unwrap();
    adapter.finish(first, Ok(()));
    let terminal = adapter.operation().unwrap();
    adapter.published(Some(&terminal));
    let occupied = MediaOperationId::new(u64::MAX).unwrap();
    assert!(
        send.try_send(Request {
            id: occupied,
            command: MediaCommand::Stop(player),
            deadline: Instant::now() + DEADLINE
        })
        .is_ok()
    );
    assert!(adapter.start(MediaCommand::Next(player)).is_err());
    assert_eq!(adapter.operation(), Some(terminal.clone()));
    assert_eq!(receive.try_recv().unwrap().id, occupied);
    assert!(
        receive.try_recv().is_err(),
        "A full-channel rejection must leave no orphan request"
    );
    drop(receive);
    assert!(adapter.start(MediaCommand::Next(player)).is_err());
    assert_eq!(adapter.operation(), Some(terminal));
}

#[test]
fn media_admission_disconnect_retires_exact_identity_until_recovery_publication() {
    let adapter = Mpris::default();
    let player = MediaPlayerId::new(1).unwrap();
    let command = MediaCommand::Pause(player);
    let (send, receive) = async_channel::bounded(1);
    *adapter.online.lock().unwrap() = Some(send);
    let first = adapter.start(command).unwrap();
    receive.try_recv().unwrap();
    adapter.offline();
    let terminal = adapter.operation().unwrap();
    assert_eq!(terminal.id, first);
    assert_eq!(terminal.command, command);
    assert!(matches!(
        terminal.phase,
        MediaOperationPhase::Failed(DomainError::Unavailable(_))
    ));
    adapter.finish(first, Ok(()));
    assert_eq!(
        adapter.operation(),
        Some(terminal.clone()),
        "A late success cannot revive retired authority"
    );
    assert!(adapter.start(MediaCommand::Play(player)).is_err());
    let (send, receive) = async_channel::bounded(1);
    *adapter.online.lock().unwrap() = Some(send);
    assert!(adapter.start(MediaCommand::Play(player)).is_err());
    assert!(receive.try_recv().is_err());
    adapter.published(Some(&terminal));
    let recovered = adapter.start(MediaCommand::Play(player)).unwrap();
    assert!(recovered.value() > first.value());
    assert_eq!(receive.try_recv().unwrap().id, recovered);
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
    watch.acknowledge(MediaCommand::Play(id));
    let snapshot = watch.acknowledge(MediaCommand::SetPosition {
        player: id,
        track: track.id,
        position: MediaTime::new(6_000_000).unwrap(),
    });
    assert_eq!(
        playback(
            snapshot
                .players
                .iter()
                .find(|player| player.id == id)
                .unwrap()
        )
        .position
        .unwrap()
        .position
        .micros(),
        6_000_000
    );
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
    watch.fail(MediaCommand::SetPosition {
        player: id,
        track: old_track,
        position: MediaTime::default(),
    });
    assert!(state.lock().unwrap().calls.is_empty());
    state.lock().unwrap().can_seek = false;
    changed(&app, &["CanSeek"]);
    watch.state(|s| {
        s.players.iter().any(
            |p| matches!(&p.state, MediaPlayerState::Ready(value) if !value.capabilities.can_seek),
        )
    });
    watch.fail(MediaCommand::SetPosition {
        player: id,
        track: current_track,
        position: MediaTime::default(),
    });
    assert!(state.lock().unwrap().calls.is_empty());
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
    watch.fail(MediaCommand::Play(id));
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
    let operation = watch.adapter.start(MediaCommand::Pause(old)).unwrap();
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
    let retired = current.operation.as_ref().unwrap();
    assert_eq!(retired.id, operation);
    assert!(matches!(retired.phase, MediaOperationPhase::Failed(_)));
    watch.fail(MediaCommand::Next(old));
    watch.acknowledge(MediaCommand::Next(ready(&current).id));
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
    let start = Instant::now();
    let operation = watch.adapter.start(MediaCommand::Pause(id)).unwrap();
    assert!(
        start.elapsed() < Duration::from_millis(200),
        "Admission waited for native validation or a method reply"
    );
    for _ in 0..32 {
        assert!(watch.adapter.start(MediaCommand::Next(id)).is_err());
    }
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
    let seeked = watch.state(|s| s.players.iter().any(|p| matches!(&p.state, MediaPlayerState::Ready(value) if value.position.is_some_and(|sample| sample.position.micros() == 17_000_000))));
    assert!(
        seeked.operation.as_ref().is_some_and(
            |value| value.id == operation && value.phase == MediaOperationPhase::InProgress
        ),
        "Seeked waited behind the pending native method"
    );
    let terminal = watch.terminal(operation);
    assert!(matches!(
        terminal.operation.as_ref().unwrap().phase,
        MediaOperationPhase::Failed(DomainError::Unavailable(_))
    ));
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
    assert!(watch.adapter.start(MediaCommand::Play(id)).is_err());
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
    let admitted_at = Instant::now();
    let operation = watch
        .adapter
        .start(MediaCommand::SetPosition {
            player: id,
            track,
            position: MediaTime::new(25_000_000).unwrap(),
        })
        .unwrap();
    assert!(
        admitted_at.elapsed() < Duration::from_millis(200),
        "Seek admission waited for its delayed GetAll"
    );
    read_started(&notices);
    // The validation GetAll is already dispatched but has not returned its
    // metadata. Reusing the same native path must not restore this old token.
    state.lock().unwrap().track = "/fixture/b".into();
    changed(&app, &["Metadata"]);
    let retired = watch.state(|s| {
        s.players
            .iter()
            .any(|p| matches!(&p.state, MediaPlayerState::Ready(value) if value.track.is_none()))
    });
    assert_eq!(retired.operation.as_ref().unwrap().id, operation);
    assert!(matches!(
        retired.operation.as_ref().unwrap().phase,
        MediaOperationPhase::Failed(_)
    ));
    state.lock().unwrap().track = "/fixture/a".into();
    changed(&app, &["Metadata"]);
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

#[test]
#[ignore = "owned private MPRIS D-Bus fixture; no host session bus"]
fn media_protocol_terminal_delivery_precedes_readmission_and_pressure_never_dispatches() {
    let bus = Bus::new();
    let (state, _) = fixture();
    let _app = application(&bus, "publication", state.clone());
    let adapter = Arc::new(Mpris::with_bus_address(bus.address.clone()));
    let (entered, terminal_values) = mpsc::sync_channel(1);
    let (release, held) = mpsc::sync_channel(1);
    let mut first_terminal = true;
    let watch = Watch::with_hook(adapter, move |result| {
        if first_terminal
            && let Ok(snapshot) = result
            && let Some(operation) = snapshot.operation.as_ref()
            && operation.phase != MediaOperationPhase::InProgress
        {
            first_terminal = false;
            let _ = entered.send(operation.clone());
            // A short test-only callback gate distinguishes "about to emit"
            // from "published". Missing release cannot strand the test worker.
            let _ = held.recv_timeout(Duration::from_secs(3));
        }
    });
    let initial = watch.state(|snapshot| {
        snapshot
            .players
            .iter()
            .any(|player| matches!(player.state, MediaPlayerState::Ready(_)))
    });
    let player = ready(&initial).id;
    let first = watch.adapter.start(MediaCommand::Play(player)).unwrap();
    let terminal = terminal_values
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    assert_eq!(terminal.id, first);
    assert_eq!(terminal.phase, MediaOperationPhase::Acknowledged);
    for _ in 0..64 {
        assert!(
            watch.adapter.start(MediaCommand::Next(player)).is_err(),
            "An undelivered terminal must not be replaced"
        );
    }
    assert_eq!(state.lock().unwrap().calls, vec![Call::Method("Play")]);
    release.send(()).unwrap();
    let published = watch.terminal(first);
    let refreshed = watch.refreshed_after(&published, player);
    assert_eq!(refreshed.operation, Some(terminal));
    let next = watch.acknowledge(MediaCommand::Next(player));
    assert!(next.operation.as_ref().unwrap().id.value() > first.value());
    assert_eq!(
        state.lock().unwrap().calls,
        vec![Call::Method("Play"), Call::Method("Next")]
    );
}

#[test]
#[ignore = "owned private MPRIS D-Bus fixture; no host session bus"]
fn media_protocol_pending_bus_loss_retains_exact_failure_across_owned_bus_recovery() {
    let mut bus = Bus::new();
    let (state, started) = fixture();
    state.lock().unwrap().delay = Duration::from_secs(2);
    let _app = application(&bus, "lost", state.clone());
    let watch = Watch::new(&bus, Duration::from_secs(3));
    let initial = watch.state(|snapshot| {
        snapshot
            .players
            .iter()
            .any(|player| matches!(player.state, MediaPlayerState::Ready(_)))
    });
    let old = ready(&initial).id;
    let command = MediaCommand::Pause(old);
    let operation = watch.adapter.start(command).unwrap();
    futures_lite::future::block_on(async {
        futures_lite::future::or(
            async {
                started.recv().await.unwrap();
            },
            async {
                async_io::Timer::after(Duration::from_secs(3)).await;
                panic!("Native pending method did not begin");
            },
        )
        .await;
    });
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
    let terminal = watch.adapter.operation().unwrap();
    assert_eq!(terminal.id, operation);
    assert_eq!(terminal.command, command);
    assert!(matches!(
        terminal.phase,
        MediaOperationPhase::Failed(DomainError::Unavailable(_))
    ));
    assert!(watch.adapter.start(MediaCommand::Next(old)).is_err());
    let mut adapter = watch.adapter.clone();
    drop(watch);
    let recovery_bus = Bus::new();
    Arc::get_mut(&mut adapter).unwrap().address = Some(recovery_bus.address.clone());
    let (replacement, _) = fixture();
    let _replacement_app = application(&recovery_bus, "recovered", replacement.clone());
    let recovered = Watch::from_adapter(adapter);
    let snapshot = recovered.state(|snapshot| {
        snapshot
            .players
            .iter()
            .any(|player| matches!(player.state, MediaPlayerState::Ready(_)))
    });
    assert_eq!(snapshot.operation, Some(terminal));
    let current = ready(&snapshot).id;
    assert!(current.value() > old.value());
    let stale = recovered.fail(MediaCommand::Next(old));
    assert!(stale.id.value() > operation.value());
    let next = recovered.acknowledge(MediaCommand::Next(current));
    assert!(next.operation.as_ref().unwrap().id.value() > stale.id.value());
    assert_eq!(
        replacement.lock().unwrap().calls,
        vec![Call::Method("Next")]
    );
    assert_eq!(state.lock().unwrap().calls, vec![Call::Method("Pause")]);
}

#[test]
#[ignore = "owned private MPRIS D-Bus fixture; no host session bus"]
fn media_protocol_next_during_post_ack_refresh_uses_current_owner_and_rejects_retired_seek() {
    let bus = Bus::new();
    let (state, _) = fixture();
    let app = application(&bus, "first", state.clone());
    let (other, _) = fixture();
    let _other_app = application(&bus, "second", other.clone());
    let (release, block) = async_channel::bounded(1);
    let gated_state = state.clone();
    let mut arm_refresh = true;
    let adapter = Arc::new(Mpris::with_bus_address(bus.address.clone()));
    let watch = Watch::with_hook(adapter, move |result| {
        if arm_refresh
            && let Ok(snapshot) = result
            && snapshot
                .operation
                .as_ref()
                .is_some_and(|operation| operation.phase == MediaOperationPhase::Acknowledged)
        {
            arm_refresh = false;
            // The follow-up read is spawned before this callback but has not
            // been polled. Hold only that read; command revalidation stays live.
            gated_state.lock().unwrap().read_block_once = Some(block.clone());
        }
    });
    let initial = watch.state(|snapshot| {
        snapshot.players.len() == 2
            && snapshot
                .players
                .iter()
                .all(|player| matches!(player.state, MediaPlayerState::Ready(_)))
    });
    let selected = &initial.players[0];
    let player = selected.id;
    let old_track = playback(selected).track.as_ref().unwrap().id;
    let notices = next_read(&state);
    let first = watch.adapter.start(MediaCommand::Play(player)).unwrap();
    let acknowledged = watch.terminal(first);
    assert_eq!(
        acknowledged.operation.as_ref().unwrap().phase,
        MediaOperationPhase::Acknowledged
    );
    // First GetAll validates Play; the second is the now-held post-ack read.
    read_started(&notices);
    read_started(&notices);
    assert!(state.lock().unwrap().read_block_once.is_none());
    // Deliberately avoid acknowledge()/refreshed_after(): a user can issue the
    // next action immediately after the prior terminal reaches their callback.
    let next = watch.admit(MediaCommand::Next(player));
    let completed = watch.terminal(next);
    assert_eq!(
        completed.operation.as_ref().unwrap().phase,
        MediaOperationPhase::Acknowledged,
        "A valid current owner must not become stale solely because its refresh is pending"
    );
    assert_eq!(
        state.lock().unwrap().calls,
        vec![Call::Method("Play"), Call::Method("Next")]
    );
    assert!(
        other.lock().unwrap().calls.is_empty(),
        "The action reached a different running player"
    );
    // Relaxing the redundant freshness flag must preserve exact track authority.
    state.lock().unwrap().track = "/fixture/b".into();
    changed(&app, &["Metadata"]);
    watch.state(|snapshot| {
        snapshot.players.iter().any(|value| value.id == player
        && matches!(&value.state, MediaPlayerState::Ready(playback) if playback.track.is_none()))
    });
    let stale = watch.fail(MediaCommand::SetPosition {
        player,
        track: old_track,
        position: MediaTime::default(),
    });
    assert!(matches!(
        stale.phase,
        MediaOperationPhase::Failed(DomainError::Invalid(_))
    ));
    assert_eq!(
        state.lock().unwrap().calls,
        vec![Call::Method("Play"), Call::Method("Next")]
    );
    release.send_blocking(()).unwrap();
    let refreshed = watch.state(|snapshot| {
        snapshot.players.iter().any(|value| {
            value.id == player
                && matches!(&value.state, MediaPlayerState::Ready(playback)
            if playback.track.as_ref().is_some_and(|track| track.id != old_track))
        })
    });
    assert_eq!(refreshed.operation, Some(stale));
}
