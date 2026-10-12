use super::{native::*, *};
use std::collections::{BTreeMap, BTreeSet};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Adapter,
    Device,
}
#[derive(Default)]
pub(super) struct Catalog {
    entries: BTreeMap<(Kind, String), BluetoothObjectId>,
    parents: BTreeMap<String, String>,
}
impl Catalog {
    pub fn invalidate(&mut self, removed: &BTreeSet<String>) {
        self.entries.retain(|(kind, path), _| {
            !removed.contains(path)
                && (*kind != Kind::Device
                    || self.parents.get(path).is_none_or(|p| !removed.contains(p)))
        });
    }
    pub fn resolve(&self, id: BluetoothObjectId, kind: Kind) -> Result<OwnedObjectPath> {
        self.entries
            .iter()
            .find(|((k, _), v)| *k == kind && **v == id)
            .map(|((_, p), _)| {
                OwnedObjectPath::try_from(p.as_str()).expect("validated object path")
            })
            .ok_or_else(|| {
                DomainError::Invalid("Bluetooth item is stale; choose a current item".into())
            })
    }
    pub fn replace(
        &mut self,
        raw: Managed,
        owned: &mut BTreeSet<String>,
        allocate: &mut dyn FnMut() -> Result<BluetoothObjectId>,
    ) -> Result<BluetoothSnapshot> {
        let mut adapters: Vec<_> = raw
            .iter()
            .filter_map(|(p, i)| i.get(ADAPTER).map(|v| (p, v)))
            .collect();
        let mut devices: Vec<_> = raw
            .iter()
            .filter_map(|(p, i)| i.get(DEVICE).map(|v| (p, v)))
            .collect();
        adapters.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
        devices.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
        if adapters.len() > 32 || devices.len() > 512 {
            return Err(unavailable(
                "Bluetooth catalog exceeded the observation bound",
            ));
        }
        let mut keys = BTreeSet::new();
        let mut parents = BTreeMap::new();
        for (path, _) in &adapters {
            keys.insert((Kind::Adapter, path.to_string()));
        }
        for (path, props) in &devices {
            let parent: OwnedObjectPath = property(props, "Adapter")?;
            // Removal can precede the dependent device's removal signal.
            if keys.contains(&(Kind::Adapter, parent.to_string())) {
                keys.insert((Kind::Device, path.to_string()));
                parents.insert(path.to_string(), parent.to_string());
            }
        }
        self.entries.retain(|k, _| keys.contains(k));
        for key in &keys {
            if !self.entries.contains_key(key) {
                self.entries.insert(key.clone(), allocate()?);
            }
        }
        self.parents = parents;
        owned.retain(|p| keys.contains(&(Kind::Adapter, p.clone())));
        let mut state = BluetoothSnapshot::default();
        for (path, props) in adapters {
            let address = address(props)?;
            let powered: bool = property(props, "Powered")?;
            // BlueZ's controller-stop path retires discovery sessions.
            if !powered {
                owned.remove(path.as_str());
            }
            state.adapters.push(BluetoothAdapter {
                id: self.entries[&(Kind::Adapter, path.to_string())],
                address,
                name: name(props, &address)?,
                powered,
                discoverable: property(props, "Discoverable")?,
                discovering: property(props, "Discovering")?,
                discovery_owned: owned.contains(path.as_str()),
            });
        }
        for (path, props) in devices {
            let Some(parent) = self.parents.get(path.as_str()) else {
                continue;
            };
            let address = address(props)?;
            let battery_percent = raw
                .get(path)
                .and_then(|i| i.get(BATTERY))
                .map(|p| property::<u8>(p, "Percentage"))
                .transpose()?;
            if battery_percent.is_some_and(|v| v > 100) {
                return Err(protocol());
            }
            state.devices.push(BluetoothDevice {
                id: self.entries[&(Kind::Device, path.to_string())],
                adapter: self.entries[&(Kind::Adapter, parent.clone())],
                address,
                name: name(props, &address)?,
                icon: optional::<String>(props, "Icon")?.map(label).transpose()?,
                paired: property(props, "Paired")?,
                bonded: optional(props, "Bonded")?,
                trusted: property(props, "Trusted")?,
                blocked: property(props, "Blocked")?,
                connected: property(props, "Connected")?,
                services_resolved: property(props, "ServicesResolved")?,
                rssi: optional(props, "RSSI")?,
                battery_percent,
            });
        }
        Ok(state)
    }
}
fn property<T: TryFrom<OwnedValue>>(props: &Properties, key: &str) -> Result<T> {
    T::try_from(
        props
            .get(key)
            .ok_or_else(protocol)?
            .try_clone()
            .map_err(|_| protocol())?,
    )
    .map_err(|_| protocol())
}
fn optional<T: TryFrom<OwnedValue>>(props: &Properties, key: &str) -> Result<Option<T>> {
    props
        .get(key)
        .map(|v| T::try_from(v.try_clone().map_err(|_| protocol())?).map_err(|_| protocol()))
        .transpose()
}
fn address(props: &Properties) -> Result<BluetoothAddress> {
    BluetoothAddress::parse(&property::<String>(props, "Address")?).map_err(|_| protocol())
}
fn name(props: &Properties, address: &BluetoothAddress) -> Result<String> {
    let alias = optional::<String>(props, "Alias")?;
    let name = optional::<String>(props, "Name")?;
    label(
        alias
            .filter(|v| !v.is_empty())
            .or(name.filter(|v| !v.is_empty()))
            .unwrap_or_else(|| address.to_string()),
    )
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
