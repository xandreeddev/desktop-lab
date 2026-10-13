//! Owner loop: event-driven observations continue while native authorization waits.
use super::{native::*, *};
use async_executor::LocalExecutor;
use futures_lite::{Stream, StreamExt, future, stream};
use std::pin::Pin;
use zbus::{Connection, MatchRule, Message, MessageStream, message::Type};
type Messages = Pin<Box<dyn Stream<Item = zbus::Result<Message>> + Send>>;
struct Events {
    owners: Messages,
    properties: Messages,
}
async fn add(
    messages: &mut Messages,
    connection: &Connection,
    rule: MatchRule<'static>,
    limit: usize,
) -> Result<()> {
    let next = MessageStream::for_match_rule(rule, connection, Some(limit))
        .await
        .map_err(bus_error)?;
    let previous = std::mem::replace(messages, Box::pin(stream::empty()));
    *messages = Box::pin(stream::or(previous, next));
    Ok(())
}
impl Events {
    async fn owners(connection: &Connection) -> Result<Self> {
        let mut events = Self {
            owners: Box::pin(stream::pending()),
            properties: Box::pin(stream::pending()),
        };
        for name in [UPOWER, PROFILES, LEGACY_PROFILES, LOGIN] {
            let rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender("org.freedesktop.DBus")
                .map_err(bus_error)?
                .interface("org.freedesktop.DBus")
                .map_err(bus_error)?
                .member("NameOwnerChanged")
                .map_err(bus_error)?
                .arg(0, name)
                .map_err(bus_error)?
                .build();
            add(&mut events.owners, connection, rule, 8).await?;
        }
        Ok(events)
    }
    async fn properties(&mut self, native: &Native) -> Result<()> {
        let mut interfaces = vec![];
        if let Some(target) = &native.battery {
            interfaces.push((target.owner.clone(), UPOWER));
            interfaces.push((target.owner.clone(), DEVICE));
        }
        if let Some(target) = &native.profiles {
            interfaces.push((target.owner.clone(), target.interface));
        }
        if let Some(target) = &native.login {
            interfaces.push((target.owner.clone(), MANAGER));
            interfaces.push((target.owner.clone(), SESSION));
        }
        for (owner, interface) in interfaces {
            let rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender(owner)
                .map_err(bus_error)?
                .interface("org.freedesktop.DBus.Properties")
                .map_err(bus_error)?
                .member("PropertiesChanged")
                .map_err(bus_error)?
                .arg(0, interface)
                .map_err(bus_error)?
                .build();
            add(&mut self.properties, &native.connection, rule, 64).await?;
        }
        if let Some(target) = &native.login {
            let rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender(target.owner.clone())
                .map_err(bus_error)?
                .interface(MANAGER)
                .map_err(bus_error)?
                .member("PrepareForSleep")
                .map_err(bus_error)?
                .build();
            add(&mut self.properties, &native.connection, rule, 8).await?;
        }
        Ok(())
    }
    async fn drain_owners(&mut self) -> Result<()> {
        if future::poll_once(self.owners.next()).await.is_some() {
            return Err(unavailable("Power service ownership changed"));
        }
        Ok(())
    }
    async fn drain(&mut self, sleeping: &mut bool) -> Result<bool> {
        self.drain_owners().await?;
        let mut dirty = false;
        for _ in 0..32 {
            match future::poll_once(self.properties.next()).await {
                Some(Some(message)) => {
                    signal(message.map_err(bus_error)?, sleeping)?;
                    dirty = true;
                }
                Some(None) => return Err(unavailable("Power signal stream ended")),
                None => break,
            }
        }
        Ok(dirty)
    }
}
fn signal(message: Message, sleeping: &mut bool) -> Result<()> {
    if message
        .header()
        .member()
        .is_some_and(|m| m.as_str() == "PrepareForSleep")
    {
        *sleeping = message
            .body()
            .deserialize()
            .map_err(|_| DomainError::Failed("Invalid sleep observation".into()))?;
    }
    Ok(())
}
struct Completion {
    id: PowerOperationId,
    result: Result<()>,
}
enum Wake {
    Owner,
    Property(Option<zbus::Result<Message>>),
    Request(Result<Request>),
    Completion(Result<Completion>),
    Backlight(std::io::Result<bool>),
    Tick,
}
pub(super) async fn observe(
    adapter: &PowerServices,
    emit: &mut dyn FnMut(Result<PowerSnapshot>),
    stop: &dyn StopSignal,
) -> Result<()> {
    let connection = bounded(
        async {
            match &adapter.address {
                Some(address) => zbus::connection::Builder::address(address.as_str())
                    .map_err(bus_error)?
                    .build()
                    .await
                    .map_err(bus_error),
                None => Connection::system().await.map_err(bus_error),
            }
        },
        Instant::now() + DEADLINE,
    )
    .await?;
    let result = {
        let executor = LocalExecutor::new();
        executor
            .run(run(adapter, &connection, &executor, emit, stop))
            .await
    };
    let _ = connection.close().await;
    result
}
async fn run(
    adapter: &PowerServices,
    connection: &Connection,
    executor: &LocalExecutor<'_>,
    emit: &mut dyn FnMut(Result<PowerSnapshot>),
    stop: &dyn StopSignal,
) -> Result<()> {
    let mut events = Events::owners(connection).await?;
    let mut native = Native::new(connection.clone()).await?;
    events.properties(&native).await?;
    events.drain_owners().await?;
    let (send, receive) = async_channel::bounded::<Request>(REQUEST_LIMIT);
    let (completed, completions) = async_channel::bounded::<Completion>(1);
    let mut updates = if adapter.address.is_none() {
        super::backlights::Updates::new().ok()
    } else {
        None
    };
    let mut snapshot = PowerSnapshot::default();
    let mut previous = None;
    let mut dirty = true;
    let mut sleeping = false;
    let mut operation: Option<PowerOperation> = None;
    while !stop.cancelled() {
        dirty |= events.drain(&mut sleeping).await?;
        if connection.is_closed() {
            return Err(unavailable("Power connection ended"));
        }
        if dirty {
            snapshot = native.read(adapter).await;
            events.drain_owners().await?;
            snapshot.preparing_for_sleep = sleeping;
            snapshot.operation = operation.clone();
            *adapter
                .online
                .lock()
                .map_err(|_| unavailable("Power admission lock failed"))? = Some(send.clone());
            dirty = false;
        }
        snapshot.operation = operation.clone();
        snapshot.preparing_for_sleep = sleeping;
        if previous.as_ref() != Some(&snapshot) {
            emit(Ok(snapshot.clone()));
            previous = Some(snapshot.clone());
        }
        let wake = future::or(
            async {
                events.owners.next().await;
                Wake::Owner
            },
            future::or(
                async { Wake::Property(events.properties.next().await) },
                future::or(
                    async {
                        Wake::Request(
                            receive
                                .recv()
                                .await
                                .map_err(|_| unavailable("Power admission ended")),
                        )
                    },
                    future::or(
                        async {
                            Wake::Completion(
                                completions
                                    .recv()
                                    .await
                                    .map_err(|_| unavailable("Power completion ended")),
                            )
                        },
                        future::or(
                            async {
                                Wake::Backlight(match updates.as_mut() {
                                    Some(updates) => updates.next().await,
                                    None => future::pending().await,
                                })
                            },
                            async {
                                async_io::Timer::after(Duration::from_millis(100)).await;
                                Wake::Tick
                            },
                        ),
                    ),
                ),
            ),
        )
        .await;
        match wake {
            Wake::Owner => return Err(unavailable("Power service ownership changed")),
            Wake::Property(Some(message)) => {
                signal(message.map_err(bus_error)?, &mut sleeping)?;
                dirty = true;
            }
            Wake::Property(None) => return Err(unavailable("Power signal stream ended")),
            Wake::Backlight(Ok(changed)) => dirty |= changed,
            Wake::Backlight(Err(_)) => updates = None,
            Wake::Tick => (),
            Wake::Completion(completion) => {
                let completion = completion?;
                let uncertain = matches!(completion.result, Err(DomainError::Unavailable(_)));
                if let Some(current) = operation.as_mut().filter(|o| o.id == completion.id) {
                    current.phase = match completion.result {
                        Ok(()) => PowerOperationPhase::Succeeded,
                        Err(error) => PowerOperationPhase::Failed(error.to_string()),
                    };
                }
                if uncertain {
                    return Err(unavailable(
                        "Power operation is uncertain; observe before retrying",
                    ));
                }
                dirty = true;
            }
            Wake::Request(request) => {
                let request = request?;
                if request.cancelled.load(Ordering::Acquire) || Instant::now() >= request.deadline {
                    let _ = request
                        .reply
                        .try_send(Err(unavailable("Power command expired before execution")));
                    continue;
                }
                // Refresh after queued native signals before resolving transient targets.
                if events.drain(&mut sleeping).await? {
                    snapshot = native.read(adapter).await;
                    events.drain_owners().await?;
                    snapshot.operation = operation.clone();
                    snapshot.preparing_for_sleep = sleeping;
                    dirty = true;
                }
                let plan =
                    lucent_usecases::power::validate_power_command(&snapshot, &request.command)
                        .and_then(|()| native.plan(&request.command));
                let plan = match plan {
                    Ok(plan) => plan,
                    Err(error) => {
                        let _ = request.reply.try_send(Err(error));
                        continue;
                    }
                };
                if request.cancelled.load(Ordering::Acquire) || Instant::now() >= request.deadline {
                    let _ = request
                        .reply
                        .try_send(Err(unavailable("Power command expired before admission")));
                    continue;
                }
                let id = PowerOperationId::new(adapter.identity()?)?;
                if request.reply.try_send(Ok(id)).is_err() {
                    return Err(unavailable("Power admission could not be delivered"));
                }
                operation = Some(PowerOperation {
                    id,
                    command: request.command,
                    phase: PowerOperationPhase::InProgress,
                });
                let completed = completed.clone();
                let connection = connection.clone();
                executor
                    .spawn(async move {
                        let result = plan.apply(&connection).await;
                        let _ = completed.send(Completion { id, result }).await;
                    })
                    .detach();
            }
        }
    }
    Ok(())
}
