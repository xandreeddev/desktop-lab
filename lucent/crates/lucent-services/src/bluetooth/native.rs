use super::*;
use futures_lite::{StreamExt, future};
use std::{
    collections::{BTreeSet, HashMap},
    future::Future,
    pin::pin,
};
use zbus::{
    Connection, MatchRule, Message, MessageStream,
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue},
};

pub(super) const SERVICE: &str = "org.bluez";
pub(super) const ADAPTER: &str = "org.bluez.Adapter1";
pub(super) const DEVICE: &str = "org.bluez.Device1";
pub(super) const BATTERY: &str = "org.bluez.Battery1";
pub(super) type Properties = HashMap<String, OwnedValue>;
pub(super) type Managed = HashMap<OwnedObjectPath, HashMap<String, Properties>>;
const SIGNAL_LIMIT: usize = 128;
const REMOVAL_LIMIT: usize = 544;
pub(super) fn unavailable(message: &str) -> DomainError {
    DomainError::Unavailable(message.into())
}
pub(super) fn protocol() -> DomainError {
    DomainError::Failed("BlueZ returned an invalid observation".into())
}
fn bus_error(error: zbus::Error) -> DomainError {
    match error {
        zbus::Error::MethodError(name, _, _) => match name.as_str() {
            "org.freedesktop.DBus.Error.NameHasNoOwner"
            | "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NoReply"
            | "org.freedesktop.DBus.Error.Timeout"
            | "org.freedesktop.DBus.Error.Disconnected" => {
                unavailable("BlueZ service is unavailable")
            }
            "org.bluez.Error.NotAuthorized" | "org.freedesktop.DBus.Error.AccessDenied" => {
                DomainError::Failed("Bluetooth permission denied".into())
            }
            "org.freedesktop.DBus.Error.UnknownObject" | "org.bluez.Error.DoesNotExist" => {
                DomainError::Invalid("Bluetooth item disappeared; choose a current item".into())
            }
            "org.bluez.Error.NotReady" => {
                DomainError::Failed("Bluetooth controller is not ready".into())
            }
            "org.bluez.Error.InProgress" => {
                DomainError::Failed("Bluetooth operation is already in progress".into())
            }
            "org.bluez.Error.AuthenticationCanceled" | "org.bluez.Error.Canceled" => {
                DomainError::Failed("Bluetooth authentication was cancelled".into())
            }
            "org.bluez.Error.AuthenticationRejected" | "org.bluez.Error.Rejected" => {
                DomainError::Failed("Bluetooth authentication was rejected".into())
            }
            "org.bluez.Error.AuthenticationFailed" => {
                DomainError::Failed("Bluetooth authentication failed".into())
            }
            "org.bluez.Error.AuthenticationTimeout" => {
                DomainError::Failed("Bluetooth authentication timed out".into())
            }
            _ => DomainError::Failed("BlueZ rejected the request".into()),
        },
        _ => unavailable("Bluetooth transport is unavailable"),
    }
}
pub(super) async fn bounded<T>(
    operation: impl Future<Output = Result<T>>,
    deadline: Instant,
) -> Result<T> {
    future::or(operation, async {
        async_io::Timer::at(deadline).await;
        Err(unavailable("Bluetooth operation timed out"))
    })
    .await
}
#[derive(Clone)]
pub(super) struct Transport {
    pub connection: Connection,
    pub owner: String,
}
impl Transport {
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
        // Target the observed unique owner, never a replacement daemon.
        self.connection
            .call_method(
                Some(self.owner.as_str()),
                path,
                Some(interface),
                member,
                body,
            )
            .await
            .map_err(bus_error)
    }
    pub async fn read(&self) -> Result<Managed> {
        let raw: Managed = self
            .call(
                "/",
                "org.freedesktop.DBus.ObjectManager",
                "GetManagedObjects",
                &(),
            )
            .await?
            .body()
            .deserialize()
            .map_err(|_| protocol())?;
        if raw.len() > 4096 {
            return Err(unavailable(
                "Bluetooth object catalog exceeded the observation bound",
            ));
        }
        Ok(raw)
    }
    pub async fn set(
        &self,
        path: &str,
        interface: &str,
        property: &str,
        value: bool,
    ) -> Result<()> {
        self.call(
            path,
            "org.freedesktop.DBus.Properties",
            "Set",
            &(interface, property, zbus::zvariant::Value::from(value)),
        )
        .await?;
        Ok(())
    }
}
async fn service_owner(connection: &Connection) -> Result<String> {
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetNameOwner",
            &(SERVICE,),
        )
        .await
        .map_err(bus_error)?
        .body()
        .deserialize()
        .map_err(|_| protocol())
}
pub(super) async fn connect(
    address: Option<&str>,
    timeout: Duration,
) -> Result<(Transport, Signals)> {
    bounded(
        async {
            let builder = match address {
                Some(address) => zbus::connection::Builder::address(address),
                None => zbus::connection::Builder::system(),
            }
            .map_err(bus_error)?;
            // Bound each operation: a connection-wide timeout would also
            // cut off interactive pairing while its user decision is pending.
            let connection = builder.build().await.map_err(bus_error)?;
            let rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender("org.freedesktop.DBus")
                .map_err(bus_error)?
                .interface("org.freedesktop.DBus")
                .map_err(bus_error)?
                .member("NameOwnerChanged")
                .map_err(bus_error)?
                .arg(0, SERVICE)
                .map_err(bus_error)?
                .build();
            let owners = MessageStream::for_match_rule(rule, &connection, Some(8))
                .await
                .map_err(bus_error)?;
            let owner = service_owner(&connection).await?;
            let rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender(owner.as_str())
                .map_err(bus_error)?
                .build();
            // BlueZ emits ObjectManager signals at / and property signals
            // below /org/bluez. A root path_namespace filter suppresses the
            // latter on the guest's system bus. Restrict the unique sender
            // here; Notices filters the relevant interfaces and members.
            let events = MessageStream::for_match_rule(rule, &connection, Some(SIGNAL_LIMIT))
                .await
                .map_err(bus_error)?;
            if service_owner(&connection).await? != owner {
                return Err(unavailable("BlueZ changed while connecting"));
            }
            Ok((
                Transport { connection, owner },
                Signals {
                    events,
                    owners,
                    failed: false,
                },
            ))
        },
        Instant::now() + timeout,
    )
    .await
}
#[derive(Default)]
pub(super) struct Notices {
    pub dirty: bool,
    pub removed: BTreeSet<String>,
}
impl Notices {
    fn signal(&mut self, message: Message) -> Result<()> {
        let header = message.header();
        let interface = header.interface().map(|v| v.as_str()).unwrap_or("");
        let member = header.member().map(|v| v.as_str()).unwrap_or("");
        match (interface, member) {
            ("org.freedesktop.DBus.ObjectManager", "InterfacesRemoved") => {
                let (path, interfaces): (OwnedObjectPath, Vec<String>) =
                    message.body().deserialize().map_err(|_| protocol())?;
                self.dirty |= interfaces
                    .iter()
                    .any(|v| matches!(v.as_str(), ADAPTER | DEVICE | BATTERY));
                if interfaces.iter().any(|v| v == ADAPTER || v == DEVICE) {
                    if self.removed.len() >= REMOVAL_LIMIT {
                        return Err(unavailable("Bluetooth removal burst exceeded the bound"));
                    }
                    self.removed.insert(path.to_string());
                }
            }
            ("org.freedesktop.DBus.ObjectManager", "InterfacesAdded") => {
                let (_, interfaces): (OwnedObjectPath, HashMap<String, Properties>) =
                    message.body().deserialize().map_err(|_| protocol())?;
                self.dirty |= interfaces
                    .keys()
                    .any(|v| matches!(v.as_str(), ADAPTER | DEVICE | BATTERY));
            }
            ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                let (changed, _, _): (String, Properties, Vec<String>) =
                    message.body().deserialize().map_err(|_| protocol())?;
                self.dirty |= matches!(changed.as_str(), ADAPTER | DEVICE | BATTERY);
            }
            _ => (),
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
    pub async fn wait(
        &mut self,
        receive: &async_channel::Receiver<Request>,
        agent: &async_channel::Receiver<super::agent::Event>,
        completed: &async_channel::Receiver<super::session::Completion>,
        stop: &dyn StopSignal,
        notices: &mut Notices,
    ) -> Result<super::session::Wake> {
        use super::session::Wake;
        loop {
            if stop.cancelled() {
                return Err(unavailable("Bluetooth observation cancelled"));
            }
            let event = future::or(
                async {
                    self.owners.next().await;
                    Pump::Owner
                },
                future::or(
                    async {
                        Pump::Done(
                            completed
                                .recv()
                                .await
                                .map(Wake::Completed)
                                .map_err(|_| unavailable("Bluetooth completion queue ended")),
                        )
                    },
                    future::or(
                        async {
                            Pump::Done(
                                receive
                                    .recv()
                                    .await
                                    .map(Wake::Request)
                                    .map_err(|_| unavailable("Bluetooth command queue ended")),
                            )
                        },
                        future::or(
                            async {
                                Pump::Done(
                                    agent
                                        .recv()
                                        .await
                                        .map(Wake::Agent)
                                        .map_err(|_| unavailable("Bluetooth agent queue ended")),
                                )
                            },
                            future::or(async { Pump::Event(self.events.next().await) }, async {
                                async_io::Timer::after(Duration::from_millis(100)).await;
                                Pump::Tick
                            }),
                        ),
                    ),
                ),
            )
            .await;
            match event {
                Pump::Done(result) => return result,
                Pump::Event(Some(Ok(message))) => {
                    notices.signal(message)?;
                    if notices.dirty {
                        return Ok(Wake::Refresh);
                    }
                }
                Pump::Event(_) | Pump::Owner => {
                    self.failed = true;
                    return Err(unavailable("BlueZ disconnected or changed owner"));
                }
                Pump::Tick => return Ok(Wake::Tick),
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
        let mut operation = pin!(operation);
        loop {
            if stop.cancelled() {
                return Err(unavailable("Bluetooth observation cancelled"));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                self.failed = true;
                return Err(unavailable(
                    "Bluetooth operation timed out; observe state before retrying",
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
                    return Err(unavailable("BlueZ disconnected or changed owner"));
                }
                Pump::Tick => (),
            }
        }
    }
    pub async fn drain(&mut self, notices: &mut Notices) -> Result<()> {
        if future::poll_once(self.owners.next()).await.is_some() {
            self.failed = true;
            return Err(unavailable("BlueZ changed owner"));
        }
        for _ in 0..SIGNAL_LIMIT {
            match future::poll_once(self.events.next()).await {
                Some(Some(Ok(message))) => notices.signal(message)?,
                Some(_) => {
                    self.failed = true;
                    return Err(unavailable("Bluetooth event stream ended"));
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
