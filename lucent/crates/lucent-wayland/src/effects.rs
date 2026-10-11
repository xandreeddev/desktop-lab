//! Bounded ordered workers. Client queue names express ordering, never OS policy.
use lucent_api::{Cancellation, RuntimeFailure, ScheduledTask};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, mpsc},
};

pub(super) enum Completion<M> {
    Message(M),
    Failure(RuntimeFailure),
}
struct Worker<M> {
    sender: mpsc::SyncSender<ScheduledTask<M>>,
    active: Arc<Mutex<Option<Cancellation>>>,
}
pub(super) struct Executor<M> {
    workers: BTreeMap<&'static str, Worker<M>>,
    completion: Arc<dyn Fn(Completion<M>) + Send + Sync>,
    stopping: Cancellation,
    capacity: usize,
    max_workers: usize,
}
impl<M: Send + 'static> Executor<M> {
    pub fn new(completion: impl Fn(Completion<M>) + Send + Sync + 'static) -> Self {
        Self {
            workers: BTreeMap::new(),
            completion: Arc::new(completion),
            stopping: Cancellation::default(),
            capacity: 64,
            max_workers: 8,
        }
    }
    pub fn submit(&mut self, task: ScheduledTask<M>) -> Result<(), RuntimeFailure> {
        let queue = task.queue;
        let failure = |reason: &str| RuntimeFailure {
            source: format!("effect queue {queue}"),
            reason: reason.into(),
        };
        if !self.workers.contains_key(queue) {
            if self.workers.len() == self.max_workers {
                return Err(failure("worker limit reached"));
            }
            let (sender, receiver) = mpsc::sync_channel::<ScheduledTask<M>>(self.capacity);
            let active = Arc::new(Mutex::new(None));
            let current = active.clone();
            let completion = self.completion.clone();
            let stopping = self.stopping.clone();
            std::thread::Builder::new()
                .name(format!("effect-{queue}"))
                .spawn(move || {
                    for task in receiver {
                        if stopping.cancelled() || task.cancellation.cancelled() {
                            continue;
                        }
                        let cancellation = task.cancellation.clone();
                        *current.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(cancellation.clone());
                        // Close the race with Executor::drop after the first check.
                        if stopping.cancelled() {
                            cancellation.cancel();
                        }
                        let result = if cancellation.cancelled() {
                            None
                        } else {
                            Some(catch_unwind(AssertUnwindSafe(|| task.run())))
                        };
                        *current.lock().unwrap_or_else(|e| e.into_inner()) = None;
                        if cancellation.cancelled() || stopping.cancelled() {
                            continue;
                        }
                        match result {
                            Some(Ok(message)) => completion(Completion::Message(message)),
                            Some(Err(_)) => completion(Completion::Failure(RuntimeFailure {
                                source: format!("effect queue {queue}"),
                                // A panic payload may contain private input. Do not expose it over IPC.
                                reason: "worker task panicked".into(),
                            })),
                            None => {}
                        }
                    }
                })
                .map_err(|error| failure(&format!("cannot start worker: {error}")))?;
            self.workers.insert(queue, Worker { sender, active });
        }
        match self.workers[queue].sender.try_send(task) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => {
                Err(failure("queue is full; work was not accepted"))
            }
            Err(mpsc::TrySendError::Disconnected(_)) => Err(failure("worker disconnected")),
        }
    }
}
impl<M> Drop for Executor<M> {
    fn drop(&mut self) {
        self.stopping.cancel();
        for worker in self.workers.values() {
            if let Some(cancel) = worker
                .active
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
            {
                cancel.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_api::Effects;
    use std::time::Duration;
    fn task(
        queue: &'static str,
        make: impl FnOnce() -> u32 + Send + 'static,
    ) -> ScheduledTask<u32> {
        let mut effects = Effects::default();
        effects.task_on(queue, make);
        effects.tasks.pop().unwrap()
    }
    fn next(receiver: &mpsc::Receiver<Completion<u32>>) -> Completion<u32> {
        receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("worker completion")
    }
    #[test]
    fn independent_work_runs_while_ordered_queue_is_blocked() {
        let (tx, rx) = mpsc::channel();
        let mut executor = Executor::new(move |value| {
            let _ = tx.send(value);
        });
        let (release, gate) = mpsc::channel();
        executor
            .submit(task("settings", move || {
                gate.recv().unwrap();
                1
            }))
            .unwrap();
        executor.submit(task("settings", || 2)).unwrap();
        executor.submit(task("session", || 3)).unwrap();
        assert!(matches!(next(&rx), Completion::Message(3)));
        release.send(()).unwrap();
        assert!(matches!(next(&rx), Completion::Message(1)));
        assert!(matches!(next(&rx), Completion::Message(2)));
    }
    #[test]
    fn bounds_reject_without_blocking_and_cancellation_skips_queued_work() {
        let (tx, rx) = mpsc::channel();
        let mut executor = Executor::new(move |value| {
            let _ = tx.send(value);
        });
        executor.capacity = 1;
        executor.max_workers = 1;
        let (release, gate) = mpsc::channel();
        let (started, running) = mpsc::channel();
        executor
            .submit(task("one", move || {
                started.send(()).unwrap();
                gate.recv().unwrap();
                1
            }))
            .unwrap();
        running.recv_timeout(Duration::from_secs(2)).unwrap();
        let cancelled = task("one", || panic!("cancelled work executed"));
        cancelled.cancellation.cancel();
        executor.submit(cancelled).unwrap();
        assert!(
            executor
                .submit(task("one", || 2))
                .unwrap_err()
                .reason
                .contains("full")
        );
        assert!(
            executor
                .submit(task("two", || 2))
                .unwrap_err()
                .reason
                .contains("limit")
        );
        release.send(()).unwrap();
        assert!(matches!(next(&rx), Completion::Message(1)));
        drop(executor);
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_err());
    }
    #[test]
    fn panics_are_reported_and_do_not_silently_destroy_a_worker() {
        let (tx, rx) = mpsc::channel();
        let mut executor = Executor::new(move |value| {
            let _ = tx.send(value);
        });
        executor
            .submit(task("one", || panic!("test failure")))
            .unwrap();
        executor.submit(task("one", || 7)).unwrap();
        assert!(matches!(next(&rx), Completion::Failure(_)));
        assert!(matches!(next(&rx), Completion::Message(7)));
    }
    #[test]
    fn shutdown_wakes_cooperative_work_without_polling() {
        let (started, running) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let cancel = Cancellation::default();
        let work_cancel = cancel.clone();
        let mut effects = Effects::default();
        effects.task_cancellable_on("one", cancel, move || {
            started.send(()).unwrap();
            work_cancel.sleep(Duration::from_secs(60));
            finished.send(()).unwrap();
        });
        let mut executor = Executor::new(|_| {});
        executor.submit(effects.tasks.pop().unwrap()).unwrap();
        running.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(executor);
        done.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}
