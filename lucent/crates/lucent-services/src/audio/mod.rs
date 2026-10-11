//! A single native audio connection, owned by the subscription worker.
//! Neither PulseAudio handles nor callback lifetimes escape this adapter.
mod control;
mod native;
mod snapshot;
use lucent_domain::*;
use native::{Batch, Connection, Pending};
use snapshot::{Key, Objects};
use std::{
    cell::Cell,
    collections::BTreeSet,
    io::Write,
    os::unix::net::UnixStream,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};

const REQUEST_LIMIT: usize = 32;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
struct Request {
    command: AudioCommand,
    reply: SyncSender<Result<()>>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}
#[derive(Clone)]
struct Online {
    send: SyncSender<Request>,
    wake: Arc<UnixStream>,
}
impl Online {
    fn wake(&self) {
        let _ = (&*self.wake).write(&[1]);
    }
}
pub struct PulseAudio {
    online: Mutex<Option<Online>>,
    watching: AtomicBool,
    next_id: AtomicU64,
    server: Option<String>,
}
impl Default for PulseAudio {
    fn default() -> Self {
        Self {
            online: Mutex::new(None),
            watching: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            server: None,
        }
    }
}
fn unavailable(message: impl Into<String>) -> DomainError {
    DomainError::Unavailable(message.into())
}
impl PulseAudio {
    fn allocate(&self) -> Result<AudioObjectId> {
        self.next_id
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| unavailable("Audio identity space exhausted"))
            .and_then(AudioObjectId::new)
    }
    fn offline(&self) {
        *self.online.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    fn connection(
        &self,
        emit: &mut dyn FnMut(Result<AudioSnapshot>),
        stop: &dyn StopSignal,
    ) -> Result<()> {
        let (reader, writer) = UnixStream::pair().map_err(|e| unavailable(e.to_string()))?;
        reader
            .set_nonblocking(true)
            .map_err(|e| unavailable(e.to_string()))?;
        writer
            .set_nonblocking(true)
            .map_err(|e| unavailable(e.to_string()))?;
        let connection = Connection::new(self.server.as_deref(), reader)?;
        let (send, receive) = mpsc::sync_channel(REQUEST_LIMIT);
        let online = Online {
            send,
            wake: Arc::new(writer),
        };
        let mut subscription = None;
        let mut objects = Objects::default();
        let mut batch: Option<(Batch, Instant)> = None;
        let mut command: Option<Active> = None;
        let mut refresh_started: Option<Instant> = None;
        let mut previous = None;
        let started = Instant::now();
        let result = (|| {
            while !stop.cancelled() {
                if connection.ready()? {
                    if subscription.is_none() {
                        subscription = Some(connection.subscribe()?);
                    }
                    let subscribed = subscription.as_ref().unwrap().state()?.get();
                    if subscribed == Some(false) {
                        return Err(unavailable("Audio event subscription was rejected"));
                    }
                    if subscribed == Some(true) {
                        {
                            let mut notices = connection.notices()?;
                            if notices.reset {
                                objects.entries.clear();
                                notices.reset = false;
                            }
                            objects.invalidate(&notices.removed);
                            // A notification arriving during a query invalidates that
                            // batch. Do not publish records from before a removal/reuse.
                            if notices.dirty {
                                batch = None;
                            }
                            notices.removed.clear();
                        }
                        if refresh_started.is_some_and(|t| t.elapsed() > OPERATION_TIMEOUT) {
                            return Err(unavailable(
                                "Audio state did not settle before the query deadline",
                            ));
                        }
                        if let Some((pending, deadline)) = &batch {
                            if Instant::now() > *deadline {
                                return Err(unavailable("Audio state query timed out"));
                            }
                            if pending.complete()? {
                                let value =
                                    objects.replace(pending.take()?, &mut || self.allocate())?;
                                batch = None;
                                refresh_started = None;
                                *self
                                    .online
                                    .lock()
                                    .map_err(|_| unavailable("Audio adapter lock poisoned"))? =
                                    Some(online.clone());
                                if command.as_ref().is_some_and(|c| c.applied) {
                                    let mut done = command.take().unwrap();
                                    done.reply(Ok(()));
                                }
                                if previous.as_ref() != Some(&value) {
                                    previous = Some(value.clone());
                                    emit(Ok(value));
                                }
                            }
                        }
                        if let Some(active) = &mut command {
                            if active.request.cancelled.load(Ordering::Relaxed)
                                || Instant::now() > active.request.deadline
                            {
                                return Err(unavailable(
                                    "Audio command was not acknowledged in time",
                                ));
                            }
                            if let Some(success) = active.operation.state()?.take() {
                                if success {
                                    active.applied = true;
                                    // Refresh before admitting the next command, so repeated
                                    // relative adjustments see the previous acknowledged value.
                                    batch = None;
                                    connection.notices()?.dirty = true;
                                } else {
                                    let mut done = command.take().unwrap();
                                    done.reply(Err(unavailable(
                                        "Audio service rejected the operation",
                                    )));
                                    connection.notices()?.dirty = true;
                                }
                            }
                        }
                        if batch.is_none() && connection.notices()?.dirty {
                            connection.notices()?.dirty = false;
                            refresh_started.get_or_insert_with(Instant::now);
                            batch = Some((connection.query()?, Instant::now() + OPERATION_TIMEOUT));
                        }
                        if command.is_none() && batch.is_none() && previous.is_some() {
                            command = admit(&receive, &connection, &objects);
                        }
                    }
                }
                if previous.is_none() && started.elapsed() > OPERATION_TIMEOUT {
                    return Err(unavailable("Audio service did not become ready"));
                }
                // Server events and the command fd wake this poll immediately.
                // The maximum wait only bounds cancellation, not state polling.
                connection.poll()?;
            }
            Ok(())
        })();
        self.offline();
        // Cancel outstanding callbacks before disconnecting/destroying context.
        drop(command);
        drop(batch);
        drop(subscription);
        result
    }
}
#[derive(Default)]
struct Notices {
    dirty: bool,
    removed: BTreeSet<Key>,
    reset: bool,
}
struct Active {
    operation: Pending<Cell<Option<bool>>>,
    replied: bool,
    request: Request,
    applied: bool,
}
impl Active {
    fn reply(&mut self, result: Result<()>) {
        let _ = self.request.reply.try_send(result);
        self.replied = true;
    }
}
impl Drop for Active {
    fn drop(&mut self) {
        // try_send cannot block if success was already acknowledged.
        if !self.replied {
            self.reply(Err(unavailable(
                "Audio connection ended before acknowledgement",
            )));
        }
    }
}
fn admit(
    receive: &Receiver<Request>,
    connection: &Connection,
    objects: &Objects,
) -> Option<Active> {
    for _ in 0..REQUEST_LIMIT {
        let request = receive.try_recv().ok()?;
        if request.cancelled.load(Ordering::Relaxed) || Instant::now() > request.deadline {
            continue;
        }
        match connection.control(objects, request.command.clone()) {
            Ok(operation) => {
                return Some(Active {
                    operation,
                    replied: false,
                    request,
                    applied: false,
                });
            }
            Err(error) => {
                let _ = request.reply.try_send(Err(error));
            }
        }
    }
    None
}
impl AudioPort for PulseAudio {
    fn watch(&self, emit: &mut dyn FnMut(Result<AudioSnapshot>), stop: &dyn StopSignal) {
        if self
            .watching
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            emit(Err(unavailable("Audio observation is already running")));
            return;
        }
        struct Guard<'a>(&'a PulseAudio);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.offline();
                self.0.watching.store(false, Ordering::Release);
            }
        }
        let _guard = Guard(self);
        while !stop.cancelled() {
            let result = self.connection(emit, stop);
            self.offline();
            if stop.cancelled() {
                break;
            }
            emit(Err(result.err().unwrap_or_else(|| {
                unavailable("Audio service disconnected")
            })));
            stop.wait(RECONNECT_DELAY);
        }
    }
    fn control(&self, command: AudioCommand) -> Result<()> {
        let online = self
            .online
            .lock()
            .map_err(|_| unavailable("Audio adapter lock poisoned"))?
            .clone()
            .ok_or_else(|| unavailable("Audio service is not ready"))?;
        let (send, receive) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        online
            .send
            .try_send(Request {
                command,
                reply: send,
                cancelled: cancelled.clone(),
                deadline: Instant::now() + OPERATION_TIMEOUT,
            })
            .map_err(|_| unavailable("Audio command queue is unavailable or full"))?;
        online.wake();
        match receive.recv_timeout(OPERATION_TIMEOUT + Duration::from_secs(1)) {
            Ok(result) => result,
            Err(_) => {
                cancelled.store(true, Ordering::Relaxed);
                online.wake();
                Err(unavailable(
                    "Audio command was not acknowledged; refresh before retrying",
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests;
