//! Pure state and scheduling primitives. No operating-system or rendering dependencies.

mod widget;
pub use widget::*;

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
}
