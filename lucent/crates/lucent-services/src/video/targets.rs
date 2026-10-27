//! Latest demand adapter. Decoder reads never wait for a control writer.
use lucent_domain::*;
use std::sync::{Mutex, TryLockError};

pub struct LatestWallpaperTargetAdapter {
    value: Mutex<WallpaperTargetDemand>,
}
impl LatestWallpaperTargetAdapter {
    pub fn new(target: Option<WallpaperPixelTarget>) -> Self {
        Self {
            value: Mutex::new(WallpaperTargetDemand {
                revision: WallpaperTargetRevision::initial(),
                target,
            }),
        }
    }
    /// Compare-and-set rejects stale commands. Equal demand is a true no-op;
    /// a changed source-size/physical target receives one checked increment.
    pub fn set(
        &self,
        expected: WallpaperTargetRevision,
        target: Option<WallpaperPixelTarget>,
    ) -> Result<WallpaperTargetDemand> {
        let mut value = self.value.lock().map_err(|_| unavailable())?;
        if value.revision != expected {
            return Err(DomainError::Invalid(
                "Wallpaper target command is stale".into(),
            ));
        }
        *value = value.retarget(target)?;
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
        let adapter = LatestWallpaperTargetAdapter::new(Some(first));
        let initial = adapter.try_latest().unwrap().unwrap();
        assert_eq!(adapter.set(initial.revision, Some(first)).unwrap(), initial);
        let next = adapter.set(initial.revision, Some(large)).unwrap();
        assert_eq!(next.revision, initial.revision.checked_next().unwrap());
        assert!(adapter.set(initial.revision, Some(first)).is_err());
        assert_eq!(adapter.try_latest().unwrap(), Some(next));
        let guard = adapter.value.lock().unwrap();
        assert_eq!(
            adapter.try_latest().unwrap(),
            None,
            "reader never waits on writer"
        );
        drop(guard);
        let restored = adapter.set(next.revision, Some(first)).unwrap();
        assert_eq!(adapter.try_latest().unwrap(), Some(restored));
    }
    #[test]
    fn poisoned_control_is_unavailable_instead_of_an_absent_or_stale_snapshot() {
        let target = WallpaperPixelTarget::new(32, 24).unwrap();
        let adapter = LatestWallpaperTargetAdapter::new(Some(target));
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
            adapter.set(
                initial.revision,
                Some(WallpaperPixelTarget::new(64, 48).unwrap())
            ),
            Err(DomainError::Unavailable(_))
        ));
        assert_eq!(*adapter.value.lock().unwrap_err().into_inner(), initial);
    }

    #[test]
    fn exhausted_revision_does_not_mutate_the_current_target() {
        let first = WallpaperPixelTarget::new(1, 1).unwrap();
        let adapter = LatestWallpaperTargetAdapter::new(Some(first));
        let last = WallpaperTargetDemand {
            revision: WallpaperTargetRevision::new(u64::MAX).unwrap(),
            target: Some(first),
        };
        *adapter.value.lock().unwrap() = last;
        assert!(
            adapter
                .set(
                    last.revision,
                    Some(WallpaperPixelTarget::new(2, 2).unwrap())
                )
                .is_err()
        );
        assert_eq!(adapter.try_latest().unwrap(), Some(last));
        assert_eq!(adapter.set(last.revision, Some(first)).unwrap(), last);
    }

    #[test]
    fn source_size_is_an_admitted_snapshot_and_cas_rejects_stale_return_to_source() {
        let adapter = LatestWallpaperTargetAdapter::new(None);
        let source = adapter.try_latest().unwrap().unwrap();
        assert_eq!(source.target, None);
        assert_eq!(adapter.set(source.revision, None).unwrap(), source);
        let target = WallpaperPixelTarget::new(1920, 1080).unwrap();
        let cover = adapter.set(source.revision, Some(target)).unwrap();
        assert!(adapter.set(source.revision, None).is_err());
        assert_eq!(adapter.try_latest().unwrap(), Some(cover));
        let restored = adapter.set(cover.revision, None).unwrap();
        assert_eq!(restored.target, None);
        assert!(restored.revision > cover.revision);
        assert_eq!(adapter.try_latest().unwrap(), Some(restored));
    }

    #[test]
    fn contended_source_snapshot_is_not_an_admitted_source_size_update() {
        let adapter = LatestWallpaperTargetAdapter::new(None);
        let source = adapter.try_latest().unwrap().unwrap();
        let guard = adapter.value.lock().unwrap();
        assert_eq!(adapter.try_latest().unwrap(), None);
        drop(guard);
        assert_eq!(adapter.try_latest().unwrap(), Some(source));
        let cover = adapter
            .set(
                source.revision,
                Some(WallpaperPixelTarget::new(32, 24).unwrap()),
            )
            .unwrap();
        let guard = adapter.value.lock().unwrap();
        assert_eq!(adapter.try_latest().unwrap(), None);
        drop(guard);
        assert_eq!(adapter.try_latest().unwrap(), Some(cover));
        assert_eq!(adapter.set(cover.revision, None).unwrap().target, None);
    }
}
