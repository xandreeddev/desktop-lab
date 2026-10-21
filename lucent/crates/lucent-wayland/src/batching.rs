//! One-shot admission for work left by post-submission client effects.
//! This does not begin demand, wake callback-blocked surfaces or create timers.
use lucent_api::FrameDemand;

pub(super) fn needs_wake<'a>(surfaces: impl IntoIterator<Item = (bool, &'a FrameDemand)>) -> bool {
    surfaces
        .into_iter()
        .any(|(configured, demand)| configured && demand.can_begin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_updates_submit_latest_state_once_and_wait_for_callback() {
        let mut demand = FrameDemand::default();
        let mut value = 0;
        assert_eq!(value, 0);
        for update in [1, 2, 3] {
            value = update;
            demand.invalidate();
        }
        assert!(needs_wake([(true, &demand)]));
        assert!(demand.begin());
        assert_eq!(value, 3);
        assert!(!demand.begin());
        assert!(!needs_wake([(true, &demand)]));
        demand.invalidate(); // Animation requests a future frame.
        assert!(!needs_wake([(true, &demand)]));
        demand.ready();
        assert!(needs_wake([(true, &demand)]));
        assert!(demand.begin());
        assert!(!needs_wake([(true, &demand)]));
    }

    #[test]
    fn frame_submitted_can_wake_an_earlier_surface_without_reentering_draw() {
        let mut earlier = FrameDemand::default();
        let mut submitted = FrameDemand::default();
        submitted.invalidate();
        assert!(submitted.begin());
        assert!(!needs_wake([(true, &earlier), (true, &submitted)]));
        earlier.invalidate(); // The successful frame's client message.
        assert!(needs_wake([(true, &earlier), (true, &submitted)]));
        assert!(earlier.begin()); // The following dispatch's sole draw pass.
        assert!(!needs_wake([(true, &earlier), (true, &submitted)]));
    }

    #[test]
    fn unconfigured_and_retry_blocked_work_never_busy_wakes() {
        let mut demand = FrameDemand::default();
        demand.invalidate();
        assert!(!needs_wake([(false, &demand)]));
        assert!(demand.begin());
        demand.defer();
        assert!(!needs_wake([(true, &demand)]));
        demand.ready(); // A late callback does not cancel a retry deadline.
        assert!(!needs_wake([(true, &demand)]));
        demand.retry_ready();
        assert!(needs_wake([(true, &demand)]));
        assert!(demand.begin());
        demand.ready();
        assert!(!needs_wake([(true, &demand)]));
    }

    #[test]
    fn retired_demand_does_not_grant_a_replacement_first_frame() {
        let mut old = FrameDemand::default();
        old.invalidate();
        assert!(old.begin());
        old.invalidate();
        old.ready();
        let mut replacement = FrameDemand::default();
        assert!(!needs_wake([(false, &replacement)]));
        replacement.invalidate();
        assert!(!needs_wake([(false, &replacement)]));
        assert!(needs_wake([(true, &replacement)]));
        assert!(replacement.begin());
        assert!(!needs_wake([(true, &replacement)]));
    }

    #[test]
    fn one_shot_ping_coalesces_without_periodic_or_idle_work() {
        let mut events = calloop::EventLoop::<usize>::try_new().unwrap();
        let (wake, source) = calloop::ping::make_ping().unwrap();
        events
            .handle()
            .insert_source(source, |(), _, wakes| *wakes += 1)
            .unwrap();
        let mut count = 0;
        events
            .dispatch(std::time::Duration::ZERO, &mut count)
            .unwrap();
        assert_eq!(count, 0);
        for _ in 0..4 {
            wake.ping();
        }
        events
            .dispatch(std::time::Duration::ZERO, &mut count)
            .unwrap();
        assert_eq!(count, 1);
        events
            .dispatch(std::time::Duration::ZERO, &mut count)
            .unwrap();
        assert_eq!(count, 1);
        wake.ping();
        events
            .dispatch(std::time::Duration::ZERO, &mut count)
            .unwrap();
        assert_eq!(count, 2);
    }
}
