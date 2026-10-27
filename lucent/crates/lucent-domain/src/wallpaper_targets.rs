//! Physical playback demand and immutable result identity; no native handles.
use crate::{DomainError, MediaFrame, Result, WallpaperFrameRate, WallpaperPixelTarget};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WallpaperTargetRevision(u64);
impl WallpaperTargetRevision {
    pub const fn initial() -> Self {
        Self(1)
    }
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid(
                "Wallpaper target revision is zero".into(),
            ));
        }
        Ok(Self(value))
    }
    pub const fn get(self) -> u64 {
        self.0
    }
    pub fn checked_next(self) -> Result<Self> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| DomainError::Invalid("Wallpaper target revision is exhausted".into()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallpaperTargetDemand {
    pub revision: WallpaperTargetRevision,
    /// None explicitly requests source/native-size pixels, as PlaybackOptions
    /// does. This is an admitted demand, not an absent port observation.
    pub target: Option<WallpaperPixelTarget>,
}
impl WallpaperTargetDemand {
    /// Equal demand keeps its revision. A changed source-size/cover demand
    /// advances exactly once, or fails before replacing the previous value.
    pub fn retarget(self, target: Option<WallpaperPixelTarget>) -> Result<Self> {
        if self.target == target {
            return Ok(self);
        }
        Ok(Self {
            revision: self.revision.checked_next()?,
            target,
        })
    }
}

/// The target port is the sole source of physical target demand. A read must
/// not block. None means a snapshot is temporarily contended, not cancellation.
/// Initial admission requires Some; later None retains the last admitted demand.
/// Some(WallpaperTargetDemand { target: None, .. }) explicitly requests source
/// pixels and remains distinct from that outer None observation.
pub trait WallpaperTargetPort: Send + Sync {
    fn try_latest(&self) -> Result<Option<WallpaperTargetDemand>>;
}

/// Dynamic playback policy intentionally contains no second target value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetargetablePlaybackOptions {
    pub fps: WallpaperFrameRate,
    pub looping: bool,
}
impl Default for RetargetablePlaybackOptions {
    fn default() -> Self {
        Self {
            fps: WallpaperFrameRate::default(),
            looping: true,
        }
    }
}

/// Captured before conversion. A newer demand can arrive while conversion runs;
/// consumers must reject a result whose revision no longer matches their intent.
#[derive(Debug)]
pub struct TargetedMediaFrame {
    pub revision: WallpaperTargetRevision,
    pub frame: MediaFrame,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_revisions_are_nonzero_ordered_and_never_wrap() {
        assert!(WallpaperTargetRevision::new(0).is_err());
        let first = WallpaperTargetRevision::initial();
        assert_eq!(first.get(), 1);
        assert!(first.checked_next().unwrap() > first);
        let last = WallpaperTargetRevision::new(u64::MAX).unwrap();
        assert!(last.checked_next().is_err());
        assert_eq!(last.get(), u64::MAX);
    }

    #[test]
    fn source_and_cover_transitions_advance_once_while_equal_demands_keep_their_revision() {
        let source = WallpaperTargetDemand {
            revision: WallpaperTargetRevision::initial(),
            target: None,
        };
        assert_eq!(source.retarget(None).unwrap(), source);
        let target = WallpaperPixelTarget::new(1920, 1080).unwrap();
        let cover = source.retarget(Some(target)).unwrap();
        assert_eq!(cover.target, Some(target));
        assert_eq!(cover.revision, source.revision.checked_next().unwrap());
        assert_eq!(cover.retarget(Some(target)).unwrap(), cover);
        let restored = cover.retarget(None).unwrap();
        assert_eq!(restored.target, None);
        assert_eq!(restored.revision, cover.revision.checked_next().unwrap());
        assert_eq!(restored.retarget(None).unwrap(), restored);
    }

    #[test]
    fn source_and_cover_revision_exhaustion_preserves_the_previous_demand() {
        let target = WallpaperPixelTarget::new(1920, 1080).unwrap();
        for demand in [None, Some(target)] {
            let previous = WallpaperTargetDemand {
                revision: WallpaperTargetRevision::new(u64::MAX).unwrap(),
                target: demand,
            };
            let changed = if demand.is_none() { Some(target) } else { None };
            assert!(previous.retarget(changed).is_err());
            assert_eq!(previous.target, demand);
            assert_eq!(previous.revision.get(), u64::MAX);
            assert_eq!(previous.retarget(demand).unwrap(), previous);
        }
    }
}
