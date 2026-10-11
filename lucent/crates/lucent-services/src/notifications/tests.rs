use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// Owns only a private daemon. Restart preserves its address, not its bus GUID.
struct Bus {
    dir: std::path::PathBuf,
    address: String,
    child: Option<std::process::Child>,
}
impl Bus {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "lucent-notification-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        let mut bus = Self {
            address: format!("unix:path={}", dir.join("bus").display()),
            dir,
            child: None,
        };
        bus.start();
        bus
    }
    fn start(&mut self) {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        assert!(self.child.is_none());
        self.child = Some(
            Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
                .arg(format!("--address={}", self.address))
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("private dbus-daemon"),
        );
        let stdout = self.child.as_mut().unwrap().stdout.take().unwrap();
        let mut address = String::new();
        std::io::BufReader::new(stdout)
            .read_line(&mut address)
            .unwrap();
        assert!(
            address.starts_with(&self.address),
            "private bus did not start"
        );
    }
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            child.wait().unwrap();
        }
    }
    fn connection(&self) -> zbus::blocking::Connection {
        Builder::address(self.address.as_str())
            .unwrap()
            .method_timeout(Duration::from_secs(3))
            .build()
            .unwrap()
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        self.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Pausing a subscriber must not hold the inbox lock or hide transport overload.
struct Watcher {
    server: Arc<FreedesktopNotifications>,
    stop: Arc<Stop>,
    paused: Arc<(Mutex<bool>, Condvar)>,
    updates: std::sync::mpsc::Receiver<Result<NotificationSnapshot>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Watcher {
    fn new(bus: &Bus, paused: bool) -> Self {
        let server = Arc::new(FreedesktopNotifications::default());
        let stop = Arc::new(Stop(AtomicBool::new(false)));
        let paused = Arc::new((Mutex::new(paused), Condvar::new()));
        let (send, updates) = std::sync::mpsc::channel();
        let worker = {
            let (server, stop, paused, address) = (
                server.clone(),
                stop.clone(),
                paused.clone(),
                bus.address.clone(),
            );
            std::thread::spawn(move || {
                server.watch_with(
                    &mut |value| {
                        let _ = send.send(value);
                        let (lock, wake) = &*paused;
                        let mut hold = lock.lock().unwrap();
                        while *hold && !stop.cancelled() {
                            hold = wake
                                .wait_timeout(hold, Duration::from_millis(20))
                                .unwrap()
                                .0;
                        }
                    },
                    stop.as_ref(),
                    || Builder::address(address.as_str()),
                );
            })
        };
        Self {
            server,
            stop,
            paused,
            updates,
            worker: Some(worker),
        }
    }
    fn resume(&self) {
        *self.paused.0.lock().unwrap() = false;
        self.paused.1.notify_all();
    }
    fn snapshot(&self) -> NotificationSnapshot {
        self.updates
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap()
    }
    fn recovered(&self) -> NotificationSnapshot {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let value = self
                .updates
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("notification name was not reacquired");
            if let Ok(snapshot) = value {
                return snapshot;
            }
        }
    }
}
impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.0.store(true, Ordering::Relaxed);
        self.resume();
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

fn notify(proxy: &zbus::blocking::Proxy<'_>, replaces: u32) -> zbus::Result<u32> {
    proxy.call(
        "Notify",
        &(
            "Fixture",
            replaces,
            "",
            "A protocol fixture",
            "Body",
            vec!["default", "Open"],
            HashMap::<String, OwnedValue>::new(),
            0_i32,
        ),
    )
}

#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_backpressure_preserves_state_and_recovers_after_stall() {
    let bus = Bus::new();
    let watch = Watcher::new(&bus, true);
    assert!(watch.snapshot().active.is_empty());
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    for _ in 0..SIGNAL_LIMIT {
        let id = notify(&proxy, 0).unwrap();
        proxy.call::<_, _, ()>("CloseNotification", &(id,)).unwrap();
    }
    let previous = watch.server.0.state.lock().unwrap().inbox.snapshot();
    let error = notify(&proxy, 0).unwrap_err();
    assert!(
        matches!(error, zbus::Error::MethodError(ref name, _, _)
        if name.as_str() == "org.freedesktop.DBus.Error.LimitsExceeded"),
        "{error}"
    );
    assert_eq!(
        watch.server.0.state.lock().unwrap().signals.len(),
        SIGNAL_LIMIT
    );
    assert_eq!(
        watch.server.0.state.lock().unwrap().inbox.snapshot(),
        previous
    );
    watch.resume();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !watch.server.0.state.lock().unwrap().signals.is_empty() {
        assert!(Instant::now() < deadline, "signal delivery did not resume");
        std::thread::sleep(Duration::from_millis(10));
    }
    let id = notify(&proxy, 0).unwrap();
    loop {
        assert!(Instant::now() < deadline);
        if watch.snapshot().active.iter().any(|n| n.id == id) {
            break;
        }
    }
}

#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_idle_disconnect_reports_failure_and_reconnects() {
    let mut bus = Bus::new();
    let watch = Watcher::new(&bus, false);
    assert!(watch.snapshot().active.is_empty());
    assert!(watch.server.0.state.lock().unwrap().signals.is_empty());
    bus.stop();
    let error = watch
        .updates
        .recv_timeout(Duration::from_secs(3))
        .expect("idle disconnection was not observed")
        .unwrap_err();
    assert!(matches!(error, DomainError::Unavailable(_)));
    bus.start();
    // Failed reconnects can race with daemon startup; eventually require a snapshot.
    watch.recovered();
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    let id = notify(&proxy, 0).unwrap();
    assert!(watch.snapshot().active.iter().any(|n| n.id == id));
}

#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_name_ownership_is_exclusive_without_displacing_another_server() {
    let bus = Bus::new();
    let incumbent = bus.connection();
    incumbent.request_name(NAME).unwrap();
    let watch = Watcher::new(&bus, false);
    let result = watch.updates.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(
        matches!(result, Err(DomainError::Unavailable(_))),
        "adapter displaced an existing notification server"
    );
    incumbent.release_name(NAME).unwrap();
    watch.recovered();
    let competitor = bus.connection();
    assert!(
        competitor.request_name(NAME).is_err(),
        "active name could be displaced"
    );
    let proxy = zbus::blocking::Proxy::new(&competitor, NAME, PATH, NAME).unwrap();
    let id = notify(&proxy, 0).unwrap();
    assert!(watch.snapshot().active.iter().any(|n| n.id == id));
    drop(watch);
    competitor.request_name(NAME).unwrap();
}
struct Stop(AtomicBool);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    fn wait(&self, d: Duration) {
        let deadline = Instant::now() + d;
        while !self.cancelled() && Instant::now() < deadline {
            std::thread::sleep(
                Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
}

fn seed(server: &FreedesktopNotifications, resident: bool) -> u32 {
    let hints = HashMap::from([("resident".into(), OwnedValue::from(resident))]);
    Server(Arc::downgrade(&server.0))
        .notify(
            "Fixture",
            0,
            "",
            "Fixture",
            "",
            vec!["open".into(), "Open".into()],
            hints,
            0,
        )
        .unwrap()
}

#[test]
fn signal_admission_is_atomic_for_close_and_two_signal_actions() {
    let server = FreedesktopNotifications::default();
    let _online = Online::new(server.0.clone());
    let ordinary = seed(&server, false);
    let resident = seed(&server, true);
    let (before, revision) = {
        let mut state = server.0.state.lock().unwrap();
        state
            .signals
            .extend((0..SIGNAL_LIMIT - 1).map(|_| Signal::Closed(0, CloseReason::Undefined)));
        (state.inbox.snapshot(), state.revision)
    };
    assert!(matches!(
        server.invoke(ordinary, "open"),
        Err(DomainError::Unavailable(_))
    ));
    let (after, next_revision, count) = {
        let state = server.0.state.lock().unwrap();
        (state.inbox.snapshot(), state.revision, state.signals.len())
    };
    assert_eq!(after, before);
    assert_eq!(next_revision, revision);
    assert_eq!(count, SIGNAL_LIMIT - 1);
    server.invoke(resident, "open").unwrap();
    let server_endpoint = Server(Arc::downgrade(&server.0));
    assert!(matches!(
        server_endpoint.close_notification(ordinary),
        Err(zbus::fdo::Error::LimitsExceeded(_))
    ));
    assert!(matches!(
        server.dismiss(ordinary),
        Err(DomainError::Unavailable(_))
    ));
    let after = server.0.state.lock().unwrap().inbox.snapshot();
    assert_eq!(after, before);
    server.0.state.lock().unwrap().signals.pop_front();
    server_endpoint.close_notification(ordinary).unwrap();
    let state = server.0.state.lock().unwrap();
    assert!(state.inbox.get(ordinary).is_none());
    assert!(state.inbox.get(resident).is_some());
    assert_eq!(state.signals.len(), SIGNAL_LIMIT);
    assert!(
        matches!(state.signals.back(), Some(Signal::Closed(id, CloseReason::Requested)) if *id == ordinary)
    );
}

#[test]
fn expiration_batches_wait_for_signal_capacity_without_losing_notifications() {
    let server = FreedesktopNotifications::default();
    let _online = Online::new(server.0.clone());
    for _ in 0..3 {
        seed(&server, false);
    }
    let mut state = server.0.state.lock().unwrap();
    for mut note in state.inbox.snapshot().active {
        note.timeout_ms = 1;
        state.inbox.receive(note, 0);
    }
    state
        .signals
        .extend((0..SIGNAL_LIMIT - 1).map(|_| Signal::Closed(0, CloseReason::Undefined)));
    state.expire(10);
    assert_eq!(state.signals.len(), SIGNAL_LIMIT);
    assert_eq!(state.inbox.snapshot().active.len(), 2);
    state.expire(20);
    assert_eq!(state.inbox.snapshot().active.len(), 2);
    state.signals.clear();
    state.expire(30);
    assert_eq!(state.signals.len(), 2);
    assert!(state.inbox.snapshot().active.is_empty());
    assert_eq!(state.inbox.snapshot().history.len(), 3);
    assert!(
        state
            .signals
            .iter()
            .all(|s| matches!(s, Signal::Closed(_, CloseReason::Expired)))
    );
}

#[test]
fn connection_lifetime_discards_pending_signals_and_rejects_offline_actions() {
    let server = FreedesktopNotifications::default();
    let online = Online::new(server.0.clone());
    let id = seed(&server, true);
    server.invoke(id, "open").unwrap();
    drop(online);
    assert!(matches!(
        server.invoke(id, "open"),
        Err(DomainError::Unavailable(_))
    ));
    let state = server.0.state.lock().unwrap();
    assert!(!state.online);
    assert!(state.signals.is_empty());
    assert!(
        state.inbox.get(id).is_some(),
        "in-memory inbox stays available across reconnects"
    );
}
#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_protocol() {
    let bus = Bus::new();
    let watch = Watcher::new(&bus, false);
    watch.snapshot();
    let server = &watch.server;
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    let info: (String, String, String, String) = proxy.call("GetServerInformation", &()).unwrap();
    assert_eq!(info.0, "Lucent");
    let mut actions = proxy.receive_signal("ActionInvoked").unwrap();
    let notify = |replaces: u32, timeout: i32| -> u32 {
        proxy
            .call(
                "Notify",
                &(
                    "Fixture",
                    replaces,
                    "",
                    "A real protocol message",
                    "Body",
                    vec!["default", "Open"],
                    HashMap::<String, OwnedValue>::new(),
                    timeout,
                ),
            )
            .unwrap()
    };
    let id = notify(0, 0);
    assert_eq!(notify(id, 0), id);
    assert!(server.invoke(id, "invented").is_err());
    server.invoke(id, "default").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let message = actions.next().unwrap();
        let data: (u32, String) = message.body().deserialize().unwrap();
        tx.send(data).unwrap();
    });
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        (id, "default".into())
    );
    let expires = notify(0, 30);
    std::thread::sleep(Duration::from_millis(250));
    let state = server.0.state.lock().unwrap().inbox.snapshot();
    assert!(!state.active.iter().any(|n| n.id == expires));
    assert!(state.history.iter().any(|n| n.id == expires));
}
