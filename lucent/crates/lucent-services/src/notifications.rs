//! Freedesktop delivery adapter. No rendering or application message types.
use lucent_domain::*;
use lucent_usecases::notifications::NotificationInbox;
mod history;
mod image;
mod storage;
pub use history::JsonNotificationHistory;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Condvar, Mutex, Weak},
    time::{Duration, Instant},
};
use zbus::{blocking::connection::Builder, zvariant::OwnedValue};
const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const SIGNAL_LIMIT: usize = 256;
#[derive(Debug)]
enum AdmissionError {
    Offline,
    Full,
}
impl AdmissionError {
    fn message(&self) -> &'static str {
        match self {
            Self::Offline => "Notification delivery is disconnected; retry after recovery",
            Self::Full => "Notification delivery is busy; retry after it drains",
        }
    }
    fn domain(self) -> DomainError {
        DomainError::Unavailable(self.message().into())
    }
    fn dbus(self) -> zbus::fdo::Error {
        match self {
            Self::Offline => zbus::fdo::Error::Disconnected(self.message().into()),
            Self::Full => zbus::fdo::Error::LimitsExceeded(self.message().into()),
        }
    }
}
enum Signal {
    Closed(u32, CloseReason),
    Action(u32, String),
}
#[derive(Default)]
struct Store {
    inbox: NotificationInbox,
    signals: VecDeque<Signal>,
    revision: u64,
    online: bool,
    history_status: NotificationHistoryStatus,
    loaded: bool,
    writable: bool,
    dirty: bool,
    reload: bool,
    dnd_changed: bool,
    io_serial: u64,
    load_pending: Option<u64>,
    save_pending: Option<u64>,
}
impl Store {
    // Called while holding state, before any mutation. This is admission, not a
    // blocking reservation: the producer must never wait for this queue to drain.
    fn admit(&self, count: usize) -> std::result::Result<(), AdmissionError> {
        if !self.online {
            return Err(AdmissionError::Offline);
        }
        if count > SIGNAL_LIMIT.saturating_sub(self.signals.len()) {
            return Err(AdmissionError::Full);
        }
        Ok(())
    }
    fn expire(&mut self, now: u64) {
        let capacity = SIGNAL_LIMIT.saturating_sub(self.signals.len());
        for id in self.inbox.due(now).into_iter().take(capacity) {
            self.inbox.close(id);
            self.signals
                .push_back(Signal::Closed(id, CloseReason::Expired));
            self.revision = self.revision.wrapping_add(1);
            self.dirty = true;
        }
    }
}
struct Shared {
    state: Mutex<Store>,
    changed: Condvar,
    epoch: Instant,
    history: Option<Arc<dyn NotificationHistoryPort>>,
    worker: Mutex<Option<storage::Worker>>,
}
impl Shared {
    fn storage_complete(&self, completion: storage::Completion) {
        let mut state = self.state.lock().unwrap();
        match completion {
            storage::Completion::Loaded(id, result) => {
                if state.load_pending != Some(id) {
                    return;
                }
                let bootstrap = !state.loaded;
                state.load_pending = None;
                state.loaded = true;
                let preserve_dnd = state.dnd_changed;
                let result =
                    result.and_then(|archive| state.inbox.merge_archive(archive, preserve_dnd));
                state.writable = result.is_ok();
                state.dirty |= result.is_ok() && !bootstrap;
                state.history_status = if result.is_ok() {
                    NotificationHistoryStatus::Ready
                } else {
                    NotificationHistoryStatus::ReadFailed
                };
            }
            storage::Completion::Saved(id, result) => {
                if state.save_pending != Some(id) {
                    return;
                }
                state.save_pending = None;
                state.history_status = if result.is_err() {
                    NotificationHistoryStatus::WriteFailed
                } else if state.dirty {
                    NotificationHistoryStatus::Saving
                } else {
                    NotificationHistoryStatus::Ready
                };
            }
        }
        state.revision = state.revision.wrapping_add(1);
        self.changed.notify_all();
    }
    fn mutate<T, E>(
        &self,
        f: impl FnOnce(&mut Store) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let mut state = self.state.lock().unwrap();
        let result = f(&mut state)?;
        state.revision = state.revision.wrapping_add(1);
        state.dirty = true;
        self.changed.notify_one();
        Ok(result)
    }
    fn close(&self, id: u32, reason: CloseReason) -> std::result::Result<(), AdmissionError> {
        self.mutate(|state| {
            if state.inbox.get(id).is_some() {
                state.admit(1)?;
                state.inbox.close(id);
                state.signals.push_back(Signal::Closed(id, reason));
            }
            Ok(())
        })
    }
}
// Pending signals belong to this bus connection. Never replay old actions on a
// new bus; accepted signal delivery remains best effort across a disconnect.
struct Online(Arc<Shared>);
impl Online {
    fn new(shared: Arc<Shared>) -> Self {
        shared.state.lock().unwrap().online = true;
        Self(shared)
    }
}
impl Drop for Online {
    fn drop(&mut self) {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.online = false;
        state.signals.clear();
        state.inbox.retire_active();
        state.revision = state.revision.wrapping_add(1);
        state.dirty = true;
        self.0.changed.notify_all();
    }
}
pub struct FreedesktopNotifications(Arc<Shared>);
impl Default for FreedesktopNotifications {
    fn default() -> Self {
        Self::with_history(None)
    }
}
impl FreedesktopNotifications {
    pub fn with_history(history: Option<Arc<dyn NotificationHistoryPort>>) -> Self {
        Self(Arc::new(Shared {
            state: Mutex::new(Store::default()),
            changed: Condvar::new(),
            epoch: Instant::now(),
            history,
            worker: Mutex::new(None),
        }))
    }
}
struct Server(Weak<Shared>);
fn bounded(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .scan(0usize, |bytes, c| {
            *bytes += c.len_utf8();
            Some((*bytes <= max).then_some(c))
        })
        .take_while(Option::is_some)
        .flatten()
        .collect()
}
#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<&str> {
        vec![
            "body",
            "body-markup",
            "body-hyperlinks",
            "body-images",
            "icon-static",
            "actions",
        ]
    }
    fn get_server_information(&self) -> (&str, &str, &str, &str) {
        ("Lucent", "Lucent", env!("CARGO_PKG_VERSION"), "1.2")
    }
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> zbus::fdo::Result<u32> {
        let shared = self
            .0
            .upgrade()
            .ok_or_else(|| zbus::fdo::Error::Failed("Server stopped".into()))?;
        let flag = |name| {
            hints
                .get(name)
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false)
        };
        let mut action_ids = std::collections::BTreeSet::new();
        let note = Notification {
            id: replaces_id,
            revision: 0,
            received_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis().min(u64::MAX as u128) as u64),
            desktop_entry: hints
                .get("desktop-entry")
                .and_then(|v| <&str>::try_from(v).ok().map(str::to_owned))
                .and_then(|id| {
                    AppId::new(if id.ends_with(".desktop") {
                        id
                    } else {
                        format!("{id}.desktop")
                    })
                    .ok()
                }),
            app: bounded(app_name, 128),
            summary: bounded(summary, 256),
            body: bounded(body, 4096),
            image: image::select(&hints, app_icon),
            actions: actions
                .as_chunks::<2>()
                .0
                .iter()
                .take(8)
                .filter(|a| !a[0].is_empty() && a[0].len() <= 128)
                .map(|a| NotificationAction {
                    id: a[0].clone(),
                    label: bounded(&a[1], 64),
                })
                .filter(|a| !a.id.is_empty() && action_ids.insert(a.id.clone()))
                .collect(),
            critical: hints.get("urgency").and_then(|v| u8::try_from(v).ok()) == Some(2),
            resident: flag("resident"),
            transient: flag("transient"),
            timeout_ms: expire_timeout,
        };
        shared.mutate(|state| {
            // Receiving can evict one active notification. Admit its worst-case
            // signal before assigning an ID or changing the inbox.
            state.admit(1).map_err(AdmissionError::dbus)?;
            let (id, evicted) = state
                .inbox
                .receive(note, shared.epoch.elapsed().as_millis() as u64)
                .map_err(|_| {
                    zbus::fdo::Error::InvalidArgs("Notification payload is invalid".into())
                })?;
            if let Some(id) = evicted {
                state
                    .signals
                    .push_back(Signal::Closed(id, CloseReason::Undefined));
            }
            Ok(id)
        })
    }
    fn close_notification(&self, id: u32) -> zbus::fdo::Result<()> {
        let shared = self
            .0
            .upgrade()
            .ok_or_else(|| zbus::fdo::Error::Disconnected("Server stopped".into()))?;
        shared
            .close(id, CloseReason::Requested)
            .map_err(AdmissionError::dbus)
    }
    // Declared for introspection; the delivery loop emits these outside the inbox mutex.
    #[zbus(signal)]
    async fn notification_closed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn action_invoked(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}
impl FreedesktopNotifications {
    fn load_history(&self) {
        if self.0.history.is_none() {
            return;
        }
        let mut worker = self.0.worker.lock().unwrap();
        if worker.is_none() {
            match storage::Worker::start(
                self.0.history.as_ref().unwrap().clone(),
                Arc::downgrade(&self.0),
            ) {
                Ok(started) => *worker = Some(started),
                Err(_) => {
                    let mut state = self.0.state.lock().unwrap();
                    state.loaded = true;
                    state.reload = false;
                    state.history_status = NotificationHistoryStatus::ReadFailed;
                    self.0.changed.notify_all();
                    return;
                }
            }
        }
        let mut state = self.0.state.lock().unwrap();
        if (state.loaded && !state.reload) || state.load_pending.is_some() {
            return;
        }
        let Some(id) = state.io_serial.checked_add(1) else {
            state.loaded = true;
            state.reload = false;
            state.history_status = NotificationHistoryStatus::ReadFailed;
            return;
        };
        state.io_serial = id;
        state.reload = false;
        state.load_pending = Some(id);
        state.history_status = NotificationHistoryStatus::Loading;
        state.revision = state.revision.wrapping_add(1);
        worker.as_ref().unwrap().submit(storage::Request::Load(id));
    }
    fn persist_history(&self) {
        let worker = self.0.worker.lock().unwrap();
        let Some(worker) = worker.as_ref() else {
            return;
        };
        let mut state = self.0.state.lock().unwrap();
        if !state.writable || !state.dirty {
            return;
        }
        let Some(id) = state.io_serial.checked_add(1) else {
            state.history_status = NotificationHistoryStatus::WriteFailed;
            state.dirty = false;
            return;
        };
        state.io_serial = id;
        let archive = state.inbox.archive();
        state.dirty = false;
        state.save_pending = Some(id);
        if state.history_status != NotificationHistoryStatus::Saving {
            state.history_status = NotificationHistoryStatus::Saving;
            state.revision = state.revision.wrapping_add(1);
        }
        worker.submit(storage::Request::Save(id, archive));
    }
    // Private connection selection keeps protocol testing out of the domain port.
    fn watch_with(
        &self,
        emit: &mut dyn FnMut(Result<NotificationSnapshot>),
        stop: &dyn StopSignal,
        connect: impl Fn() -> zbus::Result<Builder<'static>>,
    ) {
        self.load_history();
        // Bootstrap the persisted revision counter before admitting a sender.
        // Waiting does not perform file IO or hold the inbox mutex, and stop
        // remains observable even when the storage call stalls.
        while self.0.history.is_some() && !stop.cancelled() {
            let state = self.0.state.lock().unwrap();
            if state.loaded {
                break;
            }
            drop(
                self.0
                    .changed
                    .wait_timeout(state, Duration::from_millis(100))
                    .unwrap(),
            );
        }
        while !stop.cancelled() {
            let connection = connect()
                // Activation owns shell replacement. The delivery adapter must
                // not steal another server's name or silently lose its own.
                .map(|b| {
                    b.replace_existing_names(false)
                        .allow_name_replacements(false)
                })
                .and_then(|b| b.serve_at(PATH, Server(Arc::downgrade(&self.0))))
                .and_then(|b| b.name(NAME))
                .and_then(|b| b.build());
            let connection = match connection {
                Ok(c) => c,
                Err(_) => {
                    emit(Err(DomainError::Unavailable(
                        "Notification bus name unavailable".into(),
                    )));
                    stop.wait(Duration::from_secs(2));
                    continue;
                }
            };
            let online = Online::new(self.0.clone());
            let mut revision = u64::MAX;
            while !stop.cancelled() {
                if connection.is_closed() {
                    break;
                }
                if self.0.state.lock().unwrap().reload {
                    self.load_history();
                }
                self.0
                    .state
                    .lock()
                    .unwrap()
                    .expire(self.0.epoch.elapsed().as_millis() as u64);
                self.persist_history();
                let mut state = self.0.state.lock().unwrap();
                let snapshot = if revision != state.revision {
                    revision = state.revision;
                    let mut snapshot = state.inbox.snapshot();
                    snapshot.history_status = state.history_status;
                    Some(snapshot)
                } else {
                    None
                };
                let signals: Vec<_> = state.signals.drain(..).collect();
                drop(state);
                let mut disconnected = false;
                for signal in signals {
                    if stop.cancelled() {
                        break;
                    }
                    let result = match signal {
                        Signal::Closed(id, reason) => connection.emit_signal(
                            None::<&str>,
                            PATH,
                            NAME,
                            "NotificationClosed",
                            &(id, reason as u32),
                        ),
                        Signal::Action(id, action) => connection.emit_signal(
                            None::<&str>,
                            PATH,
                            NAME,
                            "ActionInvoked",
                            &(id, action),
                        ),
                    };
                    if result.is_err() {
                        disconnected = true;
                        break;
                    }
                }
                if disconnected || stop.cancelled() {
                    break;
                }
                if let Some(snapshot) = snapshot {
                    emit(Ok(snapshot));
                }
                let state = self.0.state.lock().unwrap();
                if state.revision == revision {
                    drop(
                        self.0
                            .changed
                            .wait_timeout(state, Duration::from_millis(100))
                            .unwrap(),
                    );
                }
            }
            drop(online);
            drop(connection);
            self.persist_history();
            if !stop.cancelled() {
                let snapshot = {
                    let state = self.0.state.lock().unwrap();
                    let mut value = state.inbox.snapshot();
                    value.history_status = state.history_status;
                    value
                };
                emit(Ok(snapshot));
                emit(Err(DomainError::Unavailable(
                    "Notification bus disconnected".into(),
                )));
            }
        }
    }
}
impl NotificationPort for FreedesktopNotifications {
    fn watch(&self, emit: &mut dyn FnMut(Result<NotificationSnapshot>), stop: &dyn StopSignal) {
        self.watch_with(emit, stop, Builder::session);
    }
    fn dismiss(&self, target: NotificationTarget) -> Result<()> {
        self.0.mutate(|state| {
            if !state.inbox.matches(target) {
                return Err(DomainError::Invalid(
                    "Notification changed or expired".into(),
                ));
            }
            state.admit(1).map_err(AdmissionError::domain)?;
            state.inbox.close(target.id());
            state
                .signals
                .push_back(Signal::Closed(target.id(), CloseReason::Dismissed));
            Ok(())
        })
    }
    fn invoke(&self, target: NotificationTarget, action: &str) -> Result<()> {
        let id = target.id();
        self.0.mutate(|s| {
            if !s.online {
                return Err(AdmissionError::Offline.domain());
            }
            if !s.inbox.matches(target) {
                return Err(DomainError::Invalid(
                    "Notification changed or expired".into(),
                ));
            }
            let note = s
                .inbox
                .get(id)
                .ok_or_else(|| DomainError::Invalid("Notification expired".into()))?;
            if !note.actions.iter().any(|a| a.id == action) {
                return Err(DomainError::Invalid("Unknown notification action".into()));
            }
            let resident = note.resident;
            s.admit(if resident { 1 } else { 2 })
                .map_err(AdmissionError::domain)?;
            s.signals.push_back(Signal::Action(id, action.into()));
            if !resident {
                s.inbox.close(id);
                s.signals
                    .push_back(Signal::Closed(id, CloseReason::Dismissed));
            }
            Ok(())
        })
    }
    fn set_do_not_disturb(&self, enabled: bool) -> Result<()> {
        self.0.mutate(|s| {
            s.inbox.dnd(enabled);
            s.dnd_changed = true;
            Ok(())
        })
    }
    fn clear_history(&self) -> Result<()> {
        if let Some(worker) = self.0.worker.lock().unwrap().as_ref() {
            worker.cancel_load();
        }
        self.0.mutate(|s| {
            s.inbox.clear_history();
            // Explicit Clear authorizes replacing even an unreadable archive.
            s.writable = true;
            s.reload = false;
            s.load_pending = None;
            s.loaded = true;
            Ok(())
        })
    }
    fn retry_history(&self) -> Result<()> {
        self.0.mutate(|state| {
            if matches!(
                state.history_status,
                NotificationHistoryStatus::ReadFailed | NotificationHistoryStatus::Loading
            ) {
                state.reload = true;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
