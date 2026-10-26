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
    pub target: WallpaperPixelTarget,
}

/// The target port is the sole source of physical target demand. A read must
/// not block. None means a snapshot is temporarily contended, not cancellation.
/// Initial admission requires Some; later None retains the last admitted demand.
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
}
