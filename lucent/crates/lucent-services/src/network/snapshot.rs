use super::{native::*, *};
use std::collections::{BTreeMap, BTreeSet};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const DEVICE_LIMIT: usize = 64;
const PROFILE_LIMIT: usize = 512;
const AP_LIMIT: usize = 512;
const CONNECTION_LIMIT: usize = 64;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Device,
    Profile,
    AccessPoint,
    Connection,
}
#[derive(Default)]
pub(super) struct Objects {
    entries: BTreeMap<(Kind, String), NetworkObjectId>,
    access_point_devices: BTreeMap<String, String>,
    connection_devices: BTreeMap<String, BTreeSet<String>>,
}
impl Objects {
    pub fn invalidate(&mut self, removed: &BTreeSet<String>) {
        self.entries.retain(|(kind, path), _| {
            !removed.contains(path)
                && match kind {
                    Kind::AccessPoint => self
                        .access_point_devices
                        .get(path)
                        .is_none_or(|device| !removed.contains(device)),
                    Kind::Connection => self
                        .connection_devices
                        .get(path)
                        .is_none_or(|devices| devices.is_disjoint(removed)),
                    _ => true,
                }
        });
    }
    pub fn resolve(&self, id: NetworkObjectId, kind: Kind) -> Result<OwnedObjectPath> {
        self.entries
            .iter()
            .find(|((k, _), value)| *k == kind && **value == id)
            .map(|((_, path), _)| {
                OwnedObjectPath::try_from(path.as_str()).expect("validated backend object path")
            })
            .ok_or_else(|| {
                DomainError::Invalid("Network item is stale; choose a current item".into())
            })
    }
    fn get(&self, kind: Kind, path: &OwnedObjectPath) -> Option<NetworkObjectId> {
        self.entries.get(&(kind, path.to_string())).copied()
    }
    pub fn replace(
        &mut self,
        raw: Raw,
        allocate: &mut dyn FnMut() -> Result<NetworkObjectId>,
    ) -> Result<NetworkSnapshot> {
        let keys: BTreeSet<_> = raw
            .devices
            .iter()
            .map(|v| (Kind::Device, v.path.to_string()))
            .chain(
                raw.profiles
                    .iter()
                    .map(|(path, _)| (Kind::Profile, path.to_string())),
            )
            .chain(
                raw.access_points
                    .iter()
                    .map(|(path, _, _)| (Kind::AccessPoint, path.to_string())),
            )
            .chain(
                raw.connections
                    .iter()
                    .map(|(path, _)| (Kind::Connection, path.to_string())),
            )
            .collect();
        if keys.len()
            != raw.devices.len()
                + raw.profiles.len()
                + raw.access_points.len()
                + raw.connections.len()
        {
            return Err(protocol());
        }
        self.access_point_devices = raw
            .access_points
            .iter()
            .map(|(ap, device, _)| (ap.to_string(), device.to_string()))
            .collect();
        self.connection_devices = raw
            .connections
            .iter()
            .map(|(path, properties)| {
                let devices: Vec<OwnedObjectPath> = property(properties, "Devices")?;
                check_count(devices.len(), DEVICE_LIMIT)?;
                Ok((
                    path.to_string(),
                    devices.iter().map(ToString::to_string).collect(),
                ))
            })
            .collect::<Result<_>>()?;
        self.entries.retain(|key, _| keys.contains(key));
        for key in keys {
            if let std::collections::btree_map::Entry::Vacant(entry) = self.entries.entry(key) {
                entry.insert(allocate()?);
            }
        }
        let mut devices = vec![];
        for device in raw.devices {
            let state = property::<u32>(&device.properties, "State")?;
            let reason = property::<(u32, u32)>(&device.properties, "StateReason")?.1;
            let (active_access_point, scan_generation) = if let Some(wifi) = device.wifi {
                let scan: i64 = property(&wifi, "LastScan")?;
                (
                    self.get(Kind::AccessPoint, &property(&wifi, "ActiveAccessPoint")?),
                    u64::try_from(scan).ok(),
                )
            } else {
                (None, None)
            };
            devices.push(NetworkDevice {
                id: self.get(Kind::Device, &device.path).unwrap(),
                interface: label(property(&device.properties, "Interface")?)?,
                kind: device_kind(property(&device.properties, "DeviceType")?),
                managed: property(&device.properties, "Managed")?,
                state: device_state(state),
                failure: failure(state, reason),
                active_connection: self.get(
                    Kind::Connection,
                    &property(&device.properties, "ActiveConnection")?,
                ),
                active_access_point,
                scan_generation,
            });
        }
        let mut profiles = vec![];
        for (path, settings) in raw.profiles {
            let connection = settings.get("connection").ok_or_else(protocol)?;
            let kind = profile_kind(&property::<String>(connection, "type")?);
            let ssid = if kind == NetworkKind::Wifi {
                Some(NetworkSsid::new(property::<Vec<u8>>(
                    settings.get("802-11-wireless").ok_or_else(protocol)?,
                    "ssid",
                )?)?)
            } else {
                None
            };
            profiles.push(NetworkProfile {
                id: self.get(Kind::Profile, &path).unwrap(),
                key: NetworkProfileKey::new(property::<String>(connection, "uuid")?)?,
                name: label(property(connection, "id")?)?,
                kind,
                ssid,
            });
        }
        let mut access_points = vec![];
        for (path, device, properties) in raw.access_points {
            let ssid = property::<Vec<u8>>(&properties, "Ssid")?;
            access_points.push(NetworkAccessPoint {
                id: self.get(Kind::AccessPoint, &path).unwrap(),
                device: self.get(Kind::Device, &device).ok_or_else(protocol)?,
                ssid: if ssid.is_empty() {
                    None
                } else {
                    Some(NetworkSsid::new(ssid)?)
                },
                strength: NetworkStrength::new(property(&properties, "Strength")?)?,
                security: security(
                    property(&properties, "Flags")?,
                    property(&properties, "WpaFlags")?,
                    property(&properties, "RsnFlags")?,
                ),
            });
        }
        let mut connections = vec![];
        for (path, properties) in raw.connections {
            let devices: Vec<OwnedObjectPath> = property(&properties, "Devices")?;
            connections.push(NetworkConnection {
                id: self.get(Kind::Connection, &path).unwrap(),
                profile: self.get(Kind::Profile, &property(&properties, "Connection")?),
                name: label(property(&properties, "Id")?)?,
                kind: profile_kind(&property::<String>(&properties, "Type")?),
                devices: devices
                    .iter()
                    .filter_map(|p| self.get(Kind::Device, p))
                    .collect(),
                state: match property::<u32>(&properties, "State")? {
                    1 => NetworkConnectionState::Connecting,
                    2 => NetworkConnectionState::Connected,
                    3 => NetworkConnectionState::Disconnecting,
                    4 => NetworkConnectionState::Disconnected,
                    _ => NetworkConnectionState::Unknown,
                },
            });
        }
        devices.sort_by_key(|v| v.id);
        profiles.sort_by_key(|v| v.id);
        access_points.sort_by_key(|v| v.id);
        connections.sort_by_key(|v| v.id);
        Ok(NetworkSnapshot {
            state: match property::<u32>(&raw.root, "State")? {
                10 => NetworkState::Asleep,
                20 => NetworkState::Disconnected,
                30 => NetworkState::Disconnecting,
                40 => NetworkState::Connecting,
                50 => NetworkState::Local,
                60 => NetworkState::Site,
                70 => NetworkState::Global,
                _ => NetworkState::Unknown,
            },
            connectivity: match property::<u32>(&raw.root, "Connectivity")? {
                1 => NetworkConnectivity::None,
                2 => NetworkConnectivity::Portal,
                3 => NetworkConnectivity::Limited,
                4 => NetworkConnectivity::Full,
                _ => NetworkConnectivity::Unknown,
            },
            wifi_enabled: property(&raw.root, "WirelessEnabled")?,
            wifi_hardware_enabled: property(&raw.root, "WirelessHardwareEnabled")?,
            primary_connection: self
                .get(Kind::Connection, &property(&raw.root, "PrimaryConnection")?),
            devices,
            profiles,
            access_points,
            connections,
        })
    }
}
pub(super) struct RawDevice {
    path: OwnedObjectPath,
    properties: Properties,
    wifi: Option<Properties>,
}
pub(super) struct Raw {
    root: Properties,
    devices: Vec<RawDevice>,
    profiles: Vec<(OwnedObjectPath, Settings)>,
    access_points: Vec<(OwnedObjectPath, OwnedObjectPath, Properties)>,
    connections: Vec<(OwnedObjectPath, Properties)>,
}
pub(super) async fn read(transport: &Transport) -> Result<Raw> {
    let root = transport.properties(ROOT, NM).await?;
    let device_paths: Vec<OwnedObjectPath> = property(&root, "Devices")?;
    let connection_paths: Vec<OwnedObjectPath> = property(&root, "ActiveConnections")?;
    let profile_paths: Vec<OwnedObjectPath> = transport
        .call(
            SETTINGS,
            "org.freedesktop.NetworkManager.Settings",
            "ListConnections",
            &(),
        )
        .await?
        .body()
        .deserialize()
        .map_err(|_| protocol())?;
    check_count(device_paths.len(), DEVICE_LIMIT)?;
    check_count(connection_paths.len(), CONNECTION_LIMIT)?;
    check_count(profile_paths.len(), PROFILE_LIMIT)?;
    let mut devices = vec![];
    let mut access_points = vec![];
    for path in device_paths {
        let properties = transport.properties(path.as_str(), DEVICE).await?;
        let wifi = if device_kind(property(&properties, "DeviceType")?) == NetworkKind::Wifi {
            let wifi = transport.properties(path.as_str(), WIFI).await?;
            let aps: Vec<OwnedObjectPath> = transport
                .call(path.as_str(), WIFI, "GetAllAccessPoints", &())
                .await?
                .body()
                .deserialize()
                .map_err(|_| protocol())?;
            check_count(access_points.len() + aps.len(), AP_LIMIT)?;
            for ap in aps {
                let data = transport.properties(ap.as_str(), AP).await?;
                access_points.push((ap, path.clone(), data));
            }
            Some(wifi)
        } else {
            None
        };
        devices.push(RawDevice {
            path,
            properties,
            wifi,
        });
    }
    let mut profiles = vec![];
    for path in profile_paths {
        // GetSettings excludes secrets. Never call GetSecrets for observation.
        let settings = transport
            .call(path.as_str(), PROFILE, "GetSettings", &())
            .await?
            .body()
            .deserialize()
            .map_err(|_| protocol())?;
        profiles.push((path, settings));
    }
    let mut connections = vec![];
    for path in connection_paths {
        let data = transport.properties(path.as_str(), ACTIVE).await?;
        connections.push((path, data));
    }
    Ok(Raw {
        root,
        devices,
        profiles,
        access_points,
        connections,
    })
}
fn check_count(count: usize, limit: usize) -> Result<()> {
    if count > limit {
        Err(unavailable(
            "Network catalog exceeded the observation bound",
        ))
    } else {
        Ok(())
    }
}
fn property<T: TryFrom<OwnedValue>>(data: &Properties, key: &str) -> Result<T> {
    T::try_from(
        data.get(key)
            .ok_or_else(protocol)?
            .try_clone()
            .map_err(|_| protocol())?,
    )
    .map_err(|_| protocol())
}
fn label(value: String) -> Result<String> {
    if value.len() > 1024 {
        return Err(protocol());
    }
    Ok(value
        .chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect())
}
fn device_kind(value: u32) -> NetworkKind {
    match value {
        1 => NetworkKind::Wired,
        2 => NetworkKind::Wifi,
        _ => NetworkKind::Other,
    }
}
fn profile_kind(value: &str) -> NetworkKind {
    match value {
        "802-3-ethernet" => NetworkKind::Wired,
        "802-11-wireless" => NetworkKind::Wifi,
        _ => NetworkKind::Other,
    }
}
fn device_state(value: u32) -> NetworkDeviceState {
    match value {
        10 => NetworkDeviceState::Unmanaged,
        20 => NetworkDeviceState::Unavailable,
        30 => NetworkDeviceState::Disconnected,
        40 | 50 | 70 | 80 | 90 => NetworkDeviceState::Connecting,
        60 => NetworkDeviceState::NeedsAuthentication,
        100 => NetworkDeviceState::Connected,
        110 => NetworkDeviceState::Disconnecting,
        120 => NetworkDeviceState::Failed,
        _ => NetworkDeviceState::Unknown,
    }
}
fn failure(state: u32, reason: u32) -> NetworkFailure {
    if state != 120 && state != 60 {
        return NetworkFailure::None;
    }
    match reason {
        0 => NetworkFailure::None,
        7..=11 => NetworkFailure::Authentication,
        4 => NetworkFailure::Configuration,
        5 | 6 | 15 | 16 | 17 | 20 | 21 | 22 => NetworkFailure::Address,
        36 | 38 => NetworkFailure::Removed,
        _ => NetworkFailure::Unknown,
    }
}
pub(super) fn security(flags: u32, wpa: u32, rsn: u32) -> WifiSecurity {
    let auth = wpa | rsn;
    if auth & 0x100 != 0 {
        WifiSecurity::WpaPersonal
    } else if auth & 0x400 != 0 {
        WifiSecurity::Wpa3Personal
    } else if auth & (0x800 | 0x1000) != 0 {
        WifiSecurity::EnhancedOpen
    } else if auth & (0x200 | 0x2000) != 0 {
        WifiSecurity::Enterprise
    } else if auth != 0 {
        WifiSecurity::Unsupported
    } else if flags & 1 != 0 {
        WifiSecurity::Wep
    } else {
        WifiSecurity::Open
    }
}
