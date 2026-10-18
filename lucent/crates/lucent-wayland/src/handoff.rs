//! Bounded last-good presentation retention during an explicit native renewal.
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct SubmissionHistory {
    pub frames: u64,
    pub frame_times: std::collections::VecDeque<f64>,
    pub profiles: crate::profiling::History,
}
impl SubmissionHistory {
    pub fn continuation(&mut self) -> Self {
        std::mem::take(self)
    }
    pub fn record(&mut self, submitted_ms: f64) {
        self.frames = self.frames.saturating_add(1);
        self.frame_times.push_back(submitted_ms);
        if self.frame_times.len() > crate::profiling::MAX_FRAMES {
            self.frame_times.pop_front();
        }
    }
}

pub(super) fn validate_recreation(secure: bool, requested: bool) -> Result<(), &'static str> {
    if secure && requested {
        Err("Native surface recreation is unavailable for session-lock clients")
    } else {
        Ok(())
    }
}

pub(super) fn validate_layer_recreation(
    committed_zone: i32,
    pending_zone: i32,
    declared_zone: i32,
) -> Result<(), &'static str> {
    if [committed_zone, pending_zone, declared_zone]
        .into_iter()
        .any(|zone| zone > 0)
    {
        Err("Native surface recreation requires a non-reserving layer")
    } else {
        Ok(())
    }
}

pub(super) struct Presentations<K, V>(BTreeMap<K, V>);
impl<K, V> Default for Presentations<K, V> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}
impl<K: Ord, V> Presentations<K, V> {
    /// A second request can discard an unpainted replacement, but cannot
    /// replace the retained successful presentation with that pending surface.
    pub fn retain_last_good(&mut self, key: K, previous: V, presented: bool) {
        if presented {
            self.0.entry(key).or_insert(previous);
        }
    }
    pub fn finish_submission<E>(
        &mut self,
        key: &K,
        outcome: &Result<lucent_render::RenderOutcome, E>,
    ) {
        if matches!(outcome, Ok(lucent_render::RenderOutcome::Presented)) {
            self.0.remove(key);
        }
    }
    pub fn retire(&mut self, key: &K) {
        self.0.remove(key);
    }
    pub fn retain(&mut self, keep: impl FnMut(&K, &mut V) -> bool) {
        self.0.retain(keep);
    }
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.0.iter()
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    struct Native(&'static str, Rc<RefCell<Vec<&'static str>>>);
    impl Drop for Native {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }

    #[test]
    fn repeated_pending_replacement_keeps_one_last_good_until_actual_submission() {
        let drops = Rc::new(RefCell::new(vec![]));
        let mut handoff = Presentations::default();
        handoff.retain_last_good("panel", Native("last-good", drops.clone()), true);
        for label in ["pending-1", "pending-2"] {
            handoff.retain_last_good("panel", Native(label, drops.clone()), false);
            handoff.finish_submission(&"panel", &Err(()));
            handoff.finish_submission::<()>(
                &"panel",
                &Ok(lucent_render::RenderOutcome::RetryAfter(
                    std::time::Duration::from_millis(1),
                )),
            );
            assert_eq!(handoff.len(), 1);
            assert_eq!(handoff.iter().next().unwrap().1.0, "last-good");
        }
        assert_eq!(*drops.borrow(), ["pending-1", "pending-2"]);
        handoff.finish_submission::<()>(&"panel", &Ok(lucent_render::RenderOutcome::Presented));
        assert_eq!(handoff.len(), 0);
        assert_eq!(*drops.borrow(), ["pending-1", "pending-2", "last-good"]);
    }
    #[test]
    fn removal_failure_and_shutdown_release_retained_presentations() {
        let drops = Rc::new(RefCell::new(vec![]));
        let mut handoff = Presentations::default();
        handoff.retain_last_good("unpainted", Native("unpainted", drops.clone()), false);
        assert_eq!(handoff.len(), 0);
        handoff.retain_last_good("hidden", Native("hidden", drops.clone()), true);
        handoff.retain_last_good("failed", Native("failed", drops.clone()), true);
        handoff.retain_last_good("shutdown", Native("shutdown", drops.clone()), true);
        handoff.retire(&"hidden");
        handoff.retain(|key, _| *key != "failed");
        assert_eq!(handoff.len(), 1);
        handoff.clear();
        assert_eq!(
            *drops.borrow(),
            ["unpainted", "hidden", "failed", "shutdown"]
        );
    }
    #[test]
    fn session_lock_recreation_is_rejected_before_the_caller_can_mutate_native_state() {
        assert!(validate_recreation(true, true).is_err());
        assert!(validate_recreation(true, false).is_ok());
        assert!(validate_recreation(false, true).is_ok());
    }
    #[test]
    fn reserved_committed_pending_or_replacement_layers_cannot_overlap_reservations() {
        for zones in [(32, 0, 0), (0, 32, 0), (0, 0, 32), (32, 32, 32)] {
            assert!(validate_layer_recreation(zones.0, zones.1, zones.2).is_err());
        }
        for zones in [(0, 0, 0), (-1, -1, -1), (-1, 0, -1), (0, -1, 0)] {
            assert!(validate_layer_recreation(zones.0, zones.1, zones.2).is_ok());
        }
    }
    #[test]
    fn renewal_carries_logical_submission_history_without_fabricating_a_new_frame() {
        let mut old = SubmissionHistory::default();
        old.record(10.);
        old.record(20.);
        old.profiles.push(crate::profiling::FrameProfile {
            frame: 2,
            ..Default::default()
        });
        let mut replacement = old.continuation();
        let mut repeated_pending = replacement.continuation();
        assert_eq!(old.frames, 0);
        assert_eq!(replacement.frames, 0);
        assert_eq!(repeated_pending.frames, 2);
        assert_eq!(
            repeated_pending
                .frame_times
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [10., 20.]
        );
        assert_eq!(repeated_pending.profiles.json()[0]["frame"], 2);
        for index in 0..crate::profiling::MAX_FRAMES + 2 {
            repeated_pending.record(30. + index as f64);
        }
        assert_eq!(
            repeated_pending.frames,
            crate::profiling::MAX_FRAMES as u64 + 4
        );
        assert_eq!(
            repeated_pending.frame_times.len(),
            crate::profiling::MAX_FRAMES
        );
        assert_eq!(repeated_pending.frame_times.front(), Some(&32.));
    }
}
