//! The owner keeps routing user replies while native methods await them.
use super::{
    agent,
    catalog::{Catalog, Kind},
    control,
    native::*,
    pairing::Pairing,
    *,
};
use async_executor::LocalExecutor;
use std::{collections::BTreeSet, future::Future};

pub(super) enum Completion {
    Control {
        reply: mpsc::SyncSender<Result<Reply>>,
        result: Result<()>,
        discovery: Option<(String, bool)>,
    },
    Pair {
        id: BluetoothPairingId,
        result: Result<()>,
    },
}
pub(super) enum Wake {
    Request(Request),
    Agent(agent::Event),
    Completed(Completion),
    Refresh,
    Tick,
}

pub(super) async fn run(
    adapter: &BlueZ,
    transport: &Transport,
    signals: &mut Signals,
    emit: &mut dyn FnMut(Result<BluetoothSnapshot>),
    stop: &dyn StopSignal,
) -> Result<()> {
    let executor = LocalExecutor::new();
    executor
        .run(observe(adapter, transport, signals, emit, stop, &executor))
        .await
}
fn submit(
    executor: &LocalExecutor<'_>,
    send: async_channel::Sender<Completion>,
    operation: impl Future<Output = Completion> + 'static,
) {
    executor
        .spawn(async move {
            let _ = send.send(operation.await).await;
        })
        .detach();
}
async fn observe(
    adapter: &BlueZ,
    transport: &Transport,
    signals: &mut Signals,
    emit: &mut dyn FnMut(Result<BluetoothSnapshot>),
    stop: &dyn StopSignal,
    executor: &LocalExecutor<'_>,
) -> Result<()> {
    let (send, receive) = async_channel::bounded(REQUEST_LIMIT);
    // One ordinary call and one owned pairing call can be pending; replies
    // are serviced independently of either call's backend completion.
    let (completed, completions) = async_channel::bounded(4);
    let mut control_pending = false;
    let mut objects = Catalog::default();
    let mut owned = BTreeSet::new();
    let mut pairing = Pairing::new(transport.owner.clone());
    let mut notices = Notices::default();
    let mut snapshot = BluetoothSnapshot::default();
    let mut previous = None;
    let mut dirty = true;
    loop {
        if stop.cancelled() {
            break;
        }
        if dirty {
            let deadline = Instant::now() + adapter.timeout;
            loop {
                notices.dirty = false;
                let raw = signals
                    .pump(transport.read(), Some(deadline), stop, &mut notices)
                    .await;
                signals.drain(&mut notices).await?;
                let removed = !notices.removed.is_empty();
                objects.invalidate(&notices.removed);
                notices.removed.clear();
                if removed {
                    signals
                        .pump(
                            async {
                                async_io::Timer::after(Duration::from_millis(20)).await;
                                Ok(())
                            },
                            Some(deadline),
                            stop,
                            &mut notices,
                        )
                        .await?;
                    continue;
                }
                snapshot = objects.replace(raw?, &mut owned, &mut || adapter.allocate())?;
                dirty = notices.dirty;
                *adapter
                    .online
                    .lock()
                    .map_err(|_| unavailable("Bluetooth adapter lock poisoned"))? =
                    Some(send.clone());
                break;
            }
        }
        pairing.synchronize(&objects, &mut || adapter.allocate_pairing())?;
        pairing.publish(&mut snapshot);
        if previous.as_ref() != Some(&snapshot) {
            previous = Some(snapshot.clone());
            emit(Ok(snapshot.clone()));
        }
        let wake = if let Ok(value) = completions.try_recv() {
            Wake::Completed(value)
        } else if let Ok(value) = receive.try_recv() {
            Wake::Request(value)
        } else if let Ok(value) = pairing.host.events.try_recv() {
            Wake::Agent(value)
        } else if dirty {
            continue;
        } else {
            signals
                .wait(
                    &receive,
                    &pairing.host.events,
                    &completions,
                    stop,
                    &mut notices,
                )
                .await?
        };
        match wake {
            Wake::Tick => (),
            Wake::Refresh => {
                dirty = true;
            }
            Wake::Agent(agent::Event::Changed) => (),
            Wake::Agent(agent::Event::Proposal(proposal)) => {
                if pairing.proposed.len() >= agent::PROPOSAL_LIMIT {
                    if let Some(reply) = proposal.reply {
                        let _ = reply.try_send(agent::Answer::Cancelled);
                    }
                } else {
                    pairing.proposed.push(proposal);
                    dirty = true;
                }
            }
            Wake::Completed(Completion::Pair { id, result }) => {
                let failed = matches!(result, Err(DomainError::Unavailable(_)));
                pairing.finished(id, result);
                dirty = true;
                if failed {
                    return Err(unavailable(
                        "Bluetooth pairing outcome is uncertain; observe before retrying",
                    ));
                }
            }
            Wake::Completed(Completion::Control {
                reply,
                result,
                discovery,
            }) => {
                control_pending = false;
                pairing.control_path = None;
                let failed = matches!(result, Err(DomainError::Unavailable(_)));
                if result.is_ok()
                    && let Some((path, active)) = discovery
                {
                    if active {
                        owned.insert(path);
                    } else {
                        owned.remove(&path);
                    }
                }
                let _ = reply.try_send(result.map(|()| Reply::Done));
                dirty = true;
                if failed {
                    return Err(unavailable(
                        "Bluetooth connection ended before state confirmation",
                    ));
                }
            }
            Wake::Request(request) => {
                signals.drain(&mut notices).await?;
                objects.invalidate(&notices.removed);
                pairing.synchronize(&objects, &mut || adapter.allocate_pairing())?;
                if request.cancelled.load(Ordering::Acquire) || Instant::now() >= request.deadline {
                    let _ = request.reply.try_send(Err(unavailable(
                        "Bluetooth command expired before execution",
                    )));
                    dirty |= notices.dirty;
                    continue;
                }
                match request.command {
                    Intent::Answer(id, answer) => {
                        let display = pairing.prompts.get(&id).is_some_and(|p| p.reply.is_none());
                        if display && control_pending {
                            let _ = request.reply.try_send(Err(DomainError::Failed(
                                "Bluetooth operation is already in progress".into(),
                            )));
                        } else {
                            match pairing.answer(id, answer) {
                                Ok(None) => {
                                    let _ = request.reply.try_send(Ok(Reply::Done));
                                }
                                Err(error) => {
                                    let _ = request.reply.try_send(Err(error));
                                }
                                Ok(Some(path)) => {
                                    let transport = transport.clone();
                                    control_pending = true;
                                    submit(executor, completed.clone(), async move {
                                        let result = bounded(
                                            async {
                                                transport
                                                    .call(
                                                        path.as_str(),
                                                        DEVICE,
                                                        "CancelPairing",
                                                        &(),
                                                    )
                                                    .await?;
                                                Ok(())
                                            },
                                            request.deadline,
                                        )
                                        .await;
                                        Completion::Control {
                                            reply: request.reply,
                                            result,
                                            discovery: None,
                                        }
                                    });
                                }
                            }
                        }
                    }
                    Intent::Pair(device) => {
                        let result = async {
                            lucent_usecases::bluetooth::validate_pairing_target(&snapshot, device)?;
                            if pairing.active_path.is_some() || !pairing.prompts.is_empty() {
                                return Err(DomainError::Invalid(
                                    "Resolve the current pairing request first".into(),
                                ));
                            }
                            signals
                                .pump(
                                    pairing.host.register(transport),
                                    Some(request.deadline),
                                    stop,
                                    &mut notices,
                                )
                                .await?;
                            signals.drain(&mut notices).await?;
                            objects.invalidate(&notices.removed);
                            let path = objects.resolve(device, Kind::Device)?;
                            let id = adapter.allocate_pairing()?;
                            pairing.start(id, device, path.clone())?;
                            let transport = transport.clone();
                            let deadline = Instant::now() + adapter.interactive_timeout;
                            submit(executor, completed.clone(), async move {
                                let result = bounded(
                                    async {
                                        transport.call(path.as_str(), DEVICE, "Pair", &()).await?;
                                        Ok(())
                                    },
                                    deadline,
                                )
                                .await;
                                Completion::Pair { id, result }
                            });
                            Ok(Reply::Pairing(id))
                        }
                        .await;
                        let failed = matches!(result, Err(DomainError::Unavailable(_)));
                        let _ = request.reply.try_send(result);
                        if failed {
                            return Err(unavailable("Bluetooth agent could not be established"));
                        }
                    }
                    Intent::Incoming(enabled) => {
                        if pairing.active_path.is_some() {
                            let _ = request.reply.try_send(Err(DomainError::Invalid(
                                "Finish pairing before changing its agent".into(),
                            )));
                        } else {
                            let result = signals
                                .pump(
                                    pairing.host.set_incoming(transport, enabled),
                                    Some(request.deadline),
                                    stop,
                                    &mut notices,
                                )
                                .await;
                            let failed = matches!(result, Err(DomainError::Unavailable(_)));
                            let _ = request.reply.try_send(result.map(|()| Reply::Done));
                            if failed {
                                return Err(unavailable(
                                    "Bluetooth agent registration outcome is uncertain",
                                ));
                            }
                        }
                    }
                    intent => {
                        if control_pending {
                            let _ = request.reply.try_send(Err(DomainError::Failed(
                                "Bluetooth operation is already in progress".into(),
                            )));
                            continue;
                        }
                        let result = match intent {
                            Intent::Control(command) => control::plan(command, &objects, &snapshot),
                            Intent::Cancel(id) => pairing
                                .cancel(id)
                                .map(|path| control::Plan::Device(path, "CancelPairing")),
                            _ => unreachable!(),
                        };
                        match result {
                            Err(error) => {
                                let _ = request.reply.try_send(Err(error));
                            }
                            Ok(plan) => {
                                if let control::Plan::Device(path, "Connect") = &plan {
                                    let registered = signals
                                        .pump(
                                            pairing.host.register(transport),
                                            Some(Instant::now() + adapter.timeout),
                                            stop,
                                            &mut notices,
                                        )
                                        .await;
                                    signals.drain(&mut notices).await?;
                                    objects.invalidate(&notices.removed);
                                    pairing.synchronize(&objects, &mut || {
                                        adapter.allocate_pairing()
                                    })?;
                                    if let Err(error) = registered {
                                        let failed = matches!(error, DomainError::Unavailable(_));
                                        let _ = request.reply.try_send(Err(error));
                                        if failed {
                                            return Err(unavailable(
                                                "Bluetooth agent could not be established",
                                            ));
                                        }
                                        dirty |= notices.dirty;
                                        continue;
                                    }
                                    // Registration can pump removal signals; never reuse an old handle.
                                    if objects.device_at(path.as_str()).is_none_or(|id| {
                                        !snapshot.devices.iter().any(|d| d.id == id)
                                    }) {
                                        let _ = request.reply.try_send(Err(DomainError::Invalid(
                                            "Bluetooth device disappeared; choose a current device"
                                                .into(),
                                        )));
                                        dirty |= notices.dirty;
                                        continue;
                                    }
                                    pairing.control_path = Some(path.clone());
                                }
                                let discovery = match &plan {
                                    control::Plan::Discovery(p, active) => {
                                        Some((p.to_string(), *active))
                                    }
                                    _ => None,
                                };
                                let mut ownership = owned.clone();
                                let transport = transport.clone();
                                control_pending = true;
                                submit(executor, completed.clone(), async move {
                                    let result = bounded(
                                        control::apply(&transport, plan, &mut ownership),
                                        request.deadline,
                                    )
                                    .await;
                                    Completion::Control {
                                        reply: request.reply,
                                        result,
                                        discovery,
                                    }
                                });
                            }
                        }
                    }
                }
                dirty |= notices.dirty;
            }
        }
        if signals.failed || transport.connection.is_closed() {
            return Err(unavailable("Bluetooth connection ended"));
        }
    }
    Ok(())
}
