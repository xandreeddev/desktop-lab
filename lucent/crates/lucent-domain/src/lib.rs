//! Pure state and scheduling primitives. No operating-system or rendering dependencies.

/// Tracks demand for frames, coalescing input while a compositor callback is pending.
#[derive(Debug, Default)]
pub struct FrameDemand {
    dirty: bool,
    pending: bool,
}
impl FrameDemand {
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    pub fn ready(&mut self) {
        self.pending = false;
    }
    pub fn begin(&mut self) -> bool {
        if !self.dirty || self.pending {
            return false;
        }
        self.dirty = false;
        self.pending = true;
        true
    }
}

/// Small domain example that makes pointer input observable.
#[derive(Debug, Default)]
pub struct Counter {
    pub clicks: u64,
}
impl Counter {
    pub fn click(&mut self) {
        self.clicks = self.clicks.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_never_requests_frames() {
        let mut frames = FrameDemand::default();
        assert!(!frames.begin());
        frames.invalidate();
        assert!(frames.begin());
        frames.ready();
        assert!(!frames.begin());
    }
    #[test]
    fn input_during_pending_frame_is_not_lost() {
        let mut frames = FrameDemand::default();
        frames.invalidate();
        assert!(frames.begin());
        frames.invalidate();
        frames.invalidate();
        assert!(!frames.begin());
        frames.ready();
        assert!(frames.begin());
        frames.ready();
        assert!(!frames.begin());
    }
    #[test]
    fn counter_saturates() {
        let mut counter = Counter { clicks: u64::MAX };
        counter.click();
        assert_eq!(counter.clicks, u64::MAX);
    }
}
