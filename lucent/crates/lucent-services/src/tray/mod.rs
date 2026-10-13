//! Native session-bus watcher and application actions. No UI/toolkit dependency.
mod native;
mod watcher;
use lucent_domain::*;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
const DEADLINE: Duration = Duration::from_secs(3);
enum Command {
    Act(TrayItemId, TrayAction),
    Menu(TrayItemId, i32),
    Invoke(TrayItemId, u32, i32, u32),
}
enum Reply {
    Done,
    Menu(TrayMenu),
}
struct Request {
    command: Command,
    reply: mpsc::SyncSender<Result<Reply>>,
    deadline: Instant,
    cancelled: std::sync::Arc<AtomicBool>,
}
#[derive(Default)]
pub struct StatusNotifier {
    address: Option<String>,
    watching: AtomicBool,
    identity: AtomicU64,
    online: Mutex<Option<async_channel::Sender<Request>>>,
}
impl StatusNotifier {
    pub fn with_bus_address(address: String) -> Self {
        Self {
            address: Some(address),
            ..Default::default()
        }
    }
    fn identity(&self) -> Result<TrayItemId> {
        let id = self
            .identity
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| native::unavailable("Tray identities exhausted"))?;
        TrayItemId::new(id + 1)
    }
    fn request(&self, command: Command) -> Result<Reply> {
        let sender = self
            .online
            .lock()
            .map_err(|_| native::unavailable("Tray admission failed"))?
            .clone()
            .ok_or_else(|| native::unavailable("Tray is not ready"))?;
        let (reply, receive) = mpsc::sync_channel(1);
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        sender
            .try_send(Request {
                command,
                reply,
                deadline: Instant::now() + DEADLINE,
                cancelled: cancelled.clone(),
            })
            .map_err(|_| native::unavailable("Tray request queue is unavailable or full"))?;
        match receive.recv_timeout(DEADLINE + Duration::from_millis(200)) {
            Ok(result) => result,
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(native::unavailable(
                    "Tray request timed out; its outcome may be uncertain",
                ))
            }
        }
    }
}
struct Observer<'a>(&'a StatusNotifier);
impl Drop for Observer<'_> {
    fn drop(&mut self) {
        *self.0.online.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.0.watching.store(false, Ordering::Release);
    }
}
impl TrayPort for StatusNotifier {
    fn watch(&self, emit: &mut dyn FnMut(Result<TraySnapshot>), stop: &dyn StopSignal) {
        if self.watching.swap(true, Ordering::AcqRel) {
            emit(Err(native::unavailable("Tray already has an observer")));
            return;
        }
        let _observer = Observer(self);
        while !stop.cancelled() {
            let result = futures_lite::future::block_on(native::observe(self, emit, stop));
            *self.online.lock().unwrap_or_else(|e| e.into_inner()) = None;
            if stop.cancelled() {
                break;
            }
            emit(Err(result
                .err()
                .unwrap_or_else(|| native::unavailable("Tray disconnected"))));
            stop.wait(Duration::from_secs(2));
        }
    }
    fn act(&self, item: TrayItemId, action: TrayAction) -> Result<()> {
        self.request(Command::Act(item, action)).map(|_| ())
    }
    fn menu(&self, item: TrayItemId, parent: i32) -> Result<TrayMenu> {
        match self.request(Command::Menu(item, parent))? {
            Reply::Menu(menu) => Ok(menu),
            Reply::Done => Err(native::unavailable("Tray menu reply was invalid")),
        }
    }
    fn invoke(&self, item: TrayItemId, revision: u32, entry: i32, timestamp: u32) -> Result<()> {
        self.request(Command::Invoke(item, revision, entry, timestamp))
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests;
