//! Bounded delivery from background producers. Never call send on the UI thread.
use calloop::channel::{self, Channel, SyncSender};
use lucent_api::Cancellation;
use std::{sync::mpsc::TrySendError, time::Duration};

pub(super) const CAPACITY: usize = 64;
/// Holds at most CAPACITY waiting values. Payload sizes are application-owned.
/// Backpressure waits only on producing workers; cancellation abandons pending
/// values. A generation token on subscription events handles cancellation after
/// acceptance. Effect completions must retain their accepted side effects.
pub(super) struct Delivery<T> {
    sender: SyncSender<T>,
    runtime: Cancellation,
    producer: Cancellation,
}
impl<T> Clone for Delivery<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            runtime: self.runtime.clone(),
            producer: self.producer.clone(),
        }
    }
}
pub(super) struct Lifetime(Cancellation);
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
impl<T> Delivery<T> {
    pub fn channel() -> (Self, Channel<T>, Lifetime) {
        Self::bounded(CAPACITY)
    }
    fn bounded(capacity: usize) -> (Self, Channel<T>, Lifetime) {
        let (sender, receiver) = channel::sync_channel(capacity);
        let runtime = Cancellation::default();
        (
            Self {
                sender,
                runtime: runtime.clone(),
                producer: runtime.clone(),
            },
            receiver,
            Lifetime(runtime),
        )
    }
    pub fn scoped(&self, producer: Cancellation) -> Self {
        Self {
            sender: self.sender.clone(),
            runtime: self.runtime.clone(),
            producer,
        }
    }
    /// Returns false when shutdown, cancellation or disconnection prevents delivery.
    pub fn send(&self, mut value: T) -> bool {
        loop {
            if self.producer.cancelled() || self.runtime.cancelled() {
                return false;
            }
            match self.sender.try_send(value) {
                Ok(()) => return true,
                Err(TrySendError::Disconnected(_)) => return false,
                Err(TrySendError::Full(pending)) => {
                    value = pending;
                    // Only overloaded producers wait. Ordinary idle operation has
                    // no polling timer. This also bounds shutdown latency when a
                    // scoped producer has a different cancellation token.
                    self.producer.sleep(Duration::from_millis(5));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    #[test]
    fn capacity_applies_backpressure_and_preserves_fifo() {
        let (out, incoming, _lifetime) = Delivery::bounded(1);
        assert!(out.send(1));
        let (entered, ready) = mpsc::channel();
        let (finished, result) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            entered.send(()).unwrap();
            finished.send(out.send(2)).unwrap();
        });
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(result.recv_timeout(Duration::from_millis(30)).is_err());
        assert_eq!(incoming.recv().unwrap(), 1);
        assert!(result.recv_timeout(Duration::from_secs(2)).unwrap());
        assert_eq!(incoming.recv().unwrap(), 2);
        worker.join().unwrap();
    }
    #[test]
    fn cancelling_an_overloaded_producer_needs_no_receiver_progress() {
        for stop_runtime in [false, true] {
            let (out, incoming, lifetime) = Delivery::bounded(1);
            assert!(out.send(1));
            let cancel = Cancellation::default();
            let scoped = out.scoped(cancel.clone());
            let (done, result) = mpsc::channel();
            let worker = std::thread::spawn(move || done.send(scoped.send(2)).unwrap());
            assert!(result.recv_timeout(Duration::from_millis(30)).is_err());
            if stop_runtime {
                drop(lifetime);
            } else {
                cancel.cancel();
            }
            assert!(!result.recv_timeout(Duration::from_secs(2)).unwrap());
            assert_eq!(incoming.recv().unwrap(), 1);
            assert!(incoming.try_recv().is_err());
            worker.join().unwrap();
        }
    }
    #[test]
    fn disconnected_receiver_and_expired_lifetime_reject_delivery() {
        let (out, incoming, lifetime) = Delivery::<u32>::bounded(1);
        drop(incoming);
        assert!(!out.send(1));
        let (out, _incoming, lifetime2) = Delivery::<u32>::bounded(1);
        drop(lifetime2);
        assert!(!out.send(2));
        drop(lifetime);
    }
    #[test]
    fn event_loop_dispatch_is_bounded_and_remaining_work_is_woken() {
        let (out, incoming, _lifetime) = Delivery::channel();
        let mut event_loop = calloop::EventLoop::<Vec<usize>>::try_new().unwrap();
        event_loop
            .handle()
            .insert_source(incoming, |event, _, received| {
                if let channel::Event::Msg(value) = event {
                    received.push(value);
                }
            })
            .unwrap();
        for value in 0..CAPACITY {
            assert!(out.send(value));
        }
        let worker = std::thread::spawn(move || {
            for value in CAPACITY..CAPACITY * 3 {
                assert!(out.send(value));
            }
        });
        let mut received = vec![];
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while received.len() < CAPACITY * 3 {
            assert!(std::time::Instant::now() < deadline, "lost a queue wakeup");
            let before = received.len();
            event_loop
                .dispatch(Duration::from_millis(50), &mut received)
                .unwrap();
            assert!(received.len() - before <= CAPACITY + 1);
        }
        assert_eq!(received, (0..CAPACITY * 3).collect::<Vec<_>>());
        worker.join().unwrap();
    }
}
