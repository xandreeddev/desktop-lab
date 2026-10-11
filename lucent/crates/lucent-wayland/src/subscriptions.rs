//! Subscription lifetimes, including cancelled streams which have not yet exited.
use super::{RuntimeEvent, delivery::Delivery};
use lucent_api::{Cancellation, Emitter, RuntimeFailure, Subscription};
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const MAX_WORKERS: usize = 64;
struct Slot(Arc<AtomicUsize>);
impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
/// Queued events from removed/replaced streams must not update a new generation.
pub(super) struct Event<M> {
    cancellation: Cancellation,
    result: Result<M, RuntimeFailure>,
}
impl<M> Event<M> {
    pub fn into_live(self) -> Option<Result<M, RuntimeFailure>> {
        (!self.cancellation.cancelled()).then_some(self.result)
    }
}
pub(super) struct Workers {
    active: BTreeMap<String, Cancellation>,
    live: Arc<AtomicUsize>,
    limit: usize,
}
impl Default for Workers {
    fn default() -> Self {
        Self {
            active: BTreeMap::new(),
            live: Arc::new(AtomicUsize::new(0)),
            limit: MAX_WORKERS,
        }
    }
}
impl Workers {
    pub fn reconcile<M: Send + 'static>(
        &mut self,
        wanted: Vec<Subscription<M>>,
        delivery: &Delivery<RuntimeEvent<M>>,
    ) -> Result<(), RuntimeFailure> {
        let mut ids = BTreeSet::new();
        for subscription in &wanted {
            if !ids.insert(subscription.id.as_str()) {
                return Err(RuntimeFailure {
                    source: "subscriptions".into(),
                    reason: "duplicate subscription identity".into(),
                });
            }
        }
        self.active.retain(|id, cancel| {
            let keep = ids.contains(id.as_str());
            if !keep {
                cancel.cancel();
            }
            keep
        });
        for subscription in wanted {
            if self.active.contains_key(&subscription.id) {
                continue;
            }
            let source = format!("subscription {}", subscription.id);
            if self
                .live
                .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count < self.limit).then_some(count + 1)
                })
                .is_err()
            {
                return Err(RuntimeFailure {
                    source,
                    reason: "live worker limit reached, including streams still stopping".into(),
                });
            }
            let slot = Slot(self.live.clone());
            let cancel = Cancellation::default();
            let outgoing = delivery.scoped(cancel.clone());
            let failures = outgoing.clone();
            let worker_cancel = cancel.clone();
            std::thread::Builder::new()
                .name("lucent-subscription".into())
                .spawn(move || {
                    let _slot = slot;
                    let event_cancel = worker_cancel.clone();
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        (subscription.run)(
                            Emitter::new(move |message| {
                                outgoing.send(RuntimeEvent::Subscription(Event {
                                    cancellation: event_cancel.clone(),
                                    result: Ok(message),
                                }));
                            }),
                            worker_cancel.clone(),
                        );
                    }));
                    if result.is_err() && !worker_cancel.cancelled() {
                        failures.send(RuntimeEvent::Subscription(Event {
                            cancellation: worker_cancel,
                            result: Err(RuntimeFailure {
                                source,
                                reason: "subscription panicked".into(),
                            }),
                        }));
                    }
                })
                .map_err(|error| RuntimeFailure {
                    source: "subscriptions".into(),
                    reason: format!("cannot start worker: {error}"),
                })?;
            self.active.insert(subscription.id, cancel);
        }
        Ok(())
    }
}
impl Drop for Workers {
    fn drop(&mut self) {
        for cancel in self.active.values() {
            cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    fn stopped(workers: &Workers) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while workers.live.load(Ordering::Acquire) != 0 {
            assert!(Instant::now() < deadline, "worker did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn event(incoming: &calloop::channel::Channel<RuntimeEvent<u32>>) -> Event<u32> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(RuntimeEvent::Subscription(event)) = incoming.try_recv() {
                return event;
            }
            assert!(Instant::now() < deadline, "subscription did not deliver");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn stable_identity_stays_and_replaced_stream_discards_already_queued_events() {
        let (out, incoming, _life) = Delivery::channel();
        let mut workers = Workers::default();
        workers
            .reconcile(vec![Subscription::stream("a", |out, _| out.send(1))], &out)
            .unwrap();
        let old = event(&incoming);
        workers
            .reconcile(
                vec![Subscription::stream("a", |_, _| {
                    panic!("stable identity restarted")
                })],
                &out,
            )
            .unwrap();
        assert!(!old.cancellation.cancelled());
        workers.reconcile(vec![], &out).unwrap();
        workers
            .reconcile(vec![Subscription::stream("a", |out, _| out.send(2))], &out)
            .unwrap();
        assert!(old.into_live().is_none());
        assert_eq!(event(&incoming).into_live().unwrap().unwrap(), 2);
    }
    #[test]
    fn retiring_workers_count_toward_limit_until_their_body_exits() {
        let (out, _incoming, _life) = Delivery::<RuntimeEvent<u32>>::channel();
        let mut workers = Workers {
            active: BTreeMap::new(),
            live: Arc::new(AtomicUsize::new(0)),
            limit: 1,
        };
        let (release, gate) = mpsc::channel();
        workers
            .reconcile(
                vec![Subscription::stream("a", move |_, _| {
                    gate.recv().unwrap();
                })],
                &out,
            )
            .unwrap();
        workers.reconcile(vec![], &out).unwrap();
        let error = workers
            .reconcile(vec![Subscription::stream("b", |_, _| {})], &out)
            .unwrap_err();
        assert!(error.reason.contains("still stopping"));
        release.send(()).unwrap();
        stopped(&workers);
        workers
            .reconcile(vec![Subscription::stream("b", |_, _| {})], &out)
            .unwrap();
        stopped(&workers);
    }
    #[test]
    fn panic_is_reported_without_payload_and_drop_cancels_cooperative_streams() {
        let (out, incoming, _life) = Delivery::channel();
        let mut workers = Workers::default();
        workers
            .reconcile(
                vec![Subscription::stream("a", |_, _| panic!("private payload"))],
                &out,
            )
            .unwrap();
        let error = event(&incoming).into_live().unwrap().unwrap_err();
        assert_eq!(error.reason, "subscription panicked");
        let (started, ready) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        workers
            .reconcile(
                vec![Subscription::stream("b", move |_, cancel| {
                    started.send(()).unwrap();
                    cancel.sleep(Duration::from_secs(60));
                    finished.send(cancel.cancelled()).unwrap();
                })],
                &out,
            )
            .unwrap();
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(workers);
        assert!(done.recv_timeout(Duration::from_secs(2)).unwrap());
    }
    #[test]
    fn duplicate_identities_are_rejected_before_starting_work() {
        let (out, _incoming, _life) = Delivery::<RuntimeEvent<u32>>::channel();
        let mut workers = Workers::default();
        let never = |_, _| panic!("invalid subscriptions started");
        assert!(
            workers
                .reconcile(
                    vec![
                        Subscription::stream("a", never),
                        Subscription::stream("a", never)
                    ],
                    &out
                )
                .unwrap_err()
                .reason
                .contains("duplicate")
        );
        assert_eq!(workers.live.load(Ordering::Acquire), 0);
        assert!(workers.active.is_empty());
    }
    #[test]
    fn dropping_workers_releases_a_stream_blocked_on_message_delivery() {
        let (out, incoming, _life) = Delivery::channel();
        let mut workers = Workers::default();
        let sent = Arc::new(AtomicUsize::new(0));
        let count = sent.clone();
        let (finished, done) = mpsc::channel();
        workers
            .reconcile(
                vec![Subscription::stream("burst", move |out, cancel| {
                    while !cancel.cancelled() {
                        out.send(1);
                        count.fetch_add(1, Ordering::AcqRel);
                    }
                    finished.send(()).unwrap();
                })],
                &out,
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while sent.load(Ordering::Acquire) < super::super::delivery::CAPACITY {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(workers);
        done.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut queued = 0;
        while let Ok(RuntimeEvent::Subscription(event)) = incoming.try_recv() {
            assert!(event.into_live().is_none());
            queued += 1;
        }
        assert_eq!(queued, super::super::delivery::CAPACITY);
    }
}
