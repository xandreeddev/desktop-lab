//! Native MPRIS observation and current-player commands. Construction starts no
//! worker; one subscription owns the session connection and all temporary IDs.
mod native;
mod snapshot;
use lucent_domain::*;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(3);
const REQUEST_LIMIT: usize = 32;
struct Request {
    command: MediaCommand,
    reply: mpsc::SyncSender<Result<()>>,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}
pub struct Mpris {
    address: Option<String>,
    watching: AtomicBool,
    identity: AtomicU64,
    online: Mutex<Option<async_channel::Sender<Request>>>,
    timeout: Duration,
}
impl Default for Mpris {
    fn default() -> Self {
        Self {
            address: None,
            watching: AtomicBool::new(false),
            identity: AtomicU64::new(1),
            online: Mutex::new(None),
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
        *self
            .online
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
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
    fn control(&self, command: MediaCommand) -> Result<()> {
        let sender = self
            .online
            .lock()
            .map_err(|_| native::unavailable("Media admission is unavailable"))?
            .clone()
            .ok_or_else(|| native::unavailable("Media observation is not ready"))?;
        let (reply, receive) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        sender
            .try_send(Request {
                command,
                reply,
                deadline: Instant::now() + self.timeout,
                cancelled: cancelled.clone(),
            })
            .map_err(|_| native::unavailable("Media command queue is unavailable or full"))?;
        match receive.recv_timeout(self.timeout + Duration::from_millis(200)) {
            Ok(result) => result,
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(native::unavailable(
                    "Media request timed out; its outcome may be uncertain",
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests;
