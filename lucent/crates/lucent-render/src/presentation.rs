//! Presentation recovery policy, independent of Wayland and GPU handles.
use std::time::Duration;

pub(super) enum SurfaceFailure {
    Timeout,
    Occluded,
    Outdated,
    Lost,
}
impl SurfaceFailure {
    pub fn reconfigure(&self) -> bool {
        matches!(self, Self::Outdated | Self::Lost)
    }
    pub fn recreate(&self) -> bool {
        matches!(self, Self::Lost)
    }
}

#[derive(Default)]
pub(super) struct Recovery {
    consecutive_failures: u32,
}
impl Recovery {
    pub fn retry(&mut self) -> Duration {
        let delay = Duration::from_millis((16_u64 << self.consecutive_failures.min(6)).min(1000));
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        delay
    }
    pub fn presented(&mut self) {
        self.consecutive_failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temporary_failures_back_off_without_disabling_future_presentation() {
        let mut recovery = Recovery::default();
        assert_eq!(recovery.retry(), Duration::from_millis(16));
        assert_eq!(recovery.retry(), Duration::from_millis(32));
        for _ in 0..100 {
            assert!(recovery.retry() <= Duration::from_secs(1));
        }
        assert_eq!(recovery.retry(), Duration::from_secs(1));
        recovery.presented();
        assert_eq!(recovery.retry(), Duration::from_millis(16));
    }
    #[test]
    fn only_lost_surface_requires_recreation() {
        for failure in [SurfaceFailure::Timeout, SurfaceFailure::Occluded] {
            assert!(!failure.reconfigure());
            assert!(!failure.recreate());
        }
        assert!(SurfaceFailure::Outdated.reconfigure());
        assert!(!SurfaceFailure::Outdated.recreate());
        assert!(SurfaceFailure::Lost.reconfigure());
        assert!(SurfaceFailure::Lost.recreate());
    }
}
