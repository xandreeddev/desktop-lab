use super::native::*;
#[test]
fn tray_pixels_validate_dimensions_and_convert_network_argb() {
    use zbus::zvariant::Value;
    let mut properties = Properties::new();
    properties.insert(
        "IconPixmap".into(),
        Value::from(vec![(1i32, 1i32, vec![128u8, 20, 30, 40])])
            .try_to_owned()
            .unwrap(),
    );
    let decoded = icon(&properties, "Icon").pixmap.unwrap();
    assert_eq!(decoded.rgba(), &[20, 30, 40, 128]);
    assert!(lucent_domain::TrayPixmap::new(0, 1, vec![]).is_err());
    assert!(lucent_domain::TrayPixmap::new(257, 1, vec![0; 257 * 4]).is_err());
}
use super::*;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command as Process, Stdio},
    sync::Arc,
    thread,
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
struct Bus {
    child: Child,
    address: String,
}
impl Bus {
    fn new() -> Self {
        let mut child = Process::new("dbus-daemon")
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
    fn wait(&self, d: Duration) {
        let end = Instant::now() + d;
        while !self.cancelled() && Instant::now() < end {
            thread::sleep(Duration::from_millis(10));
        }
    }
}
struct Watch {
    adapter: Arc<StatusNotifier>,
    stop: Arc<Stop>,
    snapshots: mpsc::Receiver<Result<TraySnapshot>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Watch {
    fn new(bus: &Bus) -> Self {
        let adapter = Arc::new(StatusNotifier::with_bus_address(bus.address.clone()));
        let stop = Arc::new(Stop(AtomicBool::new(false)));
        let (send, snapshots) = mpsc::channel();
        let (a, s) = (adapter.clone(), stop.clone());
        let worker = thread::spawn(move || {
            a.watch(
                &mut |v| {
                    let _ = send.send(v);
                },
                s.as_ref(),
            )
        });
        Self {
            adapter,
            stop,
            snapshots,
            worker: Some(worker),
        }
    }
    fn state(&self, p: impl Fn(&TraySnapshot) -> bool) -> TraySnapshot {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            match self
                .snapshots
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap()
            {
                Ok(v) if p(&v) => return v,
                Err(e) => panic!("{e}"),
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
#[derive(Default)]
struct State {
    actions: Vec<String>,
    revision: u32,
    disabled: bool,
    slow_status: bool,
}
struct Application(Arc<Mutex<State>>);
#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Application {
    #[zbus(property)]
    fn id(&self) -> &str {
        "fixture"
    }
    #[zbus(property)]
    fn title(&self) -> &str {
        "Fixture app"
    }
    #[zbus(property)]
    async fn status(&self) -> &str {
        let slow = self.0.lock().unwrap().slow_status;
        if slow {
            async_io::Timer::after(Duration::from_secs(3)).await;
        }
        "Active"
    }
    #[zbus(property)]
    fn icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        vec![(1, 1, vec![128, 20, 30, 40])]
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from("/Menu").unwrap()
    }
    fn activate(&self, x: i32, y: i32) {
        self.0
            .lock()
            .unwrap()
            .actions
            .push(format!("activate:{x}:{y}"));
    }
    fn secondary_activate(&self, x: i32, y: i32) {
        self.0
            .lock()
            .unwrap()
            .actions
            .push(format!("secondary:{x}:{y}"));
    }
    fn scroll(&self, delta: i32, orientation: &str) {
        self.0
            .lock()
            .unwrap()
            .actions
            .push(format!("scroll:{delta}:{orientation}"));
    }
    fn context_menu(&self, x: i32, y: i32) {
        self.0
            .lock()
            .unwrap()
            .actions
            .push(format!("context:{x}:{y}"));
    }
}
type FixtureLayout = (
    i32,
    std::collections::HashMap<String, OwnedValue>,
    Vec<OwnedValue>,
);
struct Menu(Arc<Mutex<State>>);
#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    fn about_to_show(&self, _id: i32) -> bool {
        false
    }
    fn get_layout(
        &self,
        _parent_id: i32,
        _recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> (u32, FixtureLayout) {
        let state = self.0.lock().unwrap();
        let mut p: Properties = std::collections::HashMap::new();
        p.insert("label".into(), Value::from("_Open").try_to_owned().unwrap());
        p.insert(
            "enabled".into(),
            Value::from(!state.disabled).try_to_owned().unwrap(),
        );
        let child = Value::from((1i32, p, Vec::<OwnedValue>::new()))
            .try_to_owned()
            .unwrap();
        (
            state.revision,
            (0, std::collections::HashMap::new(), vec![child]),
        )
    }
    fn event(&self, id: i32, event_id: &str, _data: Value<'_>, _timestamp: u32) {
        self.0
            .lock()
            .unwrap()
            .actions
            .push(format!("event:{id}:{event_id}"));
    }
}
async fn application(bus: &Bus, state: Arc<Mutex<State>>) -> zbus::Connection {
    zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.fixture.TrayApp")
        .unwrap()
        .serve_at("/StatusNotifierItem", Application(state.clone()))
        .unwrap()
        .serve_at("/Menu", Menu(state))
        .unwrap()
        .build()
        .await
        .unwrap()
}
async fn register(connection: &zbus::Connection, service: &str) -> zbus::Result<()> {
    connection
        .call_method(
            Some("org.kde.StatusNotifierWatcher"),
            "/StatusNotifierWatcher",
            Some("org.kde.StatusNotifierWatcher"),
            "RegisterStatusNotifierItem",
            &(service,),
        )
        .await?
        .body()
        .deserialize()
}
#[test]
#[ignore = "owned isolated tray D-Bus fixture, no host session bus"]
fn tray_protocol_registration_pixels_actions_and_native_menu() {
    let bus = Bus::new();
    let watch = Watch::new(&bus);
    watch.state(|s| s.items.is_empty());
    let state = Arc::new(Mutex::new(State {
        revision: 3,
        ..Default::default()
    }));
    let app = futures_lite::future::block_on(application(&bus, state.clone()));
    futures_lite::future::block_on(register(&app, "/StatusNotifierItem")).unwrap();
    let snapshot = watch.state(|s| s.items.len() == 1);
    let id = snapshot.items[0].id;
    assert_eq!(
        snapshot.items[0].icon.pixmap.as_ref().unwrap().rgba(),
        &[20, 30, 40, 128]
    );
    watch
        .adapter
        .act(id, TrayAction::Activate { x: 4, y: 8 })
        .unwrap();
    watch
        .adapter
        .act(id, TrayAction::SecondaryActivate { x: 4, y: 8 })
        .unwrap();
    watch
        .adapter
        .act(
            id,
            TrayAction::Scroll {
                delta: 120,
                horizontal: false,
            },
        )
        .unwrap();
    let menu = watch.adapter.menu(id, 0).unwrap();
    assert_eq!(menu.revision, 3);
    assert_eq!(menu.root.children[0].label, "_Open");
    watch.adapter.invoke(id, 3, 1, 0).unwrap();
    assert_eq!(
        state.lock().unwrap().actions,
        vec![
            "activate:4:8",
            "secondary:4:8",
            "scroll:120:vertical",
            "event:1:clicked"
        ]
    );
    futures_lite::future::block_on(app.emit_signal(
        None::<&str>,
        "/Menu",
        "com.canonical.dbusmenu",
        "LayoutUpdated",
        &(4u32, 0i32),
    ))
    .unwrap();
    watch.state(|s| s.items.first().is_some_and(|i| i.menu_generation > 0));
}
#[test]
#[ignore = "owned isolated tray D-Bus fixture, no host session bus"]
fn tray_protocol_sender_authority_and_owner_retirement() {
    let bus = Bus::new();
    let watch = Watch::new(&bus);
    watch.state(|s| s.items.is_empty());
    let state = Arc::new(Mutex::new(State::default()));
    let app = futures_lite::future::block_on(application(&bus, state.clone()));
    let stranger = futures_lite::future::block_on(
        zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .build(),
    )
    .unwrap();
    assert!(futures_lite::future::block_on(register(&stranger, "org.fixture.TrayApp")).is_err());
    futures_lite::future::block_on(register(&app, "org.fixture.TrayApp")).unwrap();
    let old = watch.state(|s| s.items.len() == 1).items[0].id;
    futures_lite::future::block_on(app.close()).unwrap();
    watch.state(|s| s.items.is_empty());
    let replacement = futures_lite::future::block_on(application(&bus, state.clone()));
    futures_lite::future::block_on(register(&replacement, "org.fixture.TrayApp")).unwrap();
    let new = watch.state(|s| s.items.len() == 1).items[0].id;
    assert_ne!(old, new);
    assert!(matches!(
        watch.adapter.act(old, TrayAction::Activate { x: 0, y: 0 }),
        Err(DomainError::Invalid(_))
    ));
    assert!(state.lock().unwrap().actions.is_empty());
}
#[test]
#[ignore = "owned isolated tray D-Bus fixture, no host session bus"]
fn tray_protocol_stale_revision_and_disabled_row_do_not_execute() {
    let bus = Bus::new();
    let watch = Watch::new(&bus);
    watch.state(|s| s.items.is_empty());
    let state = Arc::new(Mutex::new(State {
        revision: 1,
        ..Default::default()
    }));
    let app = futures_lite::future::block_on(application(&bus, state.clone()));
    futures_lite::future::block_on(register(&app, "/StatusNotifierItem")).unwrap();
    let id = watch.state(|s| s.items.len() == 1).items[0].id;
    watch.adapter.menu(id, 0).unwrap();
    state.lock().unwrap().revision = 2;
    assert!(matches!(
        watch.adapter.invoke(id, 1, 1, 0),
        Err(DomainError::Invalid(_))
    ));
    state.lock().unwrap().disabled = true;
    assert!(matches!(
        watch.adapter.invoke(id, 2, 1, 0),
        Err(DomainError::Invalid(_))
    ));
    assert!(state.lock().unwrap().actions.is_empty());
}
#[test]
#[ignore = "owned isolated tray D-Bus fixture, no host session bus"]
fn tray_protocol_competing_watcher_is_not_replaced() {
    let bus = Bus::new();
    let existing = futures_lite::future::block_on(
        zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name("org.kde.StatusNotifierWatcher")
            .unwrap()
            .build(),
    )
    .unwrap();
    let watch = Watch::new(&bus);
    assert!(
        watch
            .snapshots
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .is_err()
    );
    let proxy = futures_lite::future::block_on(zbus::fdo::DBusProxy::new(&existing)).unwrap();
    assert_eq!(
        futures_lite::future::block_on(
            proxy.get_name_owner("org.kde.StatusNotifierWatcher".try_into().unwrap())
        )
        .unwrap()
        .as_str(),
        existing.unique_name().unwrap().as_str()
    );
}

#[test]
#[ignore = "owned isolated tray D-Bus fixture, no host session bus"]
fn tray_protocol_slow_item_cannot_consume_all_refresh_time() {
    let bus = Bus::new();
    let watch = Watch::new(&bus);
    watch.state(|s| s.items.is_empty());
    let slow_state = Arc::new(Mutex::new(State {
        slow_status: true,
        ..Default::default()
    }));
    let fast_state = Arc::new(Mutex::new(State::default()));
    let slow = futures_lite::future::block_on(application(&bus, slow_state));
    let fast = futures_lite::future::block_on(
        zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .serve_at("/StatusNotifierItem", Application(fast_state))
            .unwrap()
            .build(),
    )
    .unwrap();
    futures_lite::future::block_on(register(&slow, "/StatusNotifierItem")).unwrap();
    futures_lite::future::block_on(register(&fast, "/StatusNotifierItem")).unwrap();
    let state = watch.state(|s| s.items.len() == 1);
    assert_eq!(state.items[0].title, "Fixture app");
    watch
        .adapter
        .act(state.items[0].id, TrayAction::Activate { x: 0, y: 0 })
        .unwrap();
}
