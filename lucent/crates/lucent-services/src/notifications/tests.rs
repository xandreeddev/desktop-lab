use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

fn target(server: &FreedesktopNotifications, id: u32) -> NotificationTarget {
    server
        .0
        .state
        .lock()
        .unwrap()
        .inbox
        .get(id)
        .and_then(Notification::target)
        .unwrap()
}
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
        Self::with_history(bus, paused, None)
    }
    fn with_history(
        bus: &Bus,
        paused: bool,
        history: Option<Arc<dyn NotificationHistoryPort>>,
    ) -> Self {
        let server = Arc::new(FreedesktopNotifications::with_history(history));
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
    fn until(&self, predicate: impl Fn(&NotificationSnapshot) -> bool) -> NotificationSnapshot {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let value = self
                .updates
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("Expected observed notification state")
                .expect("Notification service failed");
            if predicate(&value) {
                return value;
            }
        }
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
            let failed = worker.join().is_err();
            // Preserve an original test failure so Bus::drop can still reap
            // the private daemon instead of aborting on a second panic.
            assert!(
                !failed || std::thread::panicking(),
                "notification test worker failed"
            );
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
    assert_eq!(id, (SIGNAL_LIMIT + 1) as u32, "rejection consumed an ID");
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
    let retired = watch.snapshot();
    assert!(retired.active.is_empty());
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
        server.invoke(target(&server, ordinary), "open"),
        Err(DomainError::Unavailable(_))
    ));
    let (after, next_revision, count) = {
        let state = server.0.state.lock().unwrap();
        (state.inbox.snapshot(), state.revision, state.signals.len())
    };
    assert_eq!(after, before);
    assert_eq!(next_revision, revision);
    assert_eq!(count, SIGNAL_LIMIT - 1);
    server.invoke(target(&server, resident), "open").unwrap();
    let server_endpoint = Server(Arc::downgrade(&server.0));
    assert!(matches!(
        server_endpoint.close_notification(ordinary),
        Err(zbus::fdo::Error::LimitsExceeded(_))
    ));
    assert!(matches!(
        server.dismiss(target(&server, ordinary)),
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
        state.inbox.receive(note, 0).unwrap();
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
    let captured = target(&server, id);
    server.invoke(captured, "open").unwrap();
    drop(online);
    assert!(matches!(
        server.invoke(captured, "open"),
        Err(DomainError::Unavailable(_))
    ));
    let state = server.0.state.lock().unwrap();
    assert!(!state.online);
    assert!(state.signals.is_empty());
    assert!(
        state.inbox.get(id).is_none(),
        "transport-bound active identity must retire"
    );
    assert!(
        state
            .inbox
            .snapshot()
            .history
            .iter()
            .any(|note| note.id == id),
        "ordinary content stays in history across reconnects"
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
    let capabilities: Vec<String> = proxy.call("GetCapabilities", &()).unwrap();
    assert_eq!(
        capabilities,
        [
            "body",
            "body-markup",
            "body-hyperlinks",
            "body-images",
            "icon-static",
            "actions"
        ]
    );
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
    assert!(server.invoke(target(server, id), "invented").is_err());
    server.invoke(target(server, id), "default").unwrap();
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

#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_history_survives_server_restart_and_retires_transport_actions() {
    let bus = Bus::new();
    let path = bus.dir.join("notifications.json");
    let history: Arc<dyn NotificationHistoryPort> =
        Arc::new(JsonNotificationHistory { path: path.clone() });
    let watch = Watcher::with_history(&bus, false, Some(history.clone()));
    assert_eq!(
        watch.snapshot().history_status,
        NotificationHistoryStatus::Ready
    );
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    let id = notify(&proxy, 0).unwrap();
    let captured = target(&watch.server, id);
    watch.server.set_do_not_disturb(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while history.load().unwrap().entries.is_empty() || !history.load().unwrap().do_not_disturb {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(proxy);
    drop(connection);
    drop(watch);
    let restored = Watcher::with_history(&bus, false, Some(history));
    let state = restored.snapshot();
    assert!(state.active.is_empty());
    assert_eq!(state.history.len(), 1);
    assert!(state.do_not_disturb);
    assert!(restored.server.invoke(captured, "default").is_err());
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    let fresh = notify(&proxy, 0).unwrap();
    assert_ne!(target(&restored.server, fresh), captured);
    assert!(restored.server.invoke(captured, "default").is_err());
    restored.server.clear_history().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let loaded = JsonNotificationHistory { path: path.clone() }
            .load()
            .unwrap();
        if loaded
            .entries
            .iter()
            .all(|n| n.revision != captured.revision())
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_bad_history_stays_preserved_while_live_delivery_and_explicit_recovery_work() {
    let bus = Bus::new();
    let path = bus.dir.join("notifications.json");
    let original = br#"{"private-event":"keep this unreadable file"}"#;
    std::fs::write(&path, original).unwrap();
    let watch = Watcher::with_history(
        &bus,
        false,
        Some(Arc::new(JsonNotificationHistory { path: path.clone() })),
    );
    assert_eq!(
        watch.snapshot().history_status,
        NotificationHistoryStatus::ReadFailed
    );
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    let id = notify(&proxy, 0).unwrap();
    assert!(watch.snapshot().active.iter().any(|n| n.id == id));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    watch.server.retry_history().unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    watch.server.clear_history().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while (JsonNotificationHistory { path: path.clone() })
        .load()
        .is_err()
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(watch.server.0.state.lock().unwrap().inbox.get(id).is_some());
}

#[test]
fn replacement_rejects_old_actions_without_consuming_new_notification_or_signal_admission() {
    let server = FreedesktopNotifications::default();
    let _online = Online::new(server.0.clone());
    let id = seed(&server, true);
    let old = target(&server, id);
    Server(Arc::downgrade(&server.0))
        .notify(
            "Fixture",
            id,
            "",
            "Updated",
            "",
            vec!["open".into(), "Open".into()],
            HashMap::from([("resident".into(), OwnedValue::from(true))]),
            0,
        )
        .unwrap();
    let current = target(&server, id);
    assert_ne!(old, current);
    let before = server.0.state.lock().unwrap().revision;
    assert!(server.invoke(old, "open").is_err());
    assert!(server.dismiss(old).is_err());
    assert_eq!(server.0.state.lock().unwrap().revision, before);
    assert!(server.0.state.lock().unwrap().signals.is_empty());
    server.invoke(current, "open").unwrap();
    assert!(server.0.state.lock().unwrap().inbox.get(id).is_some());
}

#[derive(Default)]
struct HistoryAdapter {
    archive: Mutex<NotificationArchive>,
    fail: AtomicBool,
    gate: Mutex<(bool, bool)>,
    wake: Condvar,
}
impl NotificationHistoryPort for HistoryAdapter {
    fn load(&self) -> Result<NotificationArchive> {
        Ok(self.archive.lock().unwrap().clone())
    }
    fn save(&self, archive: &NotificationArchive) -> Result<()> {
        let mut gate = self.gate.lock().unwrap();
        gate.1 = true;
        self.wake.notify_all();
        while gate.0 {
            gate = self.wake.wait(gate).unwrap();
        }
        drop(gate);
        if self.fail.load(Ordering::Relaxed) {
            return Err(DomainError::Failed("Fixture storage failure".into()));
        }
        *self.archive.lock().unwrap() = archive.clone();
        Ok(())
    }
}
#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_history_save_failure_and_retry_do_not_remove_live_content() {
    let bus = Bus::new();
    let history = Arc::new(HistoryAdapter::default());
    let watch = Watcher::with_history(&bus, false, Some(history.clone()));
    watch.snapshot();
    history.fail.store(true, Ordering::Relaxed);
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    let id = notify(&proxy, 0).unwrap();
    let snapshot = watch.until(|s| s.history_status == NotificationHistoryStatus::WriteFailed);
    assert_eq!(
        snapshot.history_status,
        NotificationHistoryStatus::WriteFailed
    );
    assert!(snapshot.active.iter().any(|n| n.id == id));
    assert!(history.archive.lock().unwrap().entries.is_empty());
    history.fail.store(false, Ordering::Relaxed);
    watch.server.retry_history().unwrap();
    let snapshot = watch.until(|s| s.history_status == NotificationHistoryStatus::Ready);
    assert_eq!(snapshot.history_status, NotificationHistoryStatus::Ready);
    assert!(snapshot.active.iter().any(|n| n.id == id));
    assert!(
        history
            .archive
            .lock()
            .unwrap()
            .entries
            .iter()
            .any(|n| n.id == id)
    );
}
#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_history_writer_never_holds_native_inbox_admission_mutex() {
    let bus = Bus::new();
    let history = Arc::new(HistoryAdapter::default());
    let watch = Watcher::with_history(&bus, false, Some(history.clone()));
    watch.snapshot();
    *history.gate.lock().unwrap() = (true, false);
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    notify(&proxy, 0).unwrap();
    let held = history.gate.lock().unwrap();
    let (held, wait) = history
        .wake
        .wait_timeout_while(held, Duration::from_secs(2), |held| !held.1)
        .unwrap();
    let entered = held.1;
    drop(held);
    // Release on every path before asserting, so the owned observer can join.
    let result = if entered && !wait.timed_out() {
        notify(&proxy, 0)
    } else {
        Err(zbus::Error::Failure("Writer did not enter".into()))
    };
    history.gate.lock().unwrap().0 = false;
    history.wake.notify_all();
    assert!(entered);
    assert!(result.is_ok());
}

struct SaveGate(Arc<HistoryAdapter>);
impl Drop for SaveGate {
    fn drop(&mut self) {
        self.0.gate.lock().unwrap().0 = false;
        self.0.wake.notify_all();
    }
}
#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_stalled_storage_keeps_snapshots_actions_and_expiration_live() {
    let bus = Bus::new();
    let history = Arc::new(HistoryAdapter::default());
    let watch = Watcher::with_history(&bus, false, Some(history.clone()));
    watch.snapshot();
    let gate = SaveGate(history.clone());
    *history.gate.lock().unwrap() = (true, false);
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    let mut actions = proxy.receive_signal("ActionInvoked").unwrap();
    let id = notify(&proxy, 0).unwrap();
    let held = history.gate.lock().unwrap();
    let (held, wait) = history
        .wake
        .wait_timeout_while(held, Duration::from_secs(2), |g| !g.1)
        .unwrap();
    assert!(held.1 && !wait.timed_out());
    drop(held);
    let snapshot = watch.until(|s| s.active.iter().any(|n| n.id == id));
    assert_eq!(snapshot.history_status, NotificationHistoryStatus::Saving);
    let captured = target(&watch.server, id);
    watch.server.invoke(captured, "default").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let message = actions.next().unwrap();
        tx.send(message.body().deserialize::<(u32, String)>().unwrap())
            .unwrap();
    });
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        (id, "default".into())
    );
    let expiring: u32 = proxy
        .call(
            "Notify",
            &(
                "Fixture",
                0_u32,
                "",
                "Timed fixture",
                "",
                Vec::<String>::new(),
                HashMap::<String, OwnedValue>::new(),
                30_i32,
            ),
        )
        .unwrap();
    let snapshot = watch.until(|s| s.history.iter().any(|n| n.id == expiring));
    assert!(!snapshot.active.iter().any(|n| n.id == expiring));
    assert_eq!(snapshot.history_status, NotificationHistoryStatus::Saving);
    assert!(history.archive.lock().unwrap().entries.is_empty());
    drop(gate);
    watch.until(|s| s.history_status == NotificationHistoryStatus::Ready);
    assert!(
        history
            .archive
            .lock()
            .unwrap()
            .entries
            .iter()
            .any(|n| n.id == expiring)
    );
}

#[test]
fn superseded_io_completions_cannot_acknowledge_newer_save_or_resurrect_cleared_history() {
    let server = FreedesktopNotifications::default();
    {
        let mut state = server.0.state.lock().unwrap();
        state.save_pending = Some(2);
        state.history_status = NotificationHistoryStatus::Saving;
    }
    server
        .0
        .storage_complete(storage::Completion::Saved(1, Ok(())));
    assert_eq!(
        server.0.state.lock().unwrap().history_status,
        NotificationHistoryStatus::Saving
    );
    assert_eq!(server.0.state.lock().unwrap().save_pending, Some(2));
    server
        .0
        .storage_complete(storage::Completion::Saved(2, Ok(())));
    assert_eq!(
        server.0.state.lock().unwrap().history_status,
        NotificationHistoryStatus::Ready
    );
    {
        let mut state = server.0.state.lock().unwrap();
        state.load_pending = Some(3);
        state.history_status = NotificationHistoryStatus::Loading;
    }
    server.clear_history().unwrap();
    server.0.storage_complete(storage::Completion::Loaded(
        3,
        Err(DomainError::Invalid("stale fixture".into())),
    ));
    assert!(server.0.state.lock().unwrap().writable);
    assert!(server.0.state.lock().unwrap().loaded);
    assert_ne!(
        server.0.state.lock().unwrap().history_status,
        NotificationHistoryStatus::ReadFailed
    );
}

#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_stalled_storage_does_not_delay_disconnect_reconnect_or_name_release() {
    let mut bus = Bus::new();
    let history = Arc::new(HistoryAdapter::default());
    let watch = Watcher::with_history(&bus, false, Some(history.clone()));
    watch.snapshot();
    let gate = SaveGate(history.clone());
    *history.gate.lock().unwrap() = (true, false);
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    let id = notify(&proxy, 0).unwrap();
    watch.until(|s| s.active.iter().any(|n| n.id == id));
    let old = target(&watch.server, id);
    bus.stop();
    watch.until(|s| s.active.is_empty() && s.history.iter().any(|n| n.id == id));
    assert!(
        watch
            .updates
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_err()
    );
    bus.start();
    watch.recovered();
    assert!(watch.server.invoke(old, "default").is_err());
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    let fresh = notify(&proxy, 0).unwrap();
    watch.until(|s| s.active.iter().any(|n| n.id == fresh));
    let server = watch.server.clone();
    let started = Instant::now();
    drop(watch);
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(!server.0.state.lock().unwrap().online);
    let claimant = bus.connection();
    claimant.request_name(NAME).unwrap();
    drop(gate);
}

struct LoadAdapter {
    fail: AtomicBool,
    gate: Mutex<(bool, bool)>,
    wake: Condvar,
    archive: Mutex<NotificationArchive>,
}
impl NotificationHistoryPort for LoadAdapter {
    fn load(&self) -> Result<NotificationArchive> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(DomainError::Invalid("Fixture load failed".into()));
        }
        let mut gate = self.gate.lock().unwrap();
        gate.1 = true;
        self.wake.notify_all();
        while gate.0 {
            gate = self.wake.wait(gate).unwrap();
        }
        drop(gate);
        Ok(self.archive.lock().unwrap().clone())
    }
    fn save(&self, archive: &NotificationArchive) -> Result<()> {
        *self.archive.lock().unwrap() = archive.clone();
        Ok(())
    }
}
struct LoadGate(Arc<LoadAdapter>);
impl Drop for LoadGate {
    fn drop(&mut self) {
        self.0.gate.lock().unwrap().0 = false;
        self.0.wake.notify_all();
    }
}
#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_stalled_recovery_load_keeps_delivery_live_and_clear_retires_its_result() {
    let bus = Bus::new();
    let history = Arc::new(LoadAdapter {
        fail: AtomicBool::new(true),
        gate: Mutex::new((false, false)),
        wake: Condvar::new(),
        archive: Mutex::new(NotificationArchive::default()),
    });
    let gate = LoadGate(history.clone());
    let watch = Watcher::with_history(&bus, false, Some(history.clone()));
    assert_eq!(
        watch.snapshot().history_status,
        NotificationHistoryStatus::ReadFailed
    );
    *history.archive.lock().unwrap() = NotificationArchive {
        entries: vec![Notification {
            id: 20,
            revision: 100,
            received_unix_ms: 100,
            desktop_entry: None,
            app: "Old fixture".into(),
            summary: "Previous archive".into(),
            body: String::new(),
            image: None,
            actions: vec![],
            critical: false,
            resident: false,
            transient: false,
            timeout_ms: 0,
        }],
        last_revision: 100,
        do_not_disturb: true,
    };
    history.fail.store(false, Ordering::Relaxed);
    *history.gate.lock().unwrap() = (true, false);
    watch.server.retry_history().unwrap();
    let held = history.gate.lock().unwrap();
    let (held, wait) = history
        .wake
        .wait_timeout_while(held, Duration::from_secs(2), |g| !g.1)
        .unwrap();
    assert!(held.1 && !wait.timed_out());
    drop(held);
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    let id = notify(&proxy, 0).unwrap();
    let snapshot = watch.until(|s| s.active.iter().any(|n| n.id == id));
    assert_eq!(snapshot.history_status, NotificationHistoryStatus::Loading);
    let captured = target(&watch.server, id);
    watch.server.clear_history().unwrap();
    drop(gate);
    let snapshot = watch.until(|s| s.history_status == NotificationHistoryStatus::Ready);
    assert!(snapshot.history.is_empty());
    assert!(snapshot.active.iter().any(|n| n.target() == Some(captured)));
    assert!(!snapshot.do_not_disturb);
    assert!(
        history
            .archive
            .lock()
            .unwrap()
            .entries
            .iter()
            .all(|n| n.id != 20)
    );
}

#[test]
#[ignore = "requires a test-owned private dbus-daemon"]
fn notification_wire_images_reach_current_snapshots_and_fallback_obeys_priority() {
    let bus = Bus::new();
    let watch = Watcher::new(&bus, false);
    watch.snapshot();
    let connection = bus.connection();
    let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
    let mut hints = HashMap::<String, OwnedValue>::new();
    hints.insert(
        "image-data".into(),
        OwnedValue::try_from(zbus::zvariant::Value::from((
            2i32,
            1i32,
            8i32,
            true,
            8i32,
            4i32,
            vec![10u8, 20, 30, 128, 40, 50, 60, 255],
        )))
        .unwrap(),
    );
    hints.insert(
        "image-path".into(),
        OwnedValue::from(zbus::zvariant::Str::from("/tmp/fixture.png")),
    );
    let id: u32 = proxy
        .call(
            "Notify",
            &(
                "Fixture",
                0u32,
                "mail-unread",
                "Image event",
                "<b>Body</b>",
                Vec::<String>::new(),
                hints,
                0i32,
            ),
        )
        .unwrap();
    let snapshot = watch.snapshot();
    let note = snapshot.active.iter().find(|note| note.id == id).unwrap();
    assert!(
        matches!(&note.image,Some(NotificationImage::Pixels(image)) if image.rgba()==[10,20,30,128,40,50,60,255])
    );
    assert_eq!(note.body, "<b>Body</b>");
    let mut hints = HashMap::<String, OwnedValue>::new();
    hints.insert(
        "image-data".into(),
        OwnedValue::try_from(zbus::zvariant::Value::from((
            -1i32,
            1i32,
            4i32,
            true,
            8i32,
            4i32,
            vec![0u8; 4],
        )))
        .unwrap(),
    );
    hints.insert(
        "image-path".into(),
        OwnedValue::from(zbus::zvariant::Str::from("/tmp/fixture.png")),
    );
    let replacement: u32 = proxy
        .call(
            "Notify",
            &(
                "Fixture",
                id,
                "mail-unread",
                "Replacement",
                "Body",
                Vec::<String>::new(),
                hints,
                0i32,
            ),
        )
        .unwrap();
    assert_eq!(replacement, id);
    let snapshot = watch.snapshot();
    assert!(
        matches!(&snapshot.active.iter().find(|note|note.id==id).unwrap().image,Some(NotificationImage::Source(source)) if source.value()=="/tmp/fixture.png")
    );
}
