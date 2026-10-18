//! Native MPRIS observation and current-player commands. Construction starts no
//! worker; one subscription owns the session connection and all temporary IDs.
mod native;
mod snapshot;
use lucent_domain::*;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(3);
const REQUEST_LIMIT: usize = 1;
struct Request {
    id: MediaOperationId,
    command: MediaCommand,
    deadline: Instant,
}
struct Operation {
    value: MediaOperation,
    terminal_published: bool,
}
pub struct Mpris {
    address: Option<String>,
    watching: AtomicBool,
    identity: AtomicU64,
    online: Mutex<Option<async_channel::Sender<Request>>>,
    operation: Mutex<Option<Operation>>,
    timeout: Duration,
}
impl Default for Mpris {
    fn default() -> Self {
        Self {
            address: None,
            watching: AtomicBool::new(false),
            identity: AtomicU64::new(1),
            online: Mutex::new(None),
            operation: Mutex::new(None),
            timeout: DEADLINE,
        }
    }
}
impl Mpris {
    /// Explicit owned test bus. This does not fall back to the user's bus if
    /// that address is unavailable.
    pub fn with_bus_address(address: String) -> Self {
        Self {
            address: Some(address),
            ..Default::default()
        }
    }
    fn identity(&self) -> Result<u64> {
        self.identity
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| native::unavailable("Media identity space exhausted"))
    }
    fn offline(&self) {
        // Lock ordering matches admission so no request can enter an already
        // retired observer. Neither lock is held while emitting observations.
        let mut online = self
            .online
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *online = None;
        let mut operation = self
            .operation
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(operation) = operation.as_mut()
            && operation.value.phase == MediaOperationPhase::InProgress
        {
            operation.value.phase = MediaOperationPhase::Failed(native::unavailable(
                "Media observation ended; the command outcome may be uncertain",
            ));
            operation.terminal_published = false;
        }
    }
    fn operation(&self) -> Option<MediaOperation> {
        self.operation
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|operation| operation.value.clone())
    }
    fn finish(&self, id: MediaOperationId, result: Result<()>) {
        let mut operation = self
            .operation
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(operation) = operation.as_mut().filter(|operation| {
            operation.value.id == id && operation.value.phase == MediaOperationPhase::InProgress
        }) {
            operation.value.phase = match result {
                Ok(()) => MediaOperationPhase::Acknowledged,
                Err(error) => MediaOperationPhase::Failed(error),
            };
            operation.terminal_published = false;
        }
    }
    fn published(&self, observed: Option<&MediaOperation>) {
        let mut operation = self
            .operation
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(operation) = operation.as_mut().filter(|operation| {
            Some(&operation.value) == observed
                && operation.value.phase != MediaOperationPhase::InProgress
        }) {
            operation.terminal_published = true;
        }
    }
}
struct Observer<'a>(&'a Mpris);
impl Drop for Observer<'_> {
    fn drop(&mut self) {
        self.0.offline();
        self.0.watching.store(false, Ordering::Release);
    }
}
impl MediaPort for Mpris {
    fn watch(&self, emit: &mut dyn FnMut(Result<MediaSnapshot>), stop: &dyn StopSignal) {
        if self.watching.swap(true, Ordering::AcqRel) {
            emit(Err(native::unavailable("Media already has an observer")));
            return;
        }
        let _observer = Observer(self);
        while !stop.cancelled() {
            let result = futures_lite::future::block_on(native::observe(self, emit, stop));
            self.offline();
            if stop.cancelled() {
                break;
            }
            emit(Err(result.err().unwrap_or_else(|| {
                native::unavailable("Media disconnected")
            })));
            stop.wait(Duration::from_secs(2));
        }
    }
    fn start(&self, command: MediaCommand) -> Result<MediaOperationId> {
        let online = self
            .online
            .try_lock()
            .map_err(|_| native::unavailable("Media admission is unavailable"))?;
        let sender = online
            .as_ref()
            .ok_or_else(|| native::unavailable("Media observation is not ready"))?;
        let mut operation = self
            .operation
            .try_lock()
            .map_err(|_| native::unavailable("Media operation admission is unavailable"))?;
        if operation.as_ref().is_some_and(|operation| {
            operation.value.phase == MediaOperationPhase::InProgress
                || !operation.terminal_published
        }) {
            return Err(native::unavailable(
                "A media operation is pending or awaiting publication",
            ));
        }
        let id = MediaOperationId::new(self.identity()?)?;
        sender
            .try_send(Request {
                id,
                command,
                deadline: Instant::now() + self.timeout,
            })
            .map_err(|_| native::unavailable("Media command queue is unavailable or full"))?;
        *operation = Some(Operation {
            value: MediaOperation {
                id,
                command,
                phase: MediaOperationPhase::InProgress,
            },
            terminal_published: false,
        });
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
