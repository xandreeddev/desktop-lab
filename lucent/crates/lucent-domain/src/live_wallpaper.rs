//! Live wallpaper preferences and media data; no codec, GPU or filesystem types.
use crate::{DomainError, MonitorId, Result, StopSignal, Wallpaper};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
/// A saved default or a monitor override, captured when a control is created.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum LiveWallpaperTarget {
    #[default]
    Default,
    Monitor(MonitorId),
}
impl LiveWallpaperTarget {
    pub fn monitor(&self) -> Option<&MonitorId> {
        match self {
            Self::Default => None,
            Self::Monitor(id) => Some(id),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct WallpaperFps(u8);
impl WallpaperFps {
    pub fn new(value: u8) -> Result<Self> {
        if (1..=60).contains(&value) {
            Ok(Self(value))
        } else {
            Err(DomainError::Invalid(
                "Wallpaper FPS must be between 1 and 60".into(),
            ))
        }
    }
    pub fn value(self) -> u8 {
        self.0
    }
}
impl Default for WallpaperFps {
    fn default() -> Self {
        Self(30)
    }
}
impl TryFrom<u8> for WallpaperFps {
    type Error = DomainError;
    fn try_from(value: u8) -> Result<Self> {
        Self::new(value)
    }
}
impl From<WallpaperFps> for u8 {
    fn from(value: WallpaperFps) -> Self {
        value.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWallpaperAsset {
    pub source: Wallpaper,
    pub path: String,
    pub poster: Wallpaper,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWallpaperSettings {
    pub asset: LiveWallpaperAsset,
    #[serde(default)]
    pub fps: WallpaperFps,
    #[serde(default)]
    pub paused: bool,
    #[serde(default = "enabled")]
    pub enabled: bool,
}
fn enabled() -> bool {
    true
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveWallpapers {
    pub default: Option<LiveWallpaperSettings>,
    pub monitors: BTreeMap<MonitorId, LiveWallpaperSettings>,
    pub paused: bool,
    pub policy: WallpaperPlaybackPolicy,
}
/// Saved preferences, separate from transient observations and explicit pause.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WallpaperPlaybackPolicy {
    pub pause_on_battery: bool,
    pub pause_when_fullscreen: bool,
}
impl Default for WallpaperPlaybackPolicy {
    fn default() -> Self {
        Self {
            pause_on_battery: true,
            pause_when_fullscreen: true,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperPauseCondition {
    Battery,
    Fullscreen,
}
/// Current observations only; never persisted with the user's preferences.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WallpaperPlaybackContext {
    pub preparing_for_sleep: bool,
    /// Last known lock observation; an observation failure must not resume it.
    pub session_locked: bool,
    /// None means unavailable, not a claim that AC power is connected.
    pub on_battery: Option<bool>,
    pub fullscreen_monitors: BTreeSet<MonitorId>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WallpaperPauseReason {
    All,
    Profile,
    Sleep,
    SessionLock,
    Battery,
    Fullscreen,
}
impl LiveWallpapers {
    /// A disabled override still wins over the default.
    pub fn selected(&self, target: &LiveWallpaperTarget) -> Option<&LiveWallpaperSettings> {
        target
            .monitor()
            .and_then(|id| self.monitors.get(id))
            .or(self.default.as_ref())
    }
}
#[derive(Debug)]
pub struct MediaFrame {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    pub timestamp: Duration,
}
impl MediaFrame {
    pub fn rgba(width: u32, height: u32, pixels: Vec<u8>, timestamp: Duration) -> Result<Self> {
        let bytes = u64::from(width) * u64::from(height) * 4;
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || bytes > 64 * 1024 * 1024
            || bytes != pixels.len() as u64
        {
            return Err(DomainError::Invalid(
                "Decoded wallpaper frame exceeds the media envelope".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            pixels,
            timestamp,
        })
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn into_rgba(self) -> (u32, u32, Vec<u8>) {
        (self.width, self.height, self.pixels)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackOptions {
    pub fps: WallpaperFps,
    pub looping: bool,
}
impl Default for PlaybackOptions {
    fn default() -> Self {
        Self {
            fps: WallpaperFps::default(),
            looping: true,
        }
    }
}
pub trait WallpaperPlaybackPort: Send + Sync {
    /// Copy a local media file to owned immutable storage, validate real decoding
    /// and create a still poster before admitting a new wallpaper preference.
    fn prepare(&self, source: &Wallpaper) -> Result<LiveWallpaperAsset>;
    /// A producer worker owns decoding and timing. `false` retires delivery;
    /// cancellation and observer lifetime are independent from the renderer.
    fn play(
        &self,
        asset: &LiveWallpaperAsset,
        options: PlaybackOptions,
        emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
        stop: Arc<dyn StopSignal>,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rates_and_frame_storage_enforce_bounds() {
        for fps in [0, 61, 255] {
            assert!(WallpaperFps::new(fps).is_err());
        }
        assert_eq!(WallpaperFps::default().value(), 30);
        assert!(MediaFrame::rgba(0, 1, vec![], Duration::ZERO).is_err());
        assert!(MediaFrame::rgba(4097, 1, vec![], Duration::ZERO).is_err());
        assert!(MediaFrame::rgba(2, 2, vec![0; 15], Duration::ZERO).is_err());
        assert_eq!(
            MediaFrame::rgba(2, 1, vec![0; 8], Duration::ZERO)
                .unwrap()
                .dimensions(),
            (2, 1)
        );
    }
}
