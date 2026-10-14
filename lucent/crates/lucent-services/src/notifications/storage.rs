//! One ordered file worker: one pending load, one latest save, one active call.
//! It holds no bus connection, UI state, callback queue or renderer resource.
use super::Shared;
use lucent_domain::*;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex, Weak},
    thread,
};
pub(super) enum Request {
    Load(u64),
    Save(u64, NotificationArchive),
}
pub(super) enum Completion {
    Loaded(u64, Result<NotificationArchive>),
    Saved(u64, Result<()>),
}
#[derive(Default)]
struct Queue {
    load: Option<Request>,
    save: Option<Request>,
    stop: bool,
}
struct Mailbox {
    queue: Mutex<Queue>,
    wake: Condvar,
}
pub(super) struct Worker {
    mailbox: Arc<Mailbox>,
    _thread: thread::JoinHandle<()>,
}
impl Worker {
    pub(super) fn start(
        adapter: Arc<dyn NotificationHistoryPort>,
        shared: Weak<Shared>,
    ) -> std::io::Result<Self> {
        let mailbox = Arc::new(Mailbox {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
        });
        let owner = mailbox.clone();
        let thread = thread::Builder::new()
            .name("notification-history".into())
            .spawn(move || {
                loop {
                    let request = {
                        let mut queue = owner.queue.lock().unwrap();
                        while !queue.stop && queue.load.is_none() && queue.save.is_none() {
                            queue = owner.wake.wait(queue).unwrap();
                        }
                        if queue.stop {
                            return;
                        }
                        queue.load.take().or_else(|| queue.save.take()).unwrap()
                    };
                    let completion = match request {
                        Request::Load(id) => Completion::Loaded(
                            id,
                            catch_unwind(AssertUnwindSafe(|| adapter.load())).unwrap_or_else(
                                |_| Err(DomainError::Failed("History loader failed".into())),
                            ),
                        ),
                        Request::Save(id, archive) => Completion::Saved(
                            id,
                            catch_unwind(AssertUnwindSafe(|| adapter.save(&archive)))
                                .unwrap_or_else(|_| {
                                    Err(DomainError::Failed("History writer failed".into()))
                                }),
                        ),
                    };
                    if let Some(shared) = shared.upgrade() {
                        shared.storage_complete(completion);
                    }
                }
            })?;
        Ok(Self {
            mailbox,
            _thread: thread,
        })
    }
    pub(super) fn submit(&self, request: Request) {
        let mut queue = self.mailbox.queue.lock().unwrap();
        if queue.stop {
            return;
        }
        match request {
            Request::Load(_) => queue.load = Some(request),
            Request::Save(..) => queue.save = Some(request),
        }
        self.mailbox.wake.notify_one();
    }
    pub(super) fn cancel_load(&self) {
        self.mailbox.queue.lock().unwrap().load = None;
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let mut queue = self.mailbox.queue.lock().unwrap();
        queue.stop = true;
        queue.load = None;
        queue.save = None;
        self.mailbox.wake.notify_all();
        // A filesystem call cannot be cancelled safely. Do not block a caller
        // or retain the bus while it completes. The worker owns only its port
        // and bounded request; its weak completion cannot resurrect Shared.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};
    struct Adapter {
        calls: mpsc::Sender<u64>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }
    impl NotificationHistoryPort for Adapter {
        fn load(&self) -> Result<NotificationArchive> {
            Ok(NotificationArchive::default())
        }
        fn save(&self, archive: &NotificationArchive) -> Result<()> {
            self.calls.send(archive.last_revision).unwrap();
            if archive.last_revision == 1 {
                let mut held = self.gate.0.lock().unwrap();
                while *held {
                    held = self.gate.1.wait(held).unwrap();
                }
            }
            Ok(())
        }
    }
    struct Release(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Release {
        fn drop(&mut self) {
            *self.0.0.lock().unwrap() = false;
            self.0.1.notify_all();
        }
    }
    #[test]
    fn pending_saves_coalesce_to_latest_while_one_call_is_active() {
        let (tx, rx) = mpsc::channel();
        let gate = Arc::new((Mutex::new(true), Condvar::new()));
        let release = Release(gate.clone());
        let worker = Worker::start(Arc::new(Adapter { calls: tx, gate }), Weak::new()).unwrap();
        worker.submit(Request::Save(
            1,
            NotificationArchive {
                last_revision: 1,
                ..Default::default()
            },
        ));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        for revision in 2..=1000 {
            worker.submit(Request::Save(
                revision,
                NotificationArchive {
                    last_revision: revision,
                    ..Default::default()
                },
            ));
        }
        let queued = worker.mailbox.queue.lock().unwrap();
        assert!(queued.load.is_none());
        assert!(matches!(queued.save, Some(Request::Save(1000, _))));
        drop(queued);
        drop(release);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1000);
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    }
    #[test]
    fn retiring_worker_discards_queued_data_without_joining_a_blocked_call() {
        let (tx, rx) = mpsc::channel();
        let gate = Arc::new((Mutex::new(true), Condvar::new()));
        let release = Release(gate.clone());
        let worker = Worker::start(Arc::new(Adapter { calls: tx, gate }), Weak::new()).unwrap();
        worker.submit(Request::Save(
            1,
            NotificationArchive {
                last_revision: 1,
                ..Default::default()
            },
        ));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        worker.submit(Request::Save(
            2,
            NotificationArchive {
                last_revision: 2,
                ..Default::default()
            },
        ));
        let (done, retired) = mpsc::channel();
        let retiring = std::thread::spawn(move || {
            drop(worker);
            done.send(()).unwrap();
        });
        let completed = retired.recv_timeout(Duration::from_secs(1)).is_ok();
        drop(release);
        retiring.join().unwrap();
        assert!(completed, "retirement joined a blocked call");
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_err());
    }
}
