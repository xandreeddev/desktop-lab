//! Native BlueZ adapter. One subscription worker owns its connection.
//! Commands use that worker; no subprocess, toolkit or network UI is involved.
mod agent;
mod catalog;
mod control;
mod native;
mod pairing;
mod session;
use lucent_domain::*;
use native::unavailable;
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
enum Intent {
    Control(BluetoothCommand),
    Pair(BluetoothObjectId),
    Answer(BluetoothPairingId, BluetoothPairingAnswer),
    Cancel(BluetoothPairingId),
    Incoming(bool),
}
enum Reply {
    Done,
    Pairing(BluetoothPairingId),
}
struct Request {
    command: Intent,
    reply: mpsc::SyncSender<Result<Reply>>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}
pub struct BlueZ {
    online: Mutex<Option<async_channel::Sender<Request>>>,
    watching: AtomicBool,
    next_id: AtomicU64,
    address: Option<String>,
    timeout: Duration,
    interactive_timeout: Duration,
}
impl Default for BlueZ {
    fn default() -> Self {
        Self {
            online: Mutex::new(None),
            watching: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            address: None,
            timeout: OPERATION_TIMEOUT,
            interactive_timeout: Duration::from_secs(120),
        }
    }
}
impl BlueZ {
    /// An explicit bus address is useful for an isolated integration environment.
    /// The default always uses the system bus, not the desktop session bus.
    pub fn with_address(address: impl Into<String>) -> Self {
        Self {
            address: Some(address.into()),
            ..Self::default()
        }
    }
    fn allocate(&self) -> Result<BluetoothObjectId> {
        self.next_id
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| unavailable("Bluetooth identity space exhausted"))
            .and_then(BluetoothObjectId::new)
    }
    fn offline(&self) {
        *self.online.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    async fn connection(
        &self,
        emit: &mut dyn FnMut(Result<BluetoothSnapshot>),
        stop: &dyn StopSignal,
    ) -> Result<()> {
        let (transport, mut signals) =
            native::connect(self.address.as_deref(), self.timeout).await?;
        let result = self.observe(&transport, &mut signals, emit, stop).await;
        self.offline();
        // End the unique owner's lifetime on every return. This releases
        // discovery sessions even after an uncertain, timed-out method call.
        let closed = native::bounded(
            async {
                transport
                    .connection
                    .close()
                    .await
                    .map_err(|_| unavailable("Bluetooth connection cleanup failed"))
            },
            Instant::now() + Duration::from_millis(500),
        )
        .await;
        result.and(closed)
    }
    fn allocate_pairing(&self) -> Result<BluetoothPairingId> {
        self.allocate()
            .and_then(|id| BluetoothPairingId::new(id.value()))
    }
    async fn observe(
        &self,
        transport: &native::Transport,
        signals: &mut native::Signals,
        emit: &mut dyn FnMut(Result<BluetoothSnapshot>),
        stop: &dyn StopSignal,
    ) -> Result<()> {
        session::run(self, transport, signals, emit, stop).await
    }
    fn request(&self, command: Intent) -> Result<Reply> {
        let duration = if matches!(
            command,
            Intent::Incoming(true) | Intent::Control(BluetoothCommand::Connect(_))
        ) {
            self.interactive_timeout
        } else {
            self.timeout
        };
        let send = self
            .online
            .lock()
            .map_err(|_| unavailable("Bluetooth adapter lock poisoned"))?
            .clone()
            .ok_or_else(|| unavailable("Bluetooth service is not ready"))?;
        let (reply, receive) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        send.try_send(Request {
            command,
            reply,
            cancelled: cancelled.clone(),
            deadline: Instant::now() + duration,
        })
        .map_err(|_| unavailable("Bluetooth command queue is unavailable or full"))?;
        match receive.recv_timeout(duration + Duration::from_millis(200)) {
            Ok(result) => result,
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(unavailable(
                    "Bluetooth command was not acknowledged; observe state before retrying",
                ))
            }
        }
    }
    fn request_done(&self, intent: Intent) -> Result<()> {
        match self.request(intent)? {
            Reply::Done => Ok(()),
            Reply::Pairing(_) => Err(unavailable("Unexpected Bluetooth acknowledgement")),
        }
    }
}
struct Watching<'a>(&'a BlueZ);
impl Drop for Watching<'_> {
    fn drop(&mut self) {
        self.0.offline();
        self.0.watching.store(false, Ordering::Release);
    }
}
impl BluetoothPort for BlueZ {
    fn watch(&self, emit: &mut dyn FnMut(Result<BluetoothSnapshot>), stop: &dyn StopSignal) {
        if self
            .watching
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            emit(Err(unavailable(
                "Bluetooth adapter already has an observer",
            )));
            return;
        }
        let _guard = Watching(self);
        while !stop.cancelled() {
            let result = futures_lite::future::block_on(self.connection(emit, stop));
            self.offline();
            if !stop.cancelled() {
                emit(Err(result.err().unwrap_or_else(|| {
                    unavailable("Bluetooth observation ended")
                })));
                stop.wait(RECONNECT_DELAY);
            }
        }
    }
    fn control(&self, command: BluetoothCommand) -> Result<()> {
        self.request_done(Intent::Control(command))
    }
    fn start_pairing(&self, device: BluetoothObjectId) -> Result<BluetoothPairingId> {
        match self.request(Intent::Pair(device))? {
            Reply::Pairing(id) => Ok(id),
            Reply::Done => Err(unavailable("Unexpected Bluetooth acknowledgement")),
        }
    }
    fn answer_pairing(&self, id: BluetoothPairingId, answer: BluetoothPairingAnswer) -> Result<()> {
        self.request_done(Intent::Answer(id, answer))
    }
    fn cancel_pairing(&self, id: BluetoothPairingId) -> Result<()> {
        self.request_done(Intent::Cancel(id))
    }
    fn set_incoming_pairing(&self, enabled: bool) -> Result<()> {
        self.request_done(Intent::Incoming(enabled))
    }
}

#[cfg(test)]
mod tests;
