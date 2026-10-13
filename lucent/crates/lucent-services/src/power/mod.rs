//! Native power services, with a single observing owner and bounded admission.
mod backlights;
mod native;
mod session;
use lucent_domain::*;
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const REQUEST_LIMIT: usize = 32;
const DEADLINE: Duration = Duration::from_secs(5);
const INTERACTIVE_DEADLINE: Duration = Duration::from_secs(120);
struct Request {
    command: PowerCommand,
    reply: mpsc::SyncSender<Result<PowerOperationId>>,
    cancelled: std::sync::Arc<AtomicBool>,
    deadline: Instant,
}
pub struct PowerServices {
    address: Option<String>,
    backlight_root: PathBuf,
    session_hint: Option<String>,
    online: Mutex<Option<async_channel::Sender<Request>>>,
    watching: AtomicBool,
    identity: AtomicU64,
}
impl Default for PowerServices {
    fn default() -> Self {
        Self {
            address: None,
            backlight_root: "/sys/class/backlight".into(),
            session_hint: std::env::var("XDG_SESSION_ID")
                .ok()
                .filter(|s| !s.is_empty()),
            online: Mutex::new(None),
            watching: AtomicBool::new(false),
            identity: AtomicU64::new(1),
        }
    }
}
impl PowerServices {
    /// Explicit isolated-bus/sysfs fixture; production uses the system bus.
    pub fn with_fixture(address: String, backlights: PathBuf) -> Self {
        Self {
            address: Some(address),
            backlight_root: backlights,
            session_hint: None,
            ..Default::default()
        }
    }
    fn identity(&self) -> Result<u64> {
        self.identity
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| DomainError::Unavailable("Power identities exhausted".into()))
    }
}
struct Observer<'a>(&'a PowerServices);
impl Drop for Observer<'_> {
    fn drop(&mut self) {
        if let Ok(mut online) = self.0.online.lock() {
            *online = None;
        }
        self.0.watching.store(false, Ordering::Release);
    }
}
impl PowerPort for PowerServices {
    fn watch(&self, emit: &mut dyn FnMut(Result<PowerSnapshot>), stop: &dyn StopSignal) {
        if self.watching.swap(true, Ordering::AcqRel) {
            emit(Err(DomainError::Unavailable(
                "Power already has an observer".into(),
            )));
            return;
        }
        let _observer = Observer(self);
        while !stop.cancelled() {
            let result = futures_lite::future::block_on(session::observe(self, emit, stop));
            if let Ok(mut online) = self.online.lock() {
                *online = None;
            }
            if stop.cancelled() {
                break;
            }
            emit(Err(result.err().unwrap_or_else(|| {
                DomainError::Unavailable("Power observation ended".into())
            })));
            stop.wait(Duration::from_secs(2));
        }
    }
    fn control(&self, command: PowerCommand) -> Result<PowerOperationId> {
        let send = self
            .online
            .lock()
            .map_err(|_| DomainError::Unavailable("Power admission lock failed".into()))?
            .clone()
            .ok_or_else(|| DomainError::Unavailable("Power service is not ready".into()))?;
        let (reply, receive) = mpsc::sync_channel(1);
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        send.try_send(Request {
            command,
            reply,
            cancelled: cancelled.clone(),
            deadline: Instant::now() + DEADLINE,
        })
        .map_err(|_| {
            DomainError::Unavailable("Power command queue is unavailable or full".into())
        })?;
        match receive.recv_timeout(DEADLINE + Duration::from_millis(200)) {
            Ok(result) => result,
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(DomainError::Unavailable(
                    "Power admission is uncertain; observe before retrying".into(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests;
