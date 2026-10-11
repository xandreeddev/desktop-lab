//! Freedesktop delivery adapter. No rendering or application message types.
use lucent_domain::*;
use lucent_usecases::notifications::NotificationInbox;
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
        }
    }
}
struct Shared {
    state: Mutex<Store>,
    changed: Condvar,
    epoch: Instant,
}
impl Shared {
    fn mutate<T, E>(
        &self,
        f: impl FnOnce(&mut Store) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let mut state = self.state.lock().unwrap();
        let result = f(&mut state)?;
        state.revision = state.revision.wrapping_add(1);
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
        self.0.changed.notify_all();
    }
}
pub struct FreedesktopNotifications(Arc<Shared>);
impl Default for FreedesktopNotifications {
    fn default() -> Self {
        Self(Arc::new(Shared {
            state: Mutex::new(Store::default()),
            changed: Condvar::new(),
            epoch: Instant::now(),
        }))
    }
}
struct Server(Weak<Shared>);
fn bounded(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max)
        .collect()
}
#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<&str> {
        vec!["body", "actions"]
    }
    fn get_server_information(&self) -> (&str, &str, &str, &str) {
        ("Lucent", "Lucent", env!("CARGO_PKG_VERSION"), "1.2")
    }
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        _app_icon: &str,
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
        let note = Notification {
            id: replaces_id,
            app: bounded(app_name, 128),
            summary: bounded(summary, 256),
            body: bounded(body, 4096),
            actions: actions
                .as_chunks::<2>()
                .0
                .iter()
                .take(8)
                .map(|a| NotificationAction {
                    id: bounded(&a[0], 128),
                    label: bounded(&a[1], 64),
                })
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
                .receive(note, shared.epoch.elapsed().as_millis() as u64);
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
    // Private connection selection keeps protocol testing out of the domain port.
    fn watch_with(
        &self,
        emit: &mut dyn FnMut(Result<NotificationSnapshot>),
        stop: &dyn StopSignal,
        connect: impl Fn() -> zbus::Result<Builder<'static>>,
    ) {
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
                let mut state = self.0.state.lock().unwrap();
                state.expire(self.0.epoch.elapsed().as_millis() as u64);
                let snapshot = if revision != state.revision {
                    revision = state.revision;
                    Some(state.inbox.snapshot())
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
            if !stop.cancelled() {
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
    fn dismiss(&self, id: u32) -> Result<()> {
        self.0
            .close(id, CloseReason::Dismissed)
            .map_err(AdmissionError::domain)
    }
    fn invoke(&self, id: u32, action: &str) -> Result<()> {
        self.0.mutate(|s| {
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
            Ok(())
        })
    }
    fn clear_history(&self) -> Result<()> {
        self.0.mutate(|s| {
            s.inbox.clear_history();
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
