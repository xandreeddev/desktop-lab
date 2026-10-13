//! Registration authority belongs to the sender's current unique bus connection.
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use zbus::{Connection, message::Header};
pub(super) const PATH: &str = "/StatusNotifierWatcher";
pub(super) const INTERFACES: [&str; 2] = [
    "org.kde.StatusNotifierWatcher",
    "org.freedesktop.StatusNotifierWatcher",
];
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Target {
    pub owner: String,
    pub path: String,
}
impl Target {
    pub fn registration(&self) -> String {
        format!("{}{}", self.owner, self.path)
    }
}
#[derive(Default)]
pub(super) struct Registry {
    pub items: BTreeSet<Target>,
    pub hosts: BTreeMap<String, String>,
    pub online: bool,
}
pub(super) struct Shared {
    pub state: Mutex<Registry>,
    pub wake: async_channel::Sender<()>,
}
impl Shared {
    pub fn items(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .items
            .iter()
            .map(Target::registration)
            .collect()
    }
    pub fn has_host(&self) -> bool {
        !self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .hosts
            .is_empty()
    }
    async fn register(
        &self,
        service: &str,
        host: bool,
        header: &Header<'_>,
        connection: &Connection,
    ) -> zbus::fdo::Result<()> {
        let sender = header
            .sender()
            .ok_or_else(|| zbus::fdo::Error::AccessDenied("Missing registration sender".into()))?
            .to_string();
        let (owner, path) = if service.starts_with('/') && !host {
            zbus::zvariant::ObjectPath::try_from(service)
                .map_err(|_| zbus::fdo::Error::InvalidArgs("Invalid tray path".into()))?;
            (sender.clone(), service.to_string())
        } else {
            let name = zbus::names::BusName::try_from(service)
                .map_err(|_| zbus::fdo::Error::InvalidArgs("Invalid tray service".into()))?;
            let resolved = native::bounded(
                async {
                    zbus::fdo::DBusProxy::new(connection)
                        .await
                        .map_err(native::bus_error)?
                        .get_name_owner(name)
                        .await
                        .map(|n| n.to_string())
                        .map_err(|_| native::unavailable("Registration owner unavailable"))
                },
                Instant::now() + DEADLINE,
            )
            .await
            .map_err(|_| zbus::fdo::Error::Failed("Registration owner unavailable".into()))?;
            (resolved, "/StatusNotifierItem".into())
        };
        if owner != sender {
            return Err(zbus::fdo::Error::AccessDenied(
                "Register only items owned by this connection".into(),
            ));
        }
        let target = Target {
            owner: owner.clone(),
            path,
        };
        let changed = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if !state.online {
                return Err(zbus::fdo::Error::Disconnected("Tray is offline".into()));
            }
            if host {
                if state.hosts.len() >= 16 && !state.hosts.contains_key(service) {
                    return Err(zbus::fdo::Error::LimitsExceeded(
                        "Too many tray hosts".into(),
                    ));
                }
                state.hosts.insert(service.into(), owner).is_none()
            } else {
                if state.items.len() >= 64 && !state.items.contains(&target) {
                    return Err(zbus::fdo::Error::LimitsExceeded(
                        "Too many tray items".into(),
                    ));
                }
                state.items.insert(target.clone())
            }
        };
        if changed {
            // A single wake means re-read the full bounded registry; it coalesces.
            let _ = self.wake.try_send(());
            for interface in INTERFACES {
                if host {
                    connection
                        .emit_signal(
                            None::<&str>,
                            PATH,
                            interface,
                            "StatusNotifierHostRegistered",
                            &(),
                        )
                        .await
                } else {
                    connection
                        .emit_signal(
                            None::<&str>,
                            PATH,
                            interface,
                            "StatusNotifierItemRegistered",
                            &(target.registration(),),
                        )
                        .await
                }
                .map_err(|_| zbus::fdo::Error::Disconnected("Tray signal failed".into()))?;
            }
        }
        Ok(())
    }
}
pub(super) struct Kde(pub Arc<Shared>);
pub(super) struct Freedesktop(pub Arc<Shared>);

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Kde {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<()> {
        self.0.register(service, false, &header, connection).await
    }
    async fn register_status_notifier_host(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<()> {
        self.0.register(service, true, &header, connection).await
    }
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.0.items()
    }
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        self.0.has_host()
    }
    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::Result<()>;
}

#[zbus::interface(name = "org.freedesktop.StatusNotifierWatcher")]
impl Freedesktop {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<()> {
        self.0.register(service, false, &header, connection).await
    }
    async fn register_status_notifier_host(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<()> {
        self.0.register(service, true, &header, connection).await
    }
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.0.items()
    }
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        self.0.has_host()
    }
    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::Result<()>;
}
