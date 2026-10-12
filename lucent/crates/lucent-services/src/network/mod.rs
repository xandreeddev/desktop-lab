//! Native NetworkManager adapter. One subscription worker owns its connection.
//! Commands use that worker; no subprocess, toolkit or network UI is involved.
mod control;
mod native;
mod snapshot;
use lucent_domain::*;
use native::{Notices, unavailable};
use snapshot::Objects;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const REQUEST_LIMIT: usize = 32;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
struct Request {
    command: NetworkCommand,
    reply: mpsc::SyncSender<Result<()>>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}
pub struct NetworkManager {
    online: Mutex<Option<async_channel::Sender<Request>>>,
    watching: AtomicBool,
    next_id: AtomicU64,
    address: Option<String>,
    timeout: Duration,
}
impl Default for NetworkManager {
    fn default() -> Self {
        Self {
            online: Mutex::new(None),
            watching: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            address: None,
            timeout: OPERATION_TIMEOUT,
        }
    }
}
impl NetworkManager {
    /// An explicit bus address is useful for an isolated integration environment.
    /// The default always uses the system bus, not the desktop session bus.
    pub fn with_address(address: impl Into<String>) -> Self {
        Self {
            address: Some(address.into()),
            ..Self::default()
        }
    }
    fn allocate(&self) -> Result<NetworkObjectId> {
        self.next_id
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| unavailable("Network identity space exhausted"))
            .and_then(NetworkObjectId::new)
    }
    fn offline(&self) {
        *self.online.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    async fn connection(
        &self,
        emit: &mut dyn FnMut(Result<NetworkSnapshot>),
        stop: &dyn StopSignal,
    ) -> Result<()> {
        let (transport, mut signals) =
            native::connect(self.address.as_deref(), self.timeout).await?;
        let (send, receive) = async_channel::bounded(REQUEST_LIMIT);
        let mut objects = Objects::default();
        let mut notices = Notices::default();
        let mut previous: Option<NetworkSnapshot> = None;
        let mut dirty = true;
        while !stop.cancelled() {
            if dirty {
                let deadline = Instant::now() + self.timeout;
                loop {
                    notices.dirty = false;
                    let raw = signals
                        .pump(
                            snapshot::read(&transport),
                            Some(deadline),
                            stop,
                            &mut notices,
                        )
                        .await;
                    // Drain already queued removals before assigning public handles.
                    signals.drain(&mut notices).await?;
                    notices.removed.extend(transport.take_missing());
                    let removed = !notices.removed.is_empty();
                    objects.invalidate(&notices.removed);
                    notices.removed.clear();
                    if removed {
                        // The query straddled a removal/reuse. Discard it and take
                        // another within the same absolute refresh deadline.
                        // A disappearing object is a normal partial-query race,
                        // not a reason to retire unrelated devices/profiles.
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
                    let value = objects.replace(raw?, &mut || self.allocate())?;
                    dirty = notices.dirty;
                    *self
                        .online
                        .lock()
                        .map_err(|_| unavailable("Network adapter lock poisoned"))? =
                        Some(send.clone());
                    if previous.as_ref() != Some(&value) {
                        previous = Some(value.clone());
                        emit(Ok(value));
                    }
                    break;
                }
            }
            // Accepted commands get a turn between coalesced state refreshes.
            let request = match receive.try_recv() {
                Ok(request) => request,
                Err(_) if dirty => continue,
                Err(_) => match signals.idle(&receive, stop, &mut notices).await? {
                    Some(request) => request,
                    None => {
                        dirty = true;
                        continue;
                    }
                },
            };
            signals.drain(&mut notices).await?;
            notices.removed.extend(transport.take_missing());
            objects.invalidate(&notices.removed);
            if request.cancelled.load(Ordering::Relaxed) || Instant::now() >= request.deadline {
                let _ = request
                    .reply
                    .try_send(Err(unavailable("Network command expired before execution")));
                dirty |= notices.dirty;
                continue;
            }
            // Resolve handles against the last observation on this same owner.
            // A later service-side change can still reject a request.
            let result = match control::plan(request.command, &objects, previous.as_ref().unwrap())
            {
                Ok(plan) => {
                    signals
                        .pump(
                            control::apply(&transport, plan),
                            Some(request.deadline),
                            stop,
                            &mut notices,
                        )
                        .await
                }
                Err(error) => Err(error),
            };
            let transport_failed = matches!(result, Err(DomainError::Unavailable(_)));
            let _ = request.reply.try_send(result);
            if transport_failed || signals.failed || transport.connection.is_closed() {
                return Err(unavailable(
                    "Network connection ended before state confirmation",
                ));
            }
            dirty = true;
        }
        Ok(())
    }
}
struct Watching<'a>(&'a NetworkManager);
impl Drop for Watching<'_> {
    fn drop(&mut self) {
        self.0.offline();
        self.0.watching.store(false, Ordering::Release);
    }
}
impl NetworkPort for NetworkManager {
    fn watch(&self, emit: &mut dyn FnMut(Result<NetworkSnapshot>), stop: &dyn StopSignal) {
        if self
            .watching
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            emit(Err(unavailable("Network adapter already has an observer")));
            return;
        }
        let _guard = Watching(self);
        while !stop.cancelled() {
            let result = futures_lite::future::block_on(self.connection(emit, stop));
            self.offline();
            if !stop.cancelled() {
                emit(Err(result
                    .err()
                    .unwrap_or_else(|| unavailable("Network observation ended"))));
                stop.wait(RECONNECT_DELAY);
            }
        }
    }
    fn control(&self, command: NetworkCommand) -> Result<()> {
        let send = self
            .online
            .lock()
            .map_err(|_| unavailable("Network adapter lock poisoned"))?
            .clone()
            .ok_or_else(|| unavailable("Network service is not ready"))?;
        let (reply, receive) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        send.try_send(Request {
            command,
            reply,
            cancelled: cancelled.clone(),
            deadline: Instant::now() + self.timeout,
        })
        .map_err(|_| unavailable("Network command queue is unavailable or full"))?;
        match receive.recv_timeout(self.timeout + Duration::from_millis(200)) {
            Ok(result) => result,
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(unavailable(
                    "Network command was not acknowledged; observe state before retrying",
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests;
