use super::*;
use futures_lite::{StreamExt, future};
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    future::Future,
    pin::pin,
};
use zbus::{
    Connection, MatchRule, Message, MessageStream,
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue},
};

pub(super) const NM: &str = "org.freedesktop.NetworkManager";
pub(super) const ROOT: &str = "/org/freedesktop/NetworkManager";
pub(super) const SETTINGS: &str = "/org/freedesktop/NetworkManager/Settings";
pub(super) const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
pub(super) const WIFI: &str = "org.freedesktop.NetworkManager.Device.Wireless";
pub(super) const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
pub(super) const PROFILE: &str = "org.freedesktop.NetworkManager.Settings.Connection";
pub(super) const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub(super) type Properties = HashMap<String, OwnedValue>;
pub(super) type Settings = HashMap<String, Properties>;
const SIGNAL_LIMIT: usize = 128;
const REMOVAL_LIMIT: usize = 1152;
pub(super) fn unavailable(message: &str) -> DomainError {
    DomainError::Unavailable(message.into())
}
pub(super) fn protocol() -> DomainError {
    DomainError::Failed("NetworkManager returned an invalid response".into())
}
// Never expose method error bodies: a daemon can echo caller-provided settings.
pub(super) fn bus_error(error: zbus::Error) -> DomainError {
    match error {
        zbus::Error::MethodError(name, _, _)
            if name.as_str().ends_with("PermissionDenied")
                || name.as_str().ends_with("NotAuthorized")
                || name.as_str().ends_with("AccessDenied") =>
        {
            DomainError::Failed("Network permission denied".into())
        }
        zbus::Error::MethodError(name, _, _) if name.as_str().ends_with("UnknownObject") => {
            DomainError::Invalid("Network object disappeared; choose a current item".into())
        }
        zbus::Error::MethodError(_, _, _) => {
            DomainError::Failed("NetworkManager rejected the request".into())
        }
        _ => unavailable("NetworkManager transport is unavailable"),
    }
}
pub(super) async fn bounded<T>(
    future: impl Future<Output = Result<T>>,
    deadline: Instant,
) -> Result<T> {
    future::or(future, async {
        async_io::Timer::at(deadline).await;
        Err(unavailable("Network operation timed out"))
    })
    .await
}
pub(super) struct Transport {
    pub connection: Connection,
    pub owner: String,
    missing: RefCell<BTreeSet<String>>,
}
impl Transport {
    pub fn take_missing(&self) -> BTreeSet<String> {
        self.missing.take()
    }
    pub async fn call<B>(
        &self,
        path: &str,
        interface: &str,
        member: &str,
        body: &B,
    ) -> Result<Message>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        let result = self
            .connection
            .call_method(
                Some(self.owner.as_str()),
                path,
                Some(interface),
                member,
                body,
            )
            .await;
        if result
            .as_ref()
            .is_err_and(|error| removed_object(error, path, interface, member))
        {
            self.missing.borrow_mut().insert(path.to_owned());
        }
        result.map_err(bus_error)
    }
    pub async fn properties(&self, path: &str, interface: &str) -> Result<Properties> {
        self.call(
            path,
            "org.freedesktop.DBus.Properties",
            "GetAll",
            &(interface,),
        )
        .await?
        .body()
        .deserialize()
        .map_err(|_| protocol())
    }
}
fn removed_object(error: &zbus::Error, path: &str, interface: &str, member: &str) -> bool {
    let zbus::Error::MethodError(name, _, _) = error else {
        return false;
    };
    if name.as_str().ends_with("UnknownObject") {
        return true;
    }
    // GDBus returns UnknownMethod for GetAll on an object removed after its
    // parent catalog was read. Only known reads of dynamic objects may retry;
    // unsupported controls or root methods must retain their actual failure.
    name.as_str() == "org.freedesktop.DBus.Error.UnknownMethod"
        && path.starts_with("/org/freedesktop/NetworkManager/")
        && path != SETTINGS
        && matches!(
            (interface, member),
            ("org.freedesktop.DBus.Properties", "GetAll")
                | (PROFILE, "GetSettings")
                | (WIFI, "GetAllAccessPoints")
        )
}
pub(super) async fn connect(
    address: Option<&str>,
    timeout: Duration,
    secret_agent: bool,
) -> Result<(Transport, Signals)> {
    let deadline = Instant::now() + timeout;
    bounded(
        async {
            let builder = match address {
                Some(address) => zbus::connection::Builder::address(address),
                None => zbus::connection::Builder::system(),
            }
            .map_err(bus_error)?;
            let connection = builder
                // Each ordinary call still has its owner's absolute deadline.
                // A real SecretAgent exchange may legitimately wait for a user.
                .method_timeout(if secret_agent {
                    timeout + timeout + agent::PROMPT_TIMEOUT
                } else {
                    timeout
                })
                .build()
                .await
                .map_err(bus_error)?;
            let owner_rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender("org.freedesktop.DBus")
                .map_err(bus_error)?
                .interface("org.freedesktop.DBus")
                .map_err(bus_error)?
                .member("NameOwnerChanged")
                .map_err(bus_error)?
                .arg(0, NM)
                .map_err(bus_error)?
                .build();
            let owners = MessageStream::for_match_rule(owner_rule, &connection, Some(8))
                .await
                .map_err(bus_error)?;
            let owner = service_owner(&connection).await?;
            let rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender(owner.as_str())
                .map_err(bus_error)?
                .path_namespace(ROOT)
                .map_err(bus_error)?
                .build();
            let events = MessageStream::for_match_rule(rule, &connection, Some(SIGNAL_LIMIT))
                .await
                .map_err(bus_error)?;
            if service_owner(&connection).await? != owner {
                return Err(unavailable("NetworkManager changed while connecting"));
            }
            Ok((
                Transport {
                    connection,
                    owner,
                    missing: RefCell::default(),
                },
                Signals {
                    events,
                    owners,
                    failed: false,
                },
            ))
        },
        deadline,
    )
    .await
}
async fn service_owner(connection: &Connection) -> Result<String> {
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetNameOwner",
            &(NM,),
        )
        .await
        .map_err(bus_error)?
        .body()
        .deserialize()
        .map_err(|_| protocol())
}
#[derive(Default)]
pub(super) struct Notices {
    pub credentials: Option<Arc<agent::Hub>>,
    pub dirty: bool,
    pub removed: BTreeSet<String>,
}
impl Notices {
    fn signal(&mut self, message: Message) -> Result<()> {
        self.dirty = true;
        let header = message.header();
        let member = header.member().map(|s| s.as_str()).unwrap_or("");
        let removed = match member {
            "DeviceRemoved" | "AccessPointRemoved" | "ConnectionRemoved" => Some(
                message
                    .body()
                    .deserialize::<OwnedObjectPath>()
                    .map_err(|_| protocol())?
                    .to_string(),
            ),
            "Removed" if header.interface().is_some_and(|i| i.as_str() == PROFILE) => {
                header.path().map(|p| p.to_string())
            }
            _ => None,
        };
        if let Some(path) = removed {
            if let Some(hub) = &self.credentials {
                hub.invalidate(&path);
            }
            if self.removed.len() >= REMOVAL_LIMIT {
                return Err(unavailable(
                    "Network change burst exceeded the observation bound",
                ));
            }
            self.removed.insert(path);
        }
        Ok(())
    }
}
pub(super) struct Signals {
    events: MessageStream,
    owners: MessageStream,
    pub failed: bool,
}
enum Pump<T> {
    Done(Result<T>),
    Event(Option<zbus::Result<Message>>),
    Owner,
    Tick,
}
impl Signals {
    pub async fn idle(
        &mut self,
        receive: &async_channel::Receiver<Request>,
        stop: &dyn StopSignal,
        notices: &mut Notices,
    ) -> Result<Option<Request>> {
        loop {
            if stop.cancelled() {
                return Err(unavailable("Network observation cancelled"));
            }
            let event = future::or(
                async {
                    self.owners.next().await;
                    Pump::Owner
                },
                future::or(
                    async {
                        Pump::Done(
                            receive
                                .recv()
                                .await
                                .map_err(|_| unavailable("Network command queue ended")),
                        )
                    },
                    future::or(async { Pump::Event(self.events.next().await) }, async {
                        async_io::Timer::after(Duration::from_millis(100)).await;
                        Pump::Tick
                    }),
                ),
            )
            .await;
            match event {
                Pump::Done(request) => return request.map(Some),
                Pump::Event(Some(Ok(message))) => {
                    notices.signal(message)?;
                    return Ok(None);
                }
                Pump::Event(_) | Pump::Owner => {
                    self.failed = true;
                    return Err(unavailable("NetworkManager disconnected or changed owner"));
                }
                Pump::Tick => (),
            }
        }
    }
    /// Drain signals while an operation is pending: a bounded stream must not
    /// block zbus's reader behind a burst of signals and starve its method reply.
    pub async fn pump<T>(
        &mut self,
        operation: impl Future<Output = Result<T>>,
        deadline: Option<Instant>,
        stop: &dyn StopSignal,
        notices: &mut Notices,
    ) -> Result<T> {
        self.pump_interactive(operation, deadline, stop, notices, &|| None)
            .await
    }
    pub async fn pump_interactive<T>(
        &mut self,
        operation: impl Future<Output = Result<T>>,
        deadline: Option<Instant>,
        stop: &dyn StopSignal,
        notices: &mut Notices,
        interactive: &dyn Fn() -> Option<Instant>,
    ) -> Result<T> {
        let mut operation = pin!(operation);
        loop {
            let deadline = interactive().or(deadline);
            if stop.cancelled() {
                return Err(unavailable("Network observation cancelled"));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                self.failed = true;
                return Err(unavailable(
                    "Network operation timed out; observe state before retrying",
                ));
            }
            let event = future::or(
                async {
                    self.owners.next().await;
                    Pump::Owner
                },
                future::or(
                    async { Pump::Done(operation.as_mut().await) },
                    future::or(async { Pump::Event(self.events.next().await) }, async {
                        async_io::Timer::after(Duration::from_millis(100)).await;
                        Pump::Tick
                    }),
                ),
            )
            .await;
            match event {
                Pump::Done(value) => return value,
                Pump::Event(Some(Ok(message))) => {
                    if let Err(error) = notices.signal(message) {
                        self.failed = true;
                        return Err(error);
                    }
                }
                Pump::Event(_) | Pump::Owner => {
                    self.failed = true;
                    return Err(unavailable("NetworkManager disconnected or changed owner"));
                }
                Pump::Tick => (),
            }
        }
    }
    pub async fn drain(&mut self, notices: &mut Notices) -> Result<()> {
        if future::poll_once(self.owners.next()).await.is_some() {
            self.failed = true;
            return Err(unavailable("NetworkManager changed owner"));
        }
        for _ in 0..SIGNAL_LIMIT {
            match future::poll_once(self.events.next()).await {
                Some(Some(Ok(message))) => notices.signal(message)?,
                Some(_) => {
                    self.failed = true;
                    return Err(unavailable("Network event stream ended"));
                }
                None => return Ok(()),
            }
        }
        // Further queued changes will trigger another refresh, not an unbounded
        // drain on this worker. All queued removals observed so far are retained.
        notices.dirty = true;
        Ok(())
    }
}
