//! Bounded ordered workers. Client queue names express ordering, never OS policy.
use lucent_api::{Cancellation, RuntimeFailure, ScheduledTask, TaskFailure};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, mpsc},
};

pub(super) enum Completion<M> {
    Message(M),
    Failure(RuntimeFailure),
}
pub(super) struct Rejected<M> {
    pub failure: RuntimeFailure,
    task: ScheduledTask<M>,
}
impl<M> std::fmt::Debug for Rejected<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.failure.fmt(f)
    }
}
fn failure_completion<M>(
    failure: RuntimeFailure,
    handler: Option<TaskFailure<M>>,
) -> Completion<M> {
    let Some(handler) = handler else {
        return Completion::Failure(failure);
    };
    match catch_unwind(AssertUnwindSafe(|| handler(failure.clone()))) {
        Ok(message) => Completion::Message(message),
        Err(_) => Completion::Failure(RuntimeFailure {
            source: failure.source,
            reason: "task failure mapper panicked".into(),
        }),
    }
}
impl<M> Rejected<M> {
    pub fn complete(mut self) -> Option<Completion<M>> {
        if self.task.cancellation.cancelled() {
            return None;
        }
        Some(failure_completion(
            self.failure,
            self.task.take_failure_handler(),
        ))
    }
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
    pub fn submit(&mut self, task: ScheduledTask<M>) -> Result<(), Rejected<M>> {
        if self.stopping.cancelled() || task.cancellation.cancelled() {
            return Ok(());
        }
        let queue = task.queue;
        let failure = |reason: &str| RuntimeFailure {
            source: format!("effect queue {queue}"),
            reason: reason.into(),
        };
        if !self.workers.contains_key(queue) {
            if self.workers.len() == self.max_workers {
                return Err(Rejected {
                    task,
                    failure: failure("worker limit reached"),
                });
            }
            let (sender, receiver) = mpsc::sync_channel::<ScheduledTask<M>>(self.capacity);
            let active = Arc::new(Mutex::new(None));
            let current = active.clone();
            let completion = self.completion.clone();
            let stopping = self.stopping.clone();
            let started = std::thread::Builder::new()
                .name(format!("effect-{queue}"))
                .spawn(move || {
                    for mut task in receiver {
                        if stopping.cancelled() || task.cancellation.cancelled() {
                            continue;
                        }
                        let cancellation = task.cancellation.clone();
                        let on_failure = task.take_failure_handler();
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
                            Some(Err(_)) => completion(failure_completion(
                                RuntimeFailure {
                                    source: format!("effect queue {queue}"),
                                    // A panic payload may contain private input. Do not expose it over IPC.
                                    reason: "worker task panicked".into(),
                                },
                                on_failure,
                            )),
                            None => {}
                        }
                    }
                });
            if let Err(error) = started {
                return Err(Rejected {
                    task,
                    failure: failure(&format!("cannot start worker: {error}")),
                });
            }
            self.workers.insert(queue, Worker { sender, active });
        }
        match self.workers[queue].sender.try_send(task) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(task)) => Err(Rejected {
                task,
                failure: failure("queue is full; work was not accepted"),
            }),
            Err(mpsc::TrySendError::Disconnected(task)) => Err(Rejected {
                task,
                failure: failure("worker disconnected"),
            }),
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
        let cancellation = cancelled.cancellation.clone();
        executor.submit(cancelled).unwrap();
        cancellation.cancel();
        assert!(
            executor
                .submit(task("one", || 2))
                .unwrap_err()
                .failure
                .reason
                .contains("full")
        );
        assert!(
            executor
                .submit(task("two", || 2))
                .unwrap_err()
                .failure
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

    #[test]
    fn shared_queue_failures_reach_the_exact_task_and_unaccepted_work_does_not_run() {
        let (tx, rx) = mpsc::channel();
        let mut executor = Executor::new(move |v| {
            let _ = tx.send(v);
        });
        executor.max_workers = 1;
        let mut effects = Effects::default();
        effects.task_on_with_failure(
            "shared",
            || panic!("private fixture payload"),
            |failure| {
                assert!(!failure.reason.contains("private fixture"));
                41
            },
        );
        executor.submit(effects.tasks.remove(0)).unwrap();
        executor.submit(task("shared", || 7)).unwrap();
        assert!(matches!(next(&rx), Completion::Message(41)));
        assert!(matches!(next(&rx), Completion::Message(7)));
        effects.task_on_with_failure("another", || panic!("unaccepted body ran"), |_| 42);
        let rejected = executor.submit(effects.tasks.remove(0)).unwrap_err();
        assert!(matches!(rejected.complete(), Some(Completion::Message(42))));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn cancellation_before_admission_or_after_rejection_drops_work_and_failure_mapping() {
        let mut executor = Executor::new(|_: Completion<u32>| panic!("cancelled work completed"));
        executor.max_workers = 0;
        let mut effects = Effects::default();
        effects.task_on_with_failure(
            "unused",
            || panic!("cancelled body ran"),
            |_| panic!("cancelled failure mapper ran"),
        );
        effects.tasks[0].cancellation.cancel();
        executor.submit(effects.tasks.remove(0)).unwrap();
        assert!(executor.workers.is_empty());

        effects.task_on_with_failure(
            "rejected",
            || panic!("rejected body ran"),
            |_| panic!("retired failure mapper ran"),
        );
        let cancellation = effects.tasks[0].cancellation.clone();
        let rejected = executor.submit(effects.tasks.remove(0)).unwrap_err();
        cancellation.cancel();
        assert!(rejected.complete().is_none());
    }

    #[test]
    fn cancellation_skips_failure_mapping_and_a_panicking_mapper_uses_safe_fallback() {
        let (tx, rx) = mpsc::channel();
        let mut executor = Executor::new(move |v| {
            let _ = tx.send(v);
        });
        let mut effects = Effects::default();
        effects.task_on_with_failure(
            "shared",
            || panic!("cancelled body"),
            |_| panic!("cancelled mapper"),
        );
        effects.tasks[0].cancellation.cancel();
        executor.submit(effects.tasks.remove(0)).unwrap();
        effects.task_on_with_failure(
            "shared",
            || panic!("fixture body"),
            |_| panic!("private mapper payload"),
        );
        executor.submit(effects.tasks.remove(0)).unwrap();
        executor.submit(task("shared", || 7)).unwrap();
        let Completion::Failure(failure) = next(&rx) else {
            panic!("expected safe failure fallback");
        };
        assert_eq!(failure.reason, "task failure mapper panicked");
        assert!(matches!(next(&rx), Completion::Message(7)));
        assert!(rx.try_recv().is_err());
    }
}
