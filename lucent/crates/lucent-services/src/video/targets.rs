//! Latest demand adapter. Decoder reads never wait for a control writer.
use lucent_domain::*;
use std::sync::{Mutex, TryLockError};

pub struct LatestWallpaperTargetAdapter {
    value: Mutex<WallpaperTargetDemand>,
}
impl LatestWallpaperTargetAdapter {
    pub fn new(target: WallpaperPixelTarget) -> Self {
        Self {
            value: Mutex::new(WallpaperTargetDemand {
                revision: WallpaperTargetRevision::initial(),
                target,
            }),
        }
    }
    /// Compare-and-set rejects stale commands. Equal demand is a true no-op;
    /// a new physical target receives exactly one checked revision increment.
    pub fn set(
        &self,
        expected: WallpaperTargetRevision,
        target: WallpaperPixelTarget,
    ) -> Result<WallpaperTargetDemand> {
        let mut value = self.value.lock().map_err(|_| unavailable())?;
        if value.revision != expected {
            return Err(DomainError::Invalid(
                "Wallpaper target command is stale".into(),
            ));
        }
        if value.target != target {
            *value = WallpaperTargetDemand {
                revision: value.revision.checked_next()?,
                target,
            };
        }
        Ok(*value)
    }
}
impl WallpaperTargetPort for LatestWallpaperTargetAdapter {
    fn try_latest(&self) -> Result<Option<WallpaperTargetDemand>> {
        match self.value.try_lock() {
            Ok(value) => Ok(Some(*value)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(_)) => Err(unavailable()),
        }
    }
}
fn unavailable() -> DomainError {
    DomainError::Unavailable("Wallpaper target control is unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compare_and_set_keeps_one_latest_value_and_noop_does_not_consume_revision() {
        let first = WallpaperPixelTarget::new(1920, 1080).unwrap();
        let large = WallpaperPixelTarget::new(3840, 2160).unwrap();
        let adapter = LatestWallpaperTargetAdapter::new(first);
        let initial = adapter.try_latest().unwrap().unwrap();
        assert_eq!(adapter.set(initial.revision, first).unwrap(), initial);
        let next = adapter.set(initial.revision, large).unwrap();
        assert_eq!(next.revision, initial.revision.checked_next().unwrap());
        assert!(adapter.set(initial.revision, first).is_err());
        assert_eq!(adapter.try_latest().unwrap(), Some(next));
        let guard = adapter.value.lock().unwrap();
        assert_eq!(
            adapter.try_latest().unwrap(),
            None,
            "reader never waits on writer"
        );
        drop(guard);
        let restored = adapter.set(next.revision, first).unwrap();
        assert_eq!(adapter.try_latest().unwrap(), Some(restored));
    }
    #[test]
    fn poisoned_control_is_unavailable_instead_of_an_absent_or_stale_snapshot() {
        let target = WallpaperPixelTarget::new(32, 24).unwrap();
        let adapter = LatestWallpaperTargetAdapter::new(target);
        let initial = adapter.try_latest().unwrap().unwrap();
        let result = std::panic::catch_unwind(|| {
            let _guard = adapter.value.lock().unwrap();
            panic!("Control writer failed");
        });
        assert!(result.is_err());
        assert!(matches!(
            adapter.try_latest(),
            Err(DomainError::Unavailable(_))
        ));
        assert!(matches!(
            adapter.set(initial.revision, WallpaperPixelTarget::new(64, 48).unwrap()),
            Err(DomainError::Unavailable(_))
        ));
        assert_eq!(*adapter.value.lock().unwrap_err().into_inner(), initial);
    }

    #[test]
    fn exhausted_revision_does_not_mutate_the_current_target() {
        let first = WallpaperPixelTarget::new(1, 1).unwrap();
        let adapter = LatestWallpaperTargetAdapter::new(first);
        let last = WallpaperTargetDemand {
            revision: WallpaperTargetRevision::new(u64::MAX).unwrap(),
            target: first,
        };
        *adapter.value.lock().unwrap() = last;
        assert!(
            adapter
                .set(last.revision, WallpaperPixelTarget::new(2, 2).unwrap())
                .is_err()
        );
        assert_eq!(adapter.try_latest().unwrap(), Some(last));
        assert_eq!(adapter.set(last.revision, first).unwrap(), last);
    }
}
