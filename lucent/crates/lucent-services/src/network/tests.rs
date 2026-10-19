use super::{native::*, *};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    thread,
};
use zbus::{
    Connection,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

const DEV: &str = "/org/freedesktop/NetworkManager/Devices/1";
const ACCESS: &str = "/org/freedesktop/NetworkManager/AccessPoint/1";
const HIDDEN: &str = "/org/freedesktop/NetworkManager/AccessPoint/2";
const SAVED: &str = "/org/freedesktop/NetworkManager/Settings/1";
const LIVE: &str = "/org/freedesktop/NetworkManager/ActiveConnection/1";
fn path(text: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(text).unwrap()
}
fn value(v: impl Into<Value<'static>>) -> OwnedValue {
    v.into().try_to_owned().unwrap()
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
            .expect("network tests require dbus-daemon");
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
#[derive(Default)]
struct RecordedJoin {
    ssid: Vec<u8>,
    hidden: bool,
    key: String,
    secret_matches: bool,
    persist: String,
}
struct State {
    wifi: bool,
    present: bool,
    saved: bool,
    state: u32,
    reason: u32,
    active: u32,
    last_scan: i64,
    strength: u8,
    scans: usize,
    query_count: usize,
    joins: Vec<RecordedJoin>,
    scan_delay: Duration,
    scan_denied: bool,
    scan_unknown: bool,
    flood: usize,
    oversized: bool,
    missing_read: Option<zbus::fdo::Error>,
    credential_agent: Option<String>,
    secret_replies: usize,
    activations: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            wifi: true,
            present: true,
            saved: true,
            state: 30,
            reason: 0,
            active: 0,
            last_scan: -1,
            strength: 75,
            scans: 0,
            query_count: 0,
            joins: vec![],
            scan_delay: Duration::ZERO,
            scan_denied: false,
            scan_unknown: false,
            flood: 0,
            oversized: false,
            missing_read: None,
            credential_agent: None,
            secret_replies: 0,
            activations: 0,
        }
    }
}
type Shared = Arc<Mutex<State>>;
#[derive(Clone)]
struct Manager(Shared);
#[zbus::interface(name = "org.freedesktop.NetworkManager")]
impl Manager {
    #[zbus(property)]
    fn devices(&self) -> Vec<OwnedObjectPath> {
        if self.0.lock().unwrap().present {
            vec![path(DEV)]
        } else {
            vec![]
        }
    }
    #[zbus(property)]
    fn active_connections(&self) -> Vec<OwnedObjectPath> {
        if self.0.lock().unwrap().active != 0 {
            vec![path(LIVE)]
        } else {
            vec![]
        }
    }
    #[zbus(property)]
    fn primary_connection(&self) -> OwnedObjectPath {
        if self.0.lock().unwrap().active == 2 {
            path(LIVE)
        } else {
            path("/")
        }
    }
    #[zbus(property)]
    fn state(&self) -> u32 {
        50
    }
    #[zbus(property)]
    fn connectivity(&self) -> u32 {
        3
    }
    #[zbus(property)]
    fn wireless_enabled(&self) -> bool {
        self.0.lock().unwrap().wifi
    }
    #[zbus(property)]
    fn set_wireless_enabled(&mut self, enabled: bool) {
        self.0.lock().unwrap().wifi = enabled;
    }
    #[zbus(property)]
    fn wireless_hardware_enabled(&self) -> bool {
        true
    }
    async fn activate_connection(
        &self,
        profile: OwnedObjectPath,
        device: OwnedObjectPath,
        _specific: OwnedObjectPath,
        #[zbus(connection)] conn: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        assert_eq!(profile.as_str(), SAVED);
        assert!(device.as_str() == DEV || device.as_str() == "/");
        {
            let mut state = self.0.lock().unwrap();
            state.active = 1;
            state.state = 40;
            state.activations += 1;
        }
        changed(conn, ROOT, NM).await;
        let agent = self.0.lock().unwrap().credential_agent.clone();
        if let Some(agent) = agent {
            // Deliberately hold Activate's reply until the interactive callback
            // returns. A single blocked control lane cannot deliver this answer.
            let reply = conn
                .call_method(
                    Some(agent.as_str()),
                    agent::PATH,
                    Some("org.freedesktop.NetworkManager.SecretAgent"),
                    "GetSecrets",
                    &(
                        secret_settings(),
                        path(SAVED),
                        "802-11-wireless-security",
                        Vec::<String>::new(),
                        3u32,
                    ),
                )
                .await
                .map_err(|_| {
                    zbus::fdo::Error::Failed("Credential request did not complete".into())
                })?;
            let reply: Settings = reply
                .body()
                .deserialize()
                .map_err(|_| zbus::fdo::Error::Failed("Invalid secret reply".into()))?;
            assert_eq!(
                <&str>::try_from(&reply["802-11-wireless-security"]["psk"]).unwrap(),
                "test-password"
            );
            let mut state = self.0.lock().unwrap();
            state.secret_replies += 1;
            state.active = 2;
            state.state = 100;
        }
        changed(conn, ROOT, NM).await;
        Ok(path(LIVE))
    }
    async fn deactivate_connection(
        &self,
        active: OwnedObjectPath,
        #[zbus(connection)] conn: &Connection,
    ) -> zbus::fdo::Result<()> {
        assert_eq!(active.as_str(), LIVE);
        {
            let mut state = self.0.lock().unwrap();
            state.active = 0;
            state.state = 30;
        }
        changed(conn, ROOT, NM).await;
        Ok(())
    }
    async fn add_and_activate_connection2(
        &self,
        settings: Settings,
        device: OwnedObjectPath,
        ap: OwnedObjectPath,
        options: Properties,
        #[zbus(connection)] conn: &Connection,
    ) -> zbus::fdo::Result<(OwnedObjectPath, OwnedObjectPath, Properties)> {
        assert_eq!(device.as_str(), DEV);
        assert!(ap.as_str() == ACCESS || ap.as_str() == "/");
        let wireless = &settings["802-11-wireless"];
        let security = settings.get("802-11-wireless-security");
        let join = RecordedJoin {
            ssid: Vec::<u8>::try_from(wireless["ssid"].try_clone().unwrap()).unwrap(),
            hidden: bool::try_from(&wireless["hidden"]).unwrap(),
            key: security
                .and_then(|s| s.get("key-mgmt"))
                .map(|v| <&str>::try_from(v).unwrap().into())
                .unwrap_or_default(),
            secret_matches: security
                .and_then(|s| s.get("psk"))
                .is_some_and(|v| <&str>::try_from(v).unwrap() == "test-password"),
            persist: <&str>::try_from(&options["persist"]).unwrap().into(),
        };
        {
            let mut state = self.0.lock().unwrap();
            state.joins.push(join);
            state.active = 1;
            state.state = 40;
            state.saved = true;
        }
        changed(conn, ROOT, NM).await;
        Ok((path(SAVED), path(LIVE), Properties::new()))
    }
}
#[derive(Clone)]
struct Device(Shared);
#[zbus::interface(name = "org.freedesktop.NetworkManager.Device")]
impl Device {
    #[zbus(property)]
    fn interface(&self) -> &str {
        "wifi-test"
    }
    #[zbus(property)]
    fn device_type(&self) -> u32 {
        2
    }
    #[zbus(property)]
    fn managed(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn state(&self) -> u32 {
        self.0.lock().unwrap().state
    }
    #[zbus(property)]
    fn state_reason(&self) -> (u32, u32) {
        let state = self.0.lock().unwrap();
        (state.state, state.reason)
    }
    #[zbus(property)]
    fn active_connection(&self) -> OwnedObjectPath {
        if self.0.lock().unwrap().active != 0 {
            path(LIVE)
        } else {
            path("/")
        }
    }
}
#[derive(Clone)]
struct Wireless(Shared);
#[zbus::interface(name = "org.freedesktop.NetworkManager.Device.Wireless")]
impl Wireless {
    #[zbus(property)]
    fn active_access_point(&self) -> OwnedObjectPath {
        if self.0.lock().unwrap().active != 0 {
            path(ACCESS)
        } else {
            path("/")
        }
    }
    #[zbus(property)]
    fn last_scan(&self) -> i64 {
        self.0.lock().unwrap().last_scan
    }
    fn get_all_access_points(&self) -> Vec<OwnedObjectPath> {
        if self.0.lock().unwrap().oversized {
            (1..=513)
                .map(|n| path(&format!("/org/freedesktop/NetworkManager/AccessPoint/{n}")))
                .collect()
        } else {
            vec![path(ACCESS), path(HIDDEN)]
        }
    }
    async fn request_scan(
        &self,
        _options: Properties,
        #[zbus(connection)] conn: &Connection,
    ) -> zbus::fdo::Result<()> {
        let (delay, denied, unknown, flood) = {
            let mut state = self.0.lock().unwrap();
            state.scans += 1;
            (
                state.scan_delay,
                state.scan_denied,
                state.scan_unknown,
                state.flood,
            )
        };
        if unknown {
            return Err(zbus::fdo::Error::UnknownMethod(
                "Unsupported control".into(),
            ));
        }
        if denied {
            return Err(zbus::fdo::Error::AccessDenied(
                "test-password must never reach client diagnostics".into(),
            ));
        }
        for _ in 0..flood {
            changed(conn, DEV, WIFI).await;
        }
        if !delay.is_zero() {
            async_io::Timer::after(delay).await;
        }
        Ok(())
    }
}
#[derive(Clone)]
struct AccessPoint(Shared, bool);
#[zbus::interface(name = "org.freedesktop.NetworkManager.AccessPoint")]
impl AccessPoint {
    #[zbus(property)]
    fn ssid(&self) -> Vec<u8> {
        if self.1 {
            vec![]
        } else {
            vec![b'a', b';', 0xff, b'\n']
        }
    }
    #[zbus(property)]
    fn strength(&self) -> u8 {
        self.0.lock().unwrap().strength
    }
    #[zbus(property)]
    fn flags(&self) -> u32 {
        1
    }
    #[zbus(property)]
    fn wpa_flags(&self) -> u32 {
        0
    }
    #[zbus(property)]
    fn rsn_flags(&self) -> u32 {
        0x108
    }
}
#[derive(Clone)]
struct Profiles(Shared);
#[zbus::interface(name = "org.freedesktop.NetworkManager.Settings")]
impl Profiles {
    fn list_connections(&self) -> Vec<OwnedObjectPath> {
        if self.0.lock().unwrap().saved {
            vec![path(SAVED)]
        } else {
            vec![]
        }
    }
}
#[derive(Clone)]
struct Profile(Shared);
#[zbus::interface(name = "org.freedesktop.NetworkManager.Settings.Connection")]
impl Profile {
    fn get_settings(&self) -> zbus::fdo::Result<Settings> {
        {
            let mut state = self.0.lock().unwrap();
            state.query_count += 1;
            if let Some(error) = state.missing_read.take() {
                return Err(error);
            }
        }
        Ok(Settings::from([
            (
                "connection".into(),
                Properties::from([
                    ("uuid".into(), value("test-stable-profile")),
                    ("id".into(), value("Saved Wi-Fi")),
                    ("type".into(), value("802-11-wireless")),
                ]),
            ),
            (
                "802-11-wireless".into(),
                Properties::from([("ssid".into(), value(vec![b'a', b';', 0xff, b'\n']))]),
            ),
        ]))
    }
    async fn delete(&self, #[zbus(connection)] conn: &Connection) {
        self.0.lock().unwrap().saved = false;
        conn.emit_signal(None::<&str>, SAVED, PROFILE, "Removed", &())
            .await
            .unwrap();
    }
}
#[derive(Clone)]
struct Active(Shared);
#[zbus::interface(name = "org.freedesktop.NetworkManager.Connection.Active")]
impl Active {
    #[zbus(property)]
    fn id(&self) -> &str {
        "Saved Wi-Fi"
    }
    #[zbus(property)]
    fn r#type(&self) -> &str {
        "802-11-wireless"
    }
    #[zbus(property)]
    fn connection(&self) -> OwnedObjectPath {
        path(SAVED)
    }
    #[zbus(property)]
    fn devices(&self) -> Vec<OwnedObjectPath> {
        vec![path(DEV)]
    }
    #[zbus(property, name = "State")]
    fn connection_state(&self) -> u32 {
        self.0.lock().unwrap().active
    }
}
async fn changed(conn: &Connection, path: &str, interface: &str) {
    conn.emit_signal(
        None::<&str>,
        path,
        "org.freedesktop.DBus.Properties",
        "PropertiesChanged",
        &(interface, Properties::new(), Vec::<String>::new()),
    )
    .await
    .unwrap();
}
struct Service {
    connection: zbus::blocking::Connection,
    state: Shared,
}
impl Service {
    fn new(bus: &Bus) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let connection = zbus::blocking::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .serve_at(ROOT, Manager(state.clone()))
            .unwrap()
            .serve_at(DEV, Device(state.clone()))
            .unwrap()
            .serve_at(DEV, Wireless(state.clone()))
            .unwrap()
            .serve_at(ACCESS, AccessPoint(state.clone(), false))
            .unwrap()
            .serve_at(HIDDEN, AccessPoint(state.clone(), true))
            .unwrap()
            .serve_at(SETTINGS, Profiles(state.clone()))
            .unwrap()
            .serve_at(SAVED, Profile(state.clone()))
            .unwrap()
            .serve_at(LIVE, Active(state.clone()))
            .unwrap()
            .name(NM)
            .unwrap()
            .build()
            .unwrap();
        Self { connection, state }
    }
    fn changed(&self, path: &str, interface: &str) {
        futures_lite::future::block_on(changed(self.connection.inner(), path, interface));
    }
    fn connected(&self) {
        {
            let mut state = self.state.lock().unwrap();
            state.active = 2;
            state.state = 100;
        }
        self.changed(ROOT, NM);
    }
    fn remove_reuse(&self) {
        // The path is already reused when the client reads. The removal signal
        // must still retire the device and the dependent access-point handles.
        self.connection
            .emit_signal(None::<&str>, ROOT, NM, "DeviceRemoved", &(path(DEV),))
            .unwrap();
        self.connection
            .emit_signal(None::<&str>, ROOT, NM, "DeviceAdded", &(path(DEV),))
            .unwrap();
    }
}
#[derive(Default)]
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
    adapter: Arc<NetworkManager>,
    stop: Arc<Stop>,
    latest: Arc<Mutex<Option<Result<NetworkSnapshot>>>>,
    worker: Option<thread::JoinHandle<()>>,
    credential_worker: Option<thread::JoinHandle<()>>,
    errors: Arc<AtomicU64>,
}
impl Watch {
    fn new(bus: &Bus, timeout: Duration) -> Self {
        Self::configured(bus, timeout, false)
    }
    fn configured(bus: &Bus, timeout: Duration, secret_agent: bool) -> Self {
        let adapter = Arc::new(NetworkManager {
            timeout,
            secret_agent,
            ..NetworkManager::with_address(bus.address.clone())
        });
        let stop = Arc::new(Stop::default());
        let credential_worker = if secret_agent {
            let (adapter, stop) = (adapter.clone(), stop.clone());
            Some(thread::spawn(move || {
                adapter.watch_credentials(&mut |_| {}, &*stop)
            }))
        } else {
            None
        };
        let latest = Arc::new(Mutex::new(None));
        let errors = Arc::new(AtomicU64::new(0));
        let (a, s, l, e) = (
            adapter.clone(),
            stop.clone(),
            latest.clone(),
            errors.clone(),
        );
        let worker = thread::spawn(move || {
            a.watch(
                &mut |v| {
                    if v.is_err() {
                        e.fetch_add(1, Ordering::Relaxed);
                    }
                    *l.lock().unwrap() = Some(v);
                },
                &*s,
            )
        });
        Self {
            adapter,
            stop,
            latest,
            worker: Some(worker),
            credential_worker,
            errors,
        }
    }
    fn until(&self, predicate: impl Fn(&NetworkSnapshot) -> bool) -> NetworkSnapshot {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(Ok(value)) = &*self.latest.lock().unwrap()
                && predicate(value)
            {
                return value.clone();
            }
            assert!(
                Instant::now() < deadline,
                "Network observation did not satisfy the expected predicate"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn ready(&self) -> NetworkSnapshot {
        self.until(|_| true)
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.0.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
        if let Some(worker) = self.credential_worker.take() {
            worker.join().unwrap();
        }
    }
}

#[test]
fn real_dbus_observation_controls_raw_ssids_hidden_join_and_service_acknowledgement() {
    let bus = Bus::new();
    let service = Service::new(&bus);
    let watch = Watch::new(&bus, Duration::from_secs(3));
    let initial = watch.ready();
    assert_eq!(initial.devices[0].scan_generation, None);
    assert!(initial.access_points.iter().any(|v| v.ssid.is_none()));
    let visible = initial
        .access_points
        .iter()
        .find(|v| v.ssid.is_some())
        .unwrap();
    assert_eq!(
        visible.ssid.as_ref().unwrap().bytes(),
        &[b'a', b';', 0xff, b'\n']
    );
    let queries = service.state.lock().unwrap().query_count;
    thread::sleep(Duration::from_millis(450));
    assert_eq!(
        service.state.lock().unwrap().query_count,
        queries,
        "Idle cancellation wakes must not query NetworkManager"
    );
    service.state.lock().unwrap().strength = 42;
    service.changed(ACCESS, AP);
    watch.until(|v| {
        v.access_point(visible.id)
            .is_some_and(|ap| ap.strength.percent() == 42)
    });
    watch
        .adapter
        .control(NetworkCommand::Scan(initial.devices[0].id))
        .unwrap();
    assert_eq!(
        watch.ready().devices[0].scan_generation,
        None,
        "Scan acceptance is not completion"
    );
    service.state.lock().unwrap().last_scan = 123;
    service.changed(DEV, WIFI);
    watch.until(|v| v.devices[0].scan_generation == Some(123));
    watch
        .adapter
        .control(NetworkCommand::SetWifiEnabled(false))
        .unwrap();
    watch.until(|v| !v.wifi_enabled);
    watch
        .adapter
        .control(NetworkCommand::SetWifiEnabled(true))
        .unwrap();
    watch.until(|v| v.wifi_enabled);
    watch
        .adapter
        .control(NetworkCommand::Activate {
            profile: initial.profiles[0].id,
            device: Some(initial.devices[0].id),
        })
        .unwrap();
    let connecting = watch.until(|v| {
        v.connections
            .first()
            .is_some_and(|v| v.state == NetworkConnectionState::Connecting)
    });
    assert_ne!(connecting.devices[0].state, NetworkDeviceState::Connected);
    // Device state and the manager's primary connection are separate native
    // observations. Establish the intermediate state instead of assuming the
    // first Connected snapshot is already the manager's final selection.
    service.state.lock().unwrap().state = 100;
    service.changed(DEV, DEVICE);
    let device_connected = watch.until(|v| v.devices[0].state == NetworkDeviceState::Connected);
    assert_eq!(device_connected.primary_connection, None);
    assert_eq!(
        device_connected.connections[0].state,
        NetworkConnectionState::Connecting
    );
    service.connected();
    let connected = watch.until(|v| {
        v.devices[0].state == NetworkDeviceState::Connected && v.primary_connection.is_some()
    });
    assert_eq!(
        connected.primary_connection,
        Some(connected.connections[0].id)
    );
    watch
        .adapter
        .control(NetworkCommand::Disconnect(connected.connections[0].id))
        .unwrap();
    watch.until(|v| v.connections.is_empty());
    watch
        .adapter
        .control(NetworkCommand::Join {
            target: NetworkJoinTarget::AccessPoint(visible.id),
            credentials: WifiCredentials::new(
                WifiSecurity::WpaPersonal,
                Some(Secret::new("test-password".into())),
            )
            .unwrap(),
            remember: true,
        })
        .unwrap();
    {
        let s = service.state.lock().unwrap();
        let j = &s.joins[0];
        assert_eq!(j.ssid, [b'a', b';', 0xff, b'\n']);
        assert!(j.secret_matches);
        assert_eq!(j.key, "wpa-psk");
        assert!(!j.hidden);
        assert_eq!(j.persist, "disk");
    }
    watch
        .adapter
        .control(NetworkCommand::Join {
            target: NetworkJoinTarget::Hidden {
                device: initial.devices[0].id,
                ssid: NetworkSsid::new(b"Hidden".to_vec()).unwrap(),
            },
            credentials: WifiCredentials::new(WifiSecurity::Open, None).unwrap(),
            remember: false,
        })
        .unwrap();
    {
        let s = service.state.lock().unwrap();
        let j = &s.joins[1];
        assert_eq!(j.ssid, b"Hidden");
        assert!(j.hidden);
        assert!(j.key.is_empty());
        assert_eq!(j.persist, "volatile");
    }
    watch
        .adapter
        .control(NetworkCommand::Forget(initial.profiles[0].id))
        .unwrap();
    watch.until(|v| v.profiles.is_empty() && v.connections.is_empty());
    assert!(!service.state.lock().unwrap().saved);
}
#[test]
fn removal_reuse_and_owner_replacement_retire_handles_including_dependants() {
    let bus = Bus::new();
    let service = Service::new(&bus);
    let watch = Watch::new(&bus, Duration::from_secs(3));
    let initial = watch.ready();
    service.remove_reuse();
    let replaced = watch.until(|v| v.devices[0].id != initial.devices[0].id);
    assert_ne!(replaced.access_points[0].id, initial.access_points[0].id);
    assert!(
        watch
            .adapter
            .control(NetworkCommand::Scan(initial.devices[0].id))
            .is_err()
    );
    assert!(
        watch
            .adapter
            .control(NetworkCommand::Join {
                target: NetworkJoinTarget::AccessPoint(initial.access_points[0].id),
                credentials: WifiCredentials::new(
                    WifiSecurity::WpaPersonal,
                    Some(Secret::new("test-password".into()))
                )
                .unwrap(),
                remember: false
            })
            .is_err()
    );
    assert_eq!(service.state.lock().unwrap().scans, 0);
    // The old unique owner remains alive and callable after releasing the name.
    // Clients must reconnect rather than keeping that otherwise valid endpoint.
    service.connection.release_name(NM).unwrap();
    let replacement = Service::new(&bus);
    let fresh = watch.until(|v| v.devices[0].id != replaced.devices[0].id);
    assert!(watch.errors.load(Ordering::Relaxed) > 0);
    assert!(
        watch
            .adapter
            .control(NetworkCommand::Scan(replaced.devices[0].id))
            .is_err()
    );
    watch
        .adapter
        .control(NetworkCommand::Scan(fresh.devices[0].id))
        .unwrap();
    assert_eq!(service.state.lock().unwrap().scans, 0);
    assert_eq!(replacement.state.lock().unwrap().scans, 1);
}
#[test]
fn signal_burst_is_drained_during_commands_and_errors_do_not_echo_credentials() {
    let bus = Bus::new();
    let service = Service::new(&bus);
    let watch = Watch::new(&bus, Duration::from_secs(3));
    let ready = watch.ready();
    service.state.lock().unwrap().flood = 512;
    watch
        .adapter
        .control(NetworkCommand::Scan(ready.devices[0].id))
        .unwrap();
    assert_eq!(service.state.lock().unwrap().scans, 1);
    service.state.lock().unwrap().scan_denied = true;
    let error = watch
        .adapter
        .control(NetworkCommand::Scan(ready.devices[0].id))
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Network permission denied");
    assert!(!error.contains("test-password"));
    {
        let mut state = service.state.lock().unwrap();
        state.scan_denied = false;
        state.scan_unknown = true;
    }
    let error = watch
        .adapter
        .control(NetworkCommand::Scan(ready.devices[0].id))
        .unwrap_err();
    assert!(matches!(error, DomainError::Failed(_)));
    service.state.lock().unwrap().scan_unknown = false;
    watch
        .adapter
        .control(NetworkCommand::Scan(ready.devices[0].id))
        .unwrap();
    assert_eq!(watch.errors.load(Ordering::Relaxed), 0);
    let mut errors = vec![];
    watch.adapter.watch(&mut |v| errors.push(v), &*watch.stop);
    assert!(matches!(
        errors.as_slice(),
        [Err(DomainError::Unavailable(_))]
    ));
}
#[test]
fn absolute_deadline_cancellation_and_catalog_bounds_are_observable() {
    let bus = Bus::new();
    let service = Service::new(&bus);
    let watch = Watch::new(&bus, Duration::from_millis(400));
    let initial = watch.ready();
    service.state.lock().unwrap().scan_delay = Duration::from_secs(2);
    let start = Instant::now();
    assert!(
        watch
            .adapter
            .control(NetworkCommand::Scan(initial.devices[0].id))
            .is_err()
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    let fresh = watch.until(|v| v.devices[0].id != initial.devices[0].id);
    assert!(watch.errors.load(Ordering::Relaxed) > 0);
    assert!(
        watch
            .adapter
            .control(NetworkCommand::Scan(initial.devices[0].id))
            .is_err()
    );
    assert_ne!(fresh.devices[0].id, initial.devices[0].id);
    service.state.lock().unwrap().oversized = true;
    service.changed(DEV, WIFI);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !matches!(&*watch.latest.lock().unwrap(), Some(Err(_))) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    let start = Instant::now();
    drop(watch);
    assert!(start.elapsed() < Duration::from_millis(500));
}
#[test]
fn disappearing_query_objects_retire_only_affected_handles_and_settle_before_publication() {
    for error in [
        zbus::fdo::Error::UnknownObject("Removed during query".into()),
        zbus::fdo::Error::UnknownMethod("Removed during GDBus query".into()),
    ] {
        let bus = Bus::new();
        let service = Service::new(&bus);
        let watch = Watch::new(&bus, Duration::from_secs(3));
        let initial = watch.ready();
        service.state.lock().unwrap().missing_read = Some(error);
        service.changed(SAVED, PROFILE);
        let next = watch.until(|v| v.profiles[0].id != initial.profiles[0].id);
        assert_eq!(next.devices[0].id, initial.devices[0].id);
        assert_eq!(next.access_points[0].id, initial.access_points[0].id);
        assert_eq!(watch.errors.load(Ordering::Relaxed), 0);
        assert!(
            watch
                .adapter
                .control(NetworkCommand::Forget(initial.profiles[0].id))
                .is_err()
        );
        assert!(service.state.lock().unwrap().saved);
    }
}

#[test]
fn command_admission_is_bounded_and_stale_kind_and_security_intent_fail() {
    let (send, receive) = async_channel::bounded(REQUEST_LIMIT);
    for _ in 0..REQUEST_LIMIT {
        let (reply, _) = mpsc::sync_channel(1);
        send.try_send(Request {
            command: NetworkCommand::SetWifiEnabled(true),
            reply,
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + OPERATION_TIMEOUT,
        })
        .unwrap_or_else(|_| panic!("queue filled before its bound"));
    }
    let adapter = NetworkManager::default();
    *adapter.online.lock().unwrap() = Some(send);
    let start = Instant::now();
    assert!(
        adapter
            .control(NetworkCommand::SetWifiEnabled(false))
            .is_err()
    );
    assert!(start.elapsed() < Duration::from_millis(100));
    assert_eq!(receive.len(), REQUEST_LIMIT);
    let bus = Bus::new();
    let service = Service::new(&bus);
    let watch = Watch::new(&bus, Duration::from_secs(3));
    let ready = watch.ready();
    assert!(
        watch
            .adapter
            .control(NetworkCommand::Disconnect(ready.devices[0].id))
            .is_err()
    );
    assert!(
        watch
            .adapter
            .control(NetworkCommand::Join {
                target: NetworkJoinTarget::AccessPoint(ready.access_points[0].id),
                credentials: WifiCredentials::new(WifiSecurity::Open, None).unwrap(),
                remember: true
            })
            .is_err()
    );
    assert!(service.state.lock().unwrap().joins.is_empty());
    for (flags, wpa, rsn, expected) in [
        (0, 0, 0, WifiSecurity::Open),
        (1, 0, 0, WifiSecurity::Wep),
        (1, 0x100, 0x400, WifiSecurity::WpaPersonal),
        (1, 0, 0x400, WifiSecurity::Wpa3Personal),
        (0, 0, 0x800, WifiSecurity::EnhancedOpen),
        (1, 0, 0x200, WifiSecurity::Enterprise),
        (1, 0, 0x8000, WifiSecurity::Unsupported),
    ] {
        assert_eq!(snapshot::security(flags, wpa, rsn), expected);
    }
}

fn secret_settings() -> Settings {
    Settings::from([
        (
            "connection".into(),
            Properties::from([
                ("uuid".into(), value("test-stable-profile")),
                ("type".into(), value("802-11-wireless")),
            ]),
        ),
        (
            "802-11-wireless".into(),
            Properties::from([("ssid".into(), value(vec![b'a', b';', 0xff, b'\n']))]),
        ),
        (
            "802-11-wireless-security".into(),
            Properties::from([
                ("key-mgmt".into(), value("wpa-psk")),
                ("psk-flags".into(), value(0u32)),
            ]),
        ),
    ])
}
#[derive(Clone)]
struct AgentManager(Shared);
#[zbus::interface(name = "org.freedesktop.NetworkManager.AgentManager")]
impl AgentManager {
    fn register_with_capabilities(
        &self,
        identifier: &str,
        capabilities: u32,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) {
        assert_eq!(identifier, "org.lucent.Network");
        assert_eq!(capabilities, 0);
        self.0.lock().unwrap().credential_agent = Some(header.sender().unwrap().to_string());
    }
}
fn agent_service(service: &Service) {
    service
        .connection
        .object_server()
        .at(
            "/org/freedesktop/NetworkManager/AgentManager",
            AgentManager(service.state.clone()),
        )
        .unwrap();
}
fn current_request(adapter: &NetworkManager) -> NetworkCredentialRequest {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(request) = adapter.credentials.state_for_test() {
            return request;
        }
        assert!(
            Instant::now() < deadline,
            "Interactive secret request was not observed"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn secret_call(
    connection: zbus::blocking::Connection,
    target: String,
    settings: Settings,
    flags: u32,
) -> thread::JoinHandle<zbus::Result<zbus::Message>> {
    thread::spawn(move || {
        connection.call_method(
            Some(target.as_str()),
            agent::PATH,
            Some("org.freedesktop.NetworkManager.SecretAgent"),
            "GetSecrets",
            &(
                settings,
                path(SAVED),
                "802-11-wireless-security",
                Vec::<String>::new(),
                flags,
            ),
        )
    })
}
#[test]
fn network_secret_agent_answer_is_independent_of_pending_activation_and_keeps_saved_profile() {
    let bus = Bus::new();
    let service = Service::new(&bus);
    agent_service(&service);
    let watch = Watch::configured(&bus, Duration::from_secs(5), true);
    let snapshot = watch.ready();
    let profile = snapshot.profiles[0].id;
    let device = snapshot.devices[0].id;
    let adapter = watch.adapter.clone();
    let operation = thread::spawn(move || {
        adapter.control(NetworkCommand::Activate {
            profile,
            device: Some(device),
        })
    });
    let request = current_request(&watch.adapter);
    assert_eq!(request.profile.as_str(), "test-stable-profile");
    assert!(request.new_secret_required);
    assert!(
        !operation.is_finished(),
        "The fixture must actually hold native activation for human input"
    );
    watch
        .adapter
        .answer_credentials(
            request.id,
            Some(
                WifiCredentials::new(
                    WifiSecurity::WpaPersonal,
                    Some(Secret::new("test-password".into())),
                )
                .unwrap(),
            ),
        )
        .unwrap();
    operation.join().unwrap().unwrap();
    let result = watch.until(|s| s.devices[0].state == NetworkDeviceState::Connected);
    assert_eq!(result.profiles[0].id, profile);
    assert_eq!(result.profiles.len(), 1);
    let state = service.state.lock().unwrap();
    assert_eq!(state.activations, 1);
    assert_eq!(state.secret_replies, 1);
    assert!(state.joins.is_empty());
    drop(state);
    assert!(watch.adapter.answer_credentials(request.id, None).is_err());
}
#[test]
fn network_secret_agent_rejects_untrusted_senders_flags_hints_vaults_and_profile_changes() {
    let bus = Bus::new();
    let service = Service::new(&bus);
    agent_service(&service);
    let watch = Watch::configured(&bus, Duration::from_secs(3), true);
    watch.ready();
    let target = service
        .state
        .lock()
        .unwrap()
        .credential_agent
        .clone()
        .unwrap();
    let stranger = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .unwrap();
    let result = secret_call(stranger, target.clone(), secret_settings(), 3)
        .join()
        .unwrap();
    assert!(
        matches!(result, Err(zbus::Error::MethodError(name, _, _)) if name.as_str().ends_with("PermissionDenied"))
    );
    for flags in [0, 2, 8, 0x100] {
        let result = secret_call(
            service.connection.clone(),
            target.clone(),
            secret_settings(),
            flags,
        )
        .join()
        .unwrap();
        assert!(
            matches!(result, Err(zbus::Error::MethodError(name, _, _)) if name.as_str().ends_with("NoSecrets"))
        );
    }
    for (setting, key, replacement) in [
        ("connection", "uuid", value("replaced-profile")),
        ("802-11-wireless-security", "psk-flags", value(1u32)),
        ("802-11-wireless-security", "key-mgmt", value("wpa-eap")),
    ] {
        let mut settings = secret_settings();
        settings
            .get_mut(setting)
            .unwrap()
            .insert(key.into(), replacement);
        assert!(
            secret_call(service.connection.clone(), target.clone(), settings, 3)
                .join()
                .unwrap()
                .is_err()
        );
    }
    let result = service.connection.call_method(
        Some(target.as_str()),
        agent::PATH,
        Some("org.freedesktop.NetworkManager.SecretAgent"),
        "GetSecrets",
        &(
            secret_settings(),
            path(SAVED),
            "802-11-wireless-security",
            vec!["unsupported-hint"],
            3u32,
        ),
    );
    assert!(result.is_err());
    assert!(watch.adapter.credentials.state_for_test().is_none());
}
#[test]
fn network_secret_agent_cancel_profile_retirement_and_daemon_loss_retire_exact_reply_authority() {
    let bus = Bus::new();
    let service = Service::new(&bus);
    agent_service(&service);
    let watch = Watch::configured(&bus, Duration::from_secs(3), true);
    watch.ready();
    let target = service
        .state
        .lock()
        .unwrap()
        .credential_agent
        .clone()
        .unwrap();
    let pending = secret_call(
        service.connection.clone(),
        target.clone(),
        secret_settings(),
        3,
    );
    let request = current_request(&watch.adapter);
    service
        .connection
        .call_method(
            Some(target.as_str()),
            agent::PATH,
            Some("org.freedesktop.NetworkManager.SecretAgent"),
            "CancelGetSecrets",
            &(path(SAVED), "802-11-wireless-security"),
        )
        .unwrap();
    assert!(
        matches!(pending.join().unwrap(), Err(zbus::Error::MethodError(name, _, _)) if name.as_str().ends_with("UserCanceled"))
    );
    assert!(watch.adapter.answer_credentials(request.id, None).is_err());
    let pending = secret_call(
        service.connection.clone(),
        target.clone(),
        secret_settings(),
        3,
    );
    let request = current_request(&watch.adapter);
    service.state.lock().unwrap().saved = false;
    service.changed(SETTINGS, "org.freedesktop.NetworkManager.Settings");
    watch.until(|s| s.profiles.is_empty());
    assert!(pending.join().unwrap().is_err());
    assert!(watch.adapter.answer_credentials(request.id, None).is_err());
    service.state.lock().unwrap().saved = true;
    service.changed(SETTINGS, "org.freedesktop.NetworkManager.Settings");
    watch.until(|s| !s.profiles.is_empty());
    let pending = secret_call(service.connection.clone(), target, secret_settings(), 3);
    let request = current_request(&watch.adapter);
    service.connection.release_name(NM).unwrap();
    assert!(pending.join().unwrap().is_err());
    assert!(watch.adapter.answer_credentials(request.id, None).is_err());
}
