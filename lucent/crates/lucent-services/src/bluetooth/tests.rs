use super::{native::*, *};
use std::{
    collections::{BTreeSet, HashMap},
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    thread,
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
#[path = "pairing_tests.rs"]
mod pairing_tests;
const ADAPT: &str = "/org/bluez/hci0";
const DEV: &str = "/org/bluez/hci0/dev_02_00_00_00_00_02";
fn path(value: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(value).unwrap()
}
fn value(v: impl Into<Value<'static>>) -> OwnedValue {
    v.into().try_to_owned().unwrap()
}
fn properties<const N: usize>(values: [(&str, OwnedValue); N]) -> Properties {
    values.into_iter().map(|(k, v)| (k.into(), v)).collect()
}
fn raw() -> Managed {
    Managed::from([
        (
            path(ADAPT),
            HashMap::from([(
                ADAPTER.into(),
                properties([
                    ("Address", value("02:00:00:00:00:01")),
                    ("Alias", value("Controller")),
                    ("Powered", value(true)),
                    ("Discoverable", value(false)),
                    ("Pairable", value(true)),
                    ("Discovering", value(true)),
                ]),
            )]),
        ),
        (
            path(DEV),
            HashMap::from([
                (
                    DEVICE.into(),
                    properties([
                        ("Adapter", value(path(ADAPT))),
                        ("Address", value("02:00:00:00:00:02")),
                        ("Alias", value("Headphones")),
                        ("Paired", value(true)),
                        ("Trusted", value(false)),
                        ("Blocked", value(false)),
                        ("Connected", value(false)),
                        ("ServicesResolved", value(false)),
                        ("RSSI", value(-64_i16)),
                    ]),
                ),
                (BATTERY.into(), properties([("Percentage", value(75_u8))])),
            ]),
        ),
    ])
}
#[test]
fn catalog_retains_live_ids_but_retires_device_and_controller_dependents() {
    let mut catalog = catalog::Catalog::default();
    let mut owned = BTreeSet::new();
    let mut next = 1;
    let mut allocate = || {
        let id = BluetoothObjectId::new(next);
        next += 1;
        id
    };
    let first = catalog.replace(raw(), &mut owned, &mut allocate).unwrap();
    assert_eq!(
        first,
        catalog.replace(raw(), &mut owned, &mut allocate).unwrap()
    );
    catalog.invalidate(&BTreeSet::from([ADAPT.into()]));
    assert!(
        catalog
            .resolve(first.devices[0].id, catalog::Kind::Device)
            .is_err()
    );
    let next = catalog.replace(raw(), &mut owned, &mut allocate).unwrap();
    assert_ne!(first.adapters[0].id, next.adapters[0].id);
    assert_ne!(first.devices[0].id, next.devices[0].id);
    catalog.invalidate(&BTreeSet::from([DEV.into()]));
    let last = catalog.replace(raw(), &mut owned, &mut allocate).unwrap();
    assert_eq!(next.adapters[0].id, last.adapters[0].id);
    assert_ne!(next.devices[0].id, last.devices[0].id);
}
#[test]
fn optional_metadata_and_removal_do_not_turn_errors_into_empty_ready_state() {
    let mut catalog = catalog::Catalog::default();
    let mut owned = BTreeSet::from([ADAPT.into()]);
    let mut next = 1;
    let mut allocate = || {
        let id = BluetoothObjectId::new(next);
        next += 1;
        id
    };
    let first = catalog.replace(raw(), &mut owned, &mut allocate).unwrap();
    assert!(first.adapters[0].discovery_owned);
    assert_eq!(first.devices[0].rssi, Some(-64));
    assert_eq!(first.devices[0].battery_percent, Some(75));
    let mut data = raw();
    data.get_mut(&path(DEV)).unwrap().remove(BATTERY);
    let next = catalog.replace(data, &mut owned, &mut allocate).unwrap();
    assert_eq!(next.devices[0].id, first.devices[0].id);
    assert_eq!(next.devices[0].battery_percent, None);
    let mut invalid = raw();
    invalid
        .get_mut(&path(DEV))
        .unwrap()
        .get_mut(BATTERY)
        .unwrap()
        .insert("Percentage".into(), value(101_u8));
    assert!(catalog.replace(invalid, &mut owned, &mut allocate).is_err());
    let mut invalid = raw();
    invalid
        .get_mut(&path(ADAPT))
        .unwrap()
        .get_mut(ADAPTER)
        .unwrap()
        .remove("Powered");
    assert!(catalog.replace(invalid, &mut owned, &mut allocate).is_err());
    let mut data = raw();
    data.remove(&path(ADAPT));
    let state = catalog.replace(data, &mut owned, &mut allocate).unwrap();
    assert!(state.adapters.is_empty() && state.devices.is_empty() && owned.is_empty());
}
#[test]
fn service_admission_is_bounded_and_rejects_without_backend_work() {
    let adapter = BlueZ::default();
    let command = BluetoothCommand::SetPowered {
        adapter: BluetoothObjectId::new(1).unwrap(),
        powered: true,
    };
    assert!(adapter.control(command.clone()).is_err());
    let (send, _receive) = async_channel::bounded(REQUEST_LIMIT);
    for _ in 0..REQUEST_LIMIT {
        send.try_send(Request {
            command: Intent::Control(command.clone()),
            reply: mpsc::sync_channel(1).0,
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + OPERATION_TIMEOUT,
        })
        .unwrap();
    }
    *adapter.online.lock().unwrap() = Some(send);
    let start = Instant::now();
    assert!(adapter.control(command).is_err());
    assert!(start.elapsed() < Duration::from_millis(200));
}

struct Bus {
    child: Child,
    address: String,
}
impl Bus {
    fn new() -> Self {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        assert!(address.starts_with("unix:"));
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
struct State {
    powered: bool,
    discoverable: bool,
    pairable: bool,
    other_discovery: bool,
    owners: BTreeSet<String>,
    starts: usize,
    start_requests: usize,
    stops: usize,
    connected: bool,
    trusted: bool,
    connects: usize,
    disconnects: usize,
    connect_authorization: bool,
    delay: Duration,
    flood: usize,
    denied: bool,
    reads: usize,
    paired: bool,
    agent: Option<(String, OwnedObjectPath)>,
    registrations: usize,
    defaults: usize,
    unregistrations: usize,
    pairing_mode: PairingMode,
    pairing_cancelled: bool,
    pairing_completed: bool,
    received_pin: Option<String>,
    received_passkey: Option<u32>,
}
#[derive(Clone, Copy, Default)]
enum PairingMode {
    #[default]
    Confirmation,
    Pin,
    Passkey,
    Authorization,
    Display,
}
impl Default for State {
    fn default() -> Self {
        Self {
            powered: true,
            discoverable: false,
            pairable: true,
            other_discovery: true,
            owners: BTreeSet::new(),
            starts: 0,
            start_requests: 0,
            stops: 0,
            connected: false,
            trusted: false,
            connects: 0,
            disconnects: 0,
            connect_authorization: false,
            delay: Duration::ZERO,
            flood: 0,
            denied: false,
            reads: 0,
            paired: true,
            agent: None,
            registrations: 0,
            defaults: 0,
            unregistrations: 0,
            pairing_mode: PairingMode::default(),
            pairing_cancelled: false,
            pairing_completed: false,
            received_pin: None,
            received_passkey: None,
        }
    }
}
type Shared = Arc<Mutex<State>>;
async fn changed(
    connection: &zbus::Connection,
    object: &str,
    interface: &str,
    name: &str,
    val: OwnedValue,
) {
    connection
        .emit_signal(
            None::<&str>,
            object,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(interface, properties([(name, val)]), Vec::<String>::new()),
        )
        .await
        .unwrap();
}
#[derive(Clone)]
struct Controller(Shared);
#[zbus::interface(name = "org.bluez.Adapter1")]
impl Controller {
    #[zbus(property)]
    fn address(&self) -> &str {
        "02:00:00:00:00:01"
    }
    #[zbus(property)]
    fn alias(&self) -> &str {
        "Controller fixture"
    }
    #[zbus(property)]
    fn powered(&self) -> bool {
        self.0.lock().unwrap().reads += 1;
        self.0.lock().unwrap().powered
    }
    #[zbus(property)]
    fn set_powered(&self, value: bool) {
        let mut s = self.0.lock().unwrap();
        s.powered = value;
        if !value {
            s.owners.clear();
        }
    }
    #[zbus(property)]
    fn discoverable(&self) -> bool {
        self.0.lock().unwrap().discoverable
    }
    #[zbus(property)]
    fn set_discoverable(&self, value: bool) {
        self.0.lock().unwrap().discoverable = value;
    }
    #[zbus(property)]
    fn pairable(&self) -> bool {
        self.0.lock().unwrap().pairable
    }
    #[zbus(property)]
    fn set_pairable(&self, value: bool) {
        self.0.lock().unwrap().pairable = value;
    }
    #[zbus(property)]
    fn discovering(&self) -> bool {
        let s = self.0.lock().unwrap();
        s.other_discovery || !s.owners.is_empty()
    }
    async fn start_discovery(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let (delay, flood, denied) = {
            let mut s = self.0.lock().unwrap();
            s.start_requests += 1;
            (s.delay, s.flood, s.denied)
        };
        if denied {
            return Err(zbus::fdo::Error::AccessDenied(
                "private-device-metadata".into(),
            ));
        }
        for _ in 0..flood {
            changed(connection, DEV, DEVICE, "RSSI", value(-65_i16)).await;
        }
        async_io::Timer::after(delay).await;
        {
            let mut s = self.0.lock().unwrap();
            s.starts += 1;
            s.owners.insert(header.sender().unwrap().to_string());
        }
        changed(connection, ADAPT, ADAPTER, "Discovering", value(true)).await;
        Ok(())
    }
    async fn stop_discovery(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) {
        let discovering = {
            let mut s = self.0.lock().unwrap();
            s.stops += 1;
            s.owners.remove(header.sender().unwrap().as_str());
            s.other_discovery || !s.owners.is_empty()
        };
        changed(
            connection,
            ADAPT,
            ADAPTER,
            "Discovering",
            value(discovering),
        )
        .await;
    }
    async fn remove_device(
        &self,
        device: OwnedObjectPath,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        if device.as_str() != DEV {
            return Err(zbus::fdo::Error::InvalidArgs("wrong device".into()));
        }
        connection
            .object_server()
            .remove::<Remote, _>(DEV)
            .await
            .unwrap();
        Ok(())
    }
}
#[derive(Clone)]
struct Remote(Shared);
#[zbus::interface(name = "org.bluez.Device1")]
impl Remote {
    #[zbus(property)]
    fn adapter(&self) -> OwnedObjectPath {
        path(ADAPT)
    }
    #[zbus(property)]
    fn address(&self) -> &str {
        "02:00:00:00:00:02"
    }
    #[zbus(property)]
    fn alias(&self) -> &str {
        "Headphones fixture"
    }
    #[zbus(property)]
    fn icon(&self) -> &str {
        "audio-headphones"
    }
    #[zbus(property)]
    fn paired(&self) -> bool {
        self.0.lock().unwrap().paired
    }
    #[zbus(property)]
    fn trusted(&self) -> bool {
        self.0.lock().unwrap().trusted
    }
    #[zbus(property)]
    fn set_trusted(&self, v: bool) {
        self.0.lock().unwrap().trusted = v;
    }
    #[zbus(property)]
    fn blocked(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn connected(&self) -> bool {
        self.0.lock().unwrap().connected
    }
    #[zbus(property)]
    fn services_resolved(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn rssi(&self) -> i16 {
        -64
    }
    async fn connect(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let authorization = self.0.lock().unwrap().connect_authorization;
        if authorization {
            let (destination, agent_path) = self
                .0
                .lock()
                .unwrap()
                .agent
                .clone()
                .ok_or_else(|| zbus::fdo::Error::Failed("No application agent".into()))?;
            connection
                .call_method(
                    Some(destination.as_str()),
                    agent_path.as_str(),
                    Some("org.bluez.Agent1"),
                    "AuthorizeService",
                    &(path(DEV), "0000110b-0000-1000-8000-00805f9b34fb"),
                )
                .await
                .map_err(|_| zbus::fdo::Error::AccessDenied("private-device-metadata".into()))?;
        }
        {
            let mut s = self.0.lock().unwrap();
            s.connected = true;
            s.connects += 1;
        }
        changed(connection, DEV, DEVICE, "Connected", value(true)).await;
        Ok(())
    }
    async fn disconnect(&self, #[zbus(connection)] connection: &zbus::Connection) {
        let agent = {
            let mut s = self.0.lock().unwrap();
            s.connected = false;
            s.disconnects += 1;
            s.agent.clone()
        };
        if let Some((destination, agent_path)) = agent {
            let _ = connection
                .call_method(
                    Some(destination.as_str()),
                    agent_path.as_str(),
                    Some("org.bluez.Agent1"),
                    "Cancel",
                    &(),
                )
                .await;
        }
        changed(connection, DEV, DEVICE, "Connected", value(false)).await;
    }
    async fn pair(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let (mode, destination, agent_path) = {
            let mut s = self.0.lock().unwrap();
            s.pairing_cancelled = false;
            let (destination, agent_path) = s
                .agent
                .clone()
                .ok_or_else(|| zbus::fdo::Error::Failed("No explicit application agent".into()))?;
            (s.pairing_mode, destination, agent_path)
        };
        let result = match mode {
            PairingMode::Confirmation => {
                connection
                    .call_method(
                        Some(destination.as_str()),
                        agent_path.as_str(),
                        Some("org.bluez.Agent1"),
                        "RequestConfirmation",
                        &(path(DEV), 17_u32),
                    )
                    .await
            }
            PairingMode::Pin => {
                connection
                    .call_method(
                        Some(destination.as_str()),
                        agent_path.as_str(),
                        Some("org.bluez.Agent1"),
                        "RequestPinCode",
                        &(path(DEV),),
                    )
                    .await
            }
            PairingMode::Passkey => {
                connection
                    .call_method(
                        Some(destination.as_str()),
                        agent_path.as_str(),
                        Some("org.bluez.Agent1"),
                        "RequestPasskey",
                        &(path(DEV),),
                    )
                    .await
            }
            PairingMode::Authorization => {
                connection
                    .call_method(
                        Some(destination.as_str()),
                        agent_path.as_str(),
                        Some("org.bluez.Agent1"),
                        "RequestAuthorization",
                        &(path(DEV),),
                    )
                    .await
            }
            PairingMode::Display => {
                for entered in [0_u16, 3] {
                    connection
                        .call_method(
                            Some(destination.as_str()),
                            agent_path.as_str(),
                            Some("org.bluez.Agent1"),
                            "DisplayPasskey",
                            &(path(DEV), 17_u32, entered),
                        )
                        .await
                        .map_err(|_| zbus::fdo::Error::Failed("Display failed".into()))?;
                    async_io::Timer::after(Duration::from_millis(180)).await;
                }
                loop {
                    let (cancelled, complete) = {
                        let s = self.0.lock().unwrap();
                        (s.pairing_cancelled, s.pairing_completed)
                    };
                    if cancelled {
                        return Err(zbus::fdo::Error::Failed("Pairing cancelled".into()));
                    }
                    if complete {
                        break;
                    }
                    async_io::Timer::after(Duration::from_millis(10)).await;
                }
                connection
                    .call_method(
                        Some(destination.as_str()),
                        agent_path.as_str(),
                        Some("org.bluez.Agent1"),
                        "Cancel",
                        &(),
                    )
                    .await
            }
        };
        let reply =
            result.map_err(|_| zbus::fdo::Error::Failed("Pairing not authorized".into()))?;
        {
            let mut s = self.0.lock().unwrap();
            if s.pairing_cancelled {
                return Err(zbus::fdo::Error::Failed("Pairing cancelled".into()));
            }
            if matches!(mode, PairingMode::Pin) {
                s.received_pin = Some(reply.body().deserialize().unwrap());
            }
            if matches!(mode, PairingMode::Passkey) {
                s.received_passkey = Some(reply.body().deserialize().unwrap());
            }
            s.paired = true;
        }
        changed(connection, DEV, DEVICE, "Paired", value(true)).await;
        Ok(())
    }
    async fn cancel_pairing(&self, #[zbus(connection)] connection: &zbus::Connection) {
        let target = {
            let mut s = self.0.lock().unwrap();
            s.pairing_cancelled = true;
            s.agent.clone()
        };
        if let Some((destination, agent_path)) = target {
            let _ = connection
                .call_method(
                    Some(destination.as_str()),
                    agent_path.as_str(),
                    Some("org.bluez.Agent1"),
                    "Cancel",
                    &(),
                )
                .await;
        }
    }
}
#[derive(Clone)]
struct AgentManager(Shared);
#[zbus::interface(name = "org.bluez.AgentManager1")]
impl AgentManager {
    fn register_agent(
        &self,
        agent_path: OwnedObjectPath,
        capability: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        if capability != "KeyboardDisplay" || agent_path.as_str() != agent::AGENT_PATH {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Wrong application agent".into(),
            ));
        }
        let mut s = self.0.lock().unwrap();
        s.registrations += 1;
        s.agent = Some((header.sender().unwrap().to_string(), agent_path));
        Ok(())
    }
    fn request_default_agent(&self, agent_path: OwnedObjectPath) -> zbus::fdo::Result<()> {
        let mut s = self.0.lock().unwrap();
        if s.agent.as_ref().is_none_or(|(_, p)| p != &agent_path) {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Unregistered application agent".into(),
            ));
        }
        s.defaults += 1;
        Ok(())
    }
    fn unregister_agent(&self, agent_path: OwnedObjectPath) -> zbus::fdo::Result<()> {
        let mut s = self.0.lock().unwrap();
        if s.agent.as_ref().is_none_or(|(_, p)| p != &agent_path) {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Unregistered application agent".into(),
            ));
        }
        s.unregistrations += 1;
        s.agent = None;
        Ok(())
    }
}
struct Battery;
#[zbus::interface(name = "org.bluez.Battery1")]
impl Battery {
    #[zbus(property)]
    fn percentage(&self) -> u8 {
        75
    }
}
async fn server(bus: &Bus, state: &Shared) -> zbus::Connection {
    zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name(SERVICE)
        .unwrap()
        .serve_at("/", zbus::fdo::ObjectManager)
        .unwrap()
        .serve_at("/org/bluez", AgentManager(state.clone()))
        .unwrap()
        .serve_at(ADAPT, Controller(state.clone()))
        .unwrap()
        .serve_at(DEV, Remote(state.clone()))
        .unwrap()
        .serve_at(DEV, Battery)
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
    adapter: Arc<BlueZ>,
    stop: Arc<Stop>,
    thread: Option<thread::JoinHandle<()>>,
    events: mpsc::Receiver<Result<BluetoothSnapshot>>,
}
impl Observer {
    fn new(bus: &Bus, timeout: Duration) -> Self {
        let adapter = Arc::new(BlueZ {
            timeout,
            interactive_timeout: timeout,
            ..BlueZ::with_address(&bus.address)
        });
        let stop = Arc::new(Stop::default());
        let (send, events) = mpsc::sync_channel(16);
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
    fn state(&self, predicate: impl Fn(&BluetoothSnapshot) -> bool) -> BluetoothSnapshot {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let event = self
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("Bluetooth observation timed out");
            if let Ok(state) = event
                && predicate(&state)
            {
                return state;
            }
        }
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        self.stop.0.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}
#[test]
#[ignore = "isolated BlueZ D-Bus protocol; never accesses the host system bus"]
fn bluetooth_protocol_controls_and_discovery_ownership() {
    let bus = Bus::new();
    let state = Arc::new(Mutex::new(State::default()));
    let server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(2));
    let initial = watch.state(|s| s.adapters.len() == 1 && s.devices.len() == 1);
    let controller = initial.adapters[0].id;
    let device = initial.devices[0].id;
    assert!(initial.adapters[0].discovering && !initial.adapters[0].discovery_owned);
    watch
        .adapter
        .control(BluetoothCommand::SetDiscovery {
            adapter: controller,
            active: false,
        })
        .unwrap();
    assert_eq!(state.lock().unwrap().stops, 0);
    watch
        .adapter
        .control(BluetoothCommand::SetDiscovery {
            adapter: controller,
            active: true,
        })
        .unwrap();
    watch.state(|s| s.adapters[0].discovery_owned);
    watch
        .adapter
        .control(BluetoothCommand::SetDiscovery {
            adapter: controller,
            active: true,
        })
        .unwrap();
    assert_eq!(state.lock().unwrap().starts, 1);
    watch
        .adapter
        .control(BluetoothCommand::SetDiscovery {
            adapter: controller,
            active: false,
        })
        .unwrap();
    let stopped = watch.state(|s| !s.adapters[0].discovery_owned);
    assert!(stopped.adapters[0].discovering);
    assert_eq!(state.lock().unwrap().stops, 1);
    watch
        .adapter
        .control(BluetoothCommand::Connect(device))
        .unwrap();
    let connected = watch.state(|s| s.devices[0].connected);
    assert!(!connected.devices[0].services_resolved);
    watch
        .adapter
        .control(BluetoothCommand::SetTrusted {
            device,
            trusted: true,
        })
        .unwrap();
    watch.state(|s| s.devices[0].trusted);
    watch
        .adapter
        .control(BluetoothCommand::Disconnect(device))
        .unwrap();
    watch.state(|s| !s.devices[0].connected);
    watch
        .adapter
        .control(BluetoothCommand::SetPowered {
            adapter: controller,
            powered: false,
        })
        .unwrap();
    watch.state(|s| !s.adapters[0].powered);
    watch
        .adapter
        .control(BluetoothCommand::SetPowered {
            adapter: controller,
            powered: true,
        })
        .unwrap();
    watch.state(|s| s.adapters[0].powered);
    watch
        .adapter
        .control(BluetoothCommand::Forget(device))
        .unwrap();
    watch.state(|s| s.devices.is_empty());
    drop(watch);
    drop(server);
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol; no host radio"]
fn bluetooth_protocol_radio_flags_are_observed_without_discovery_or_agent_authority() {
    let bus = Bus::new();
    let state = Arc::new(Mutex::new(State::default()));
    let server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(2));
    let first = watch.state(|s| s.adapters.len() == 1);
    let adapter = first.adapters[0].id;
    assert!(!first.adapters[0].discoverable && first.adapters[0].pairable);
    watch
        .adapter
        .control(BluetoothCommand::SetDiscoverable {
            adapter,
            discoverable: true,
        })
        .unwrap();
    watch.state(|s| s.adapters[0].discoverable);
    watch
        .adapter
        .control(BluetoothCommand::SetPairable {
            adapter,
            pairable: false,
        })
        .unwrap();
    let observed = watch.state(|s| !s.adapters[0].pairable);
    assert!(observed.adapters[0].discovering && !observed.adapters[0].discovery_owned);
    assert!(!observed.incoming_pairing_enabled);
    assert_eq!(state.lock().unwrap().registrations, 0);
    assert_eq!(state.lock().unwrap().defaults, 0);
    assert!(!state.lock().unwrap().trusted);

    // Native timeout or another settings client may hide the controller.
    state.lock().unwrap().discoverable = false;
    futures_lite::future::block_on(changed(
        &server,
        ADAPT,
        ADAPTER,
        "Discoverable",
        value(false),
    ));
    let expired = watch.state(|s| !s.adapters[0].discoverable);
    assert_eq!(expired.adapters[0].id, adapter);
    watch
        .adapter
        .control(BluetoothCommand::SetPowered {
            adapter,
            powered: false,
        })
        .unwrap();
    watch.state(|s| !s.adapters[0].powered);
    assert!(
        watch
            .adapter
            .control(BluetoothCommand::SetDiscoverable {
                adapter,
                discoverable: true
            })
            .is_err()
    );
    watch
        .adapter
        .control(BluetoothCommand::SetPairable {
            adapter,
            pairable: true,
        })
        .unwrap();
    watch.state(|s| s.adapters[0].pairable);
    assert_eq!(state.lock().unwrap().registrations, 0);
}
#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_interface_removal_and_owner_reconnect_retire_handles() {
    let bus = Bus::new();
    let state = Arc::new(Mutex::new(State::default()));
    let server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(2));
    let first = watch.state(|s| s.devices.len() == 1);
    futures_lite::future::block_on(server.object_server().remove::<Battery, _>(DEV)).unwrap();
    let next = watch.state(|s| s.devices[0].battery_percent.is_none());
    assert_eq!(first.devices[0].id, next.devices[0].id);
    futures_lite::future::block_on(server.object_server().remove::<Remote, _>(DEV)).unwrap();
    watch.state(|s| s.devices.is_empty());
    assert!(
        watch
            .adapter
            .control(BluetoothCommand::Connect(first.devices[0].id))
            .is_err()
    );
    futures_lite::future::block_on(server.object_server().at(DEV, Remote(state.clone()))).unwrap();
    let next = watch.state(|s| s.devices.len() == 1);
    assert_ne!(first.devices[0].id, next.devices[0].id);
    futures_lite::future::block_on(server.release_name(SERVICE)).unwrap();
    futures_lite::future::block_on(server.request_name(SERVICE)).unwrap();
    let last = watch.state(|s| s.adapters[0].id != next.adapters[0].id);
    assert_ne!(next.devices[0].id, last.devices[0].id);
    assert!(
        watch
            .adapter
            .control(BluetoothCommand::Disconnect(next.devices[0].id))
            .is_err()
    );
    assert_eq!(state.lock().unwrap().connects, 0);
}
#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_signal_pressure_timeout_and_error_privacy() {
    let bus = Bus::new();
    let state = Arc::new(Mutex::new(State::default()));
    let _server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_millis(500));
    let first = watch.state(|s| s.adapters.len() == 1);
    state.lock().unwrap().denied = true;
    let error = watch
        .adapter
        .control(BluetoothCommand::SetDiscovery {
            adapter: first.adapters[0].id,
            active: true,
        })
        .unwrap_err();
    assert!(!error.to_string().contains("private-device-metadata"));
    {
        let mut s = state.lock().unwrap();
        s.denied = false;
        s.flood = 512;
    }
    watch
        .adapter
        .control(BluetoothCommand::SetDiscovery {
            adapter: first.adapters[0].id,
            active: true,
        })
        .unwrap();
    watch.state(|s| s.adapters[0].discovery_owned);
    watch
        .adapter
        .control(BluetoothCommand::SetDiscovery {
            adapter: first.adapters[0].id,
            active: false,
        })
        .unwrap();
    watch.state(|s| !s.adapters[0].discovery_owned);
    {
        let mut s = state.lock().unwrap();
        s.flood = 0;
        s.delay = Duration::from_millis(900);
    }
    assert!(
        watch
            .adapter
            .control(BluetoothCommand::SetDiscovery {
                adapter: first.adapters[0].id,
                active: true
            })
            .is_err()
    );
    let next = watch.state(|s| s.adapters[0].id != first.adapters[0].id);
    assert!(
        watch
            .adapter
            .control(BluetoothCommand::SetDiscovery {
                adapter: first.adapters[0].id,
                active: false
            })
            .is_err()
    );
    assert_ne!(first.devices[0].id, next.devices[0].id);
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_idle_filters_and_observer_connection_lifetime() {
    let bus = Bus::new();
    let state = Arc::new(Mutex::new(State::default()));
    let server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(2));
    let first = watch.state(|s| s.adapters.len() == 1);
    let mut duplicate = vec![];
    watch
        .adapter
        .watch(&mut |v| duplicate.push(v), &Stop::default());
    assert!(matches!(
        duplicate.as_slice(),
        [Err(DomainError::Unavailable(_))]
    ));
    let reads = state.lock().unwrap().reads;
    for _ in 0..16 {
        futures_lite::future::block_on(changed(
            &server,
            DEV,
            "org.bluez.GattCharacteristic1",
            "Notifying",
            value(true),
        ));
        futures_lite::future::block_on(async {
            server
                .emit_signal(
                    None::<&str>,
                    "/",
                    "org.freedesktop.DBus.ObjectManager",
                    "InterfacesAdded",
                    &(
                        path(DEV),
                        HashMap::from([(
                            "org.bluez.GattCharacteristic1".to_string(),
                            Properties::new(),
                        )]),
                    ),
                )
                .await
                .unwrap();
            server
                .emit_signal(
                    None::<&str>,
                    "/",
                    "org.freedesktop.DBus.ObjectManager",
                    "InterfacesRemoved",
                    &(path(DEV), vec!["org.bluez.GattCharacteristic1".to_string()]),
                )
                .await
                .unwrap();
        });
    }
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        state.lock().unwrap().reads,
        reads,
        "Unrelated signals must not refresh the controller catalog"
    );
    watch
        .adapter
        .control(BluetoothCommand::SetDiscovery {
            adapter: first.adapters[0].id,
            active: true,
        })
        .unwrap();
    watch.state(|s| s.adapters[0].discovery_owned);
    let owner = state.lock().unwrap().owners.first().unwrap().clone();
    let adapter = watch.adapter.clone();
    let start = Instant::now();
    drop(watch);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(
        adapter
            .control(BluetoothCommand::SetDiscovery {
                adapter: first.adapters[0].id,
                active: false,
            })
            .is_err()
    );
    let owned: bool = futures_lite::future::block_on(async {
        server
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "NameHasOwner",
                &(owner,),
            )
            .await
            .unwrap()
            .body()
            .deserialize()
            .unwrap()
    });
    assert!(
        !owned,
        "The observing connection must close on cancellation"
    );
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_empty_ready_is_distinct_from_absent_and_disconnected_bus() {
    let mut bus = Bus::new();
    let watch = Observer::new(&bus, Duration::from_secs(1));
    assert!(matches!(
        watch.events.recv_timeout(Duration::from_secs(2)).unwrap(),
        Err(DomainError::Unavailable(_))
    ));
    let _server = futures_lite::future::block_on(async {
        zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at("/", zbus::fdo::ObjectManager)
            .unwrap()
            .build()
            .await
            .unwrap()
    });
    let empty = watch.state(|s| s.adapters.is_empty() && s.devices.is_empty());
    assert_eq!(empty, BluetoothSnapshot::default());
    bus.child.kill().unwrap();
    bus.child.wait().unwrap();
    assert!(matches!(
        watch.events.recv_timeout(Duration::from_secs(2)).unwrap(),
        Err(DomainError::Unavailable(_))
    ));
    assert!(
        watch
            .adapter
            .control(BluetoothCommand::SetPowered {
                adapter: BluetoothObjectId::new(1).unwrap(),
                powered: true,
            })
            .is_err()
    );
    let start = Instant::now();
    drop(watch);
    assert!(start.elapsed() < Duration::from_secs(1));
}
