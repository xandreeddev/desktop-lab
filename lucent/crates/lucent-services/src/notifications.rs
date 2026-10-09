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
enum Signal {
    Closed(u32, CloseReason),
    Action(u32, String),
}
#[derive(Default)]
struct Store {
    inbox: NotificationInbox,
    signals: VecDeque<Signal>,
    revision: u64,
}
struct Shared {
    state: Mutex<Store>,
    changed: Condvar,
    epoch: Instant,
}
impl Shared {
    fn mutate<T>(&self, f: impl FnOnce(&mut Store) -> T) -> T {
        let mut state = self.state.lock().unwrap();
        let result = f(&mut state);
        state.revision = state.revision.wrapping_add(1);
        self.changed.notify_one();
        result
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
        Ok(shared.mutate(|state| {
            let (id, evicted) = state
                .inbox
                .receive(note, shared.epoch.elapsed().as_millis() as u64);
            if let Some(id) = evicted {
                state
                    .signals
                    .push_back(Signal::Closed(id, CloseReason::Undefined));
            }
            id
        }))
    }
    fn close_notification(&self, id: u32) {
        if let Some(shared) = self.0.upgrade() {
            shared.mutate(|state| {
                if state.inbox.close(id) {
                    state
                        .signals
                        .push_back(Signal::Closed(id, CloseReason::Requested));
                }
            });
        }
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
impl NotificationPort for FreedesktopNotifications {
    fn watch(&self, emit: &mut dyn FnMut(Result<NotificationSnapshot>), stop: &dyn StopSignal) {
        while !stop.cancelled() {
            let connection = Builder::session()
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
            let mut revision = u64::MAX;
            while !stop.cancelled() {
                let mut state = self.0.state.lock().unwrap();
                for id in state.inbox.due(self.0.epoch.elapsed().as_millis() as u64) {
                    state.inbox.close(id);
                    state
                        .signals
                        .push_back(Signal::Closed(id, CloseReason::Expired));
                    state.revision = state.revision.wrapping_add(1);
                }
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
                    disconnected |= result.is_err();
                }
                if disconnected {
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
        }
    }
    fn dismiss(&self, id: u32) -> Result<()> {
        self.0.mutate(|s| {
            if s.inbox.close(id) {
                s.signals
                    .push_back(Signal::Closed(id, CloseReason::Dismissed));
            }
        });
        Ok(())
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
        self.0.mutate(|s| s.inbox.dnd(enabled));
        Ok(())
    }
    fn clear_history(&self) -> Result<()> {
        self.0.mutate(|s| s.inbox.clear_history());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Stop(AtomicBool);
    impl StopSignal for Stop {
        fn cancelled(&self) -> bool {
            self.0.load(Ordering::Relaxed)
        }
        fn wait(&self, d: Duration) {
            std::thread::sleep(d);
        }
    }
    #[test]
    #[ignore = "Run under a private dbus-run-session; must never claim a real desktop notification name"]
    fn notification_protocol() {
        let server = Arc::new(FreedesktopNotifications::default());
        let stop = Arc::new(Stop(AtomicBool::new(false)));
        let (s, r) = std::sync::mpsc::channel();
        let worker = {
            let server = server.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                server.watch(
                    &mut |v| {
                        let _ = s.send(v);
                    },
                    stop.as_ref(),
                )
            })
        };
        r.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        let connection = zbus::blocking::Connection::session().unwrap();
        let proxy = zbus::blocking::Proxy::new(&connection, NAME, PATH, NAME).unwrap();
        let info: (String, String, String, String) =
            proxy.call("GetServerInformation", &()).unwrap();
        assert_eq!(info.0, "Lucent");
        let mut actions = proxy.receive_signal("ActionInvoked").unwrap();
        let notify = |replaces: u32, timeout: i32| -> u32 {
            proxy
                .call(
                    "Notify",
                    &(
                        "Fixture",
                        replaces,
                        "",
                        "A real protocol message",
                        "Body",
                        vec!["default", "Open"],
                        HashMap::<String, OwnedValue>::new(),
                        timeout,
                    ),
                )
                .unwrap()
        };
        let id = notify(0, 0);
        assert_eq!(notify(id, 0), id);
        assert!(server.invoke(id, "invented").is_err());
        server.invoke(id, "default").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let message = actions.next().unwrap();
            let data: (u32, String) = message.body().deserialize().unwrap();
            tx.send(data).unwrap();
        });
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            (id, "default".into())
        );
        let expires = notify(0, 30);
        std::thread::sleep(Duration::from_millis(250));
        let state = server.0.state.lock().unwrap().inbox.snapshot();
        assert!(!state.active.iter().any(|n| n.id == expires));
        assert!(state.history.iter().any(|n| n.id == expires));
        stop.0.store(true, Ordering::Relaxed);
        worker.join().unwrap();
    }
}
