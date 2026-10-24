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
/// Playback follows media timestamps unless the user chooses a delivery cap.
/// A cap never invents frames for media whose own cadence is slower.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "FrameRateValue", into = "FrameRateValue")]
pub enum WallpaperFrameRate {
    #[default]
    Source,
    Capped(WallpaperFps),
}
impl WallpaperFrameRate {
    pub fn cap(self) -> Option<WallpaperFps> {
        match self {
            Self::Source => None,
            Self::Capped(fps) => Some(fps),
        }
    }
}
impl From<WallpaperFps> for WallpaperFrameRate {
    fn from(fps: WallpaperFps) -> Self {
        Self::Capped(fps)
    }
}
impl std::fmt::Display for WallpaperFrameRate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source => formatter.write_str("source"),
            Self::Capped(fps) => write!(formatter, "{}", fps.value()),
        }
    }
}
// Keep existing numeric `fps` preferences intact while adding an explicit mode.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
enum FrameRateValue {
    Capped(WallpaperFps),
    Mode(FrameRateMode),
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FrameRateMode {
    Source,
}
impl From<FrameRateValue> for WallpaperFrameRate {
    fn from(value: FrameRateValue) -> Self {
        match value {
            FrameRateValue::Capped(fps) => Self::Capped(fps),
            FrameRateValue::Mode(FrameRateMode::Source) => Self::Source,
        }
    }
}
impl From<WallpaperFrameRate> for FrameRateValue {
    fn from(value: WallpaperFrameRate) -> Self {
        match value {
            WallpaperFrameRate::Capped(fps) => Self::Capped(fps),
            WallpaperFrameRate::Source => Self::Mode(FrameRateMode::Source),
        }
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
    pub fps: WallpaperFrameRate,
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
/// Transient physical pixel demand, independent of monitor and renderer types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallpaperPixelTarget {
    width: u32,
    height: u32,
}
impl WallpaperPixelTarget {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(DomainError::Invalid(
                "Wallpaper pixel target must have positive dimensions".into(),
            ));
        }
        Ok(Self { width, height })
    }
    pub fn size(self) -> (u32, u32) {
        (self.width, self.height)
    }
    /// Retain sufficient pixels on both edges for cover presentation. Scale
    /// uniformly, round outward and never enlarge or admit an oversized source.
    pub fn cover_size(self, source_width: u32, source_height: u32) -> Result<(u32, u32)> {
        media_frame_byte_size(source_width, source_height)?;
        if self.width >= source_width || self.height >= source_height {
            return Ok((source_width, source_height));
        }
        let invalid_size = || DomainError::Invalid("Wallpaper cover size is invalid".into());
        let width_demand = u64::from(self.width)
            .checked_mul(u64::from(source_height))
            .ok_or_else(invalid_size)?;
        let height_demand = u64::from(self.height)
            .checked_mul(u64::from(source_width))
            .ok_or_else(invalid_size)?;
        let (numerator, denominator) = if width_demand >= height_demand {
            (u64::from(self.width), u64::from(source_width))
        } else {
            (u64::from(self.height), u64::from(source_height))
        };
        let scaled_edge = |edge| {
            u64::from(edge)
                .checked_mul(numerator)
                .and_then(|value| value.checked_add(denominator - 1))
                .map(|value| value / denominator)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(invalid_size)
        };
        Ok((scaled_edge(source_width)?, scaled_edge(source_height)?))
    }
}
#[derive(Debug)]
pub struct MediaFrame {
    width: u32,
    height: u32,
    aspect_size: (u32, u32),
    pixels: Arc<Vec<u8>>,
    pub timestamp: Duration,
}
impl MediaFrame {
    pub fn rgba(width: u32, height: u32, pixels: Vec<u8>, timestamp: Duration) -> Result<Self> {
        Self::rgba_with_aspect(width, height, pixels, (width, height), timestamp)
    }
    /// Keep the original source aspect when outward rounding changes the
    /// converted pixel dimensions. Both remain within the same media envelope.
    pub fn rgba_with_aspect(
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        aspect_size: (u32, u32),
        timestamp: Duration,
    ) -> Result<Self> {
        Self::rgba_shared_with_aspect(width, height, Arc::new(pixels), aspect_size, timestamp)
    }
    /// Admit shared, tightly packed straight-alpha RGBA8 pixels. Shared owners
    /// cannot mutate the published bytes; no codec or presentation state is held.
    pub fn rgba_shared_with_aspect(
        width: u32,
        height: u32,
        pixels: Arc<Vec<u8>>,
        aspect_size: (u32, u32),
        timestamp: Duration,
    ) -> Result<Self> {
        let bytes = media_frame_byte_size(width, height)?;
        media_frame_byte_size(aspect_size.0, aspect_size.1)?;
        if bytes != pixels.len() as u64 {
            return Err(DomainError::Invalid(
                "Decoded wallpaper frame exceeds the media envelope".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            aspect_size,
            pixels,
            timestamp,
        })
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn aspect_size(&self) -> (u32, u32) {
        self.aspect_size
    }
    /// Recover unique vector storage, or copy when another owner retains it.
    /// Shared consumers should use `into_shared_rgba_with_aspect` instead.
    pub fn into_rgba(self) -> (u32, u32, Vec<u8>) {
        (self.width, self.height, owned_pixels(self.pixels))
    }
    /// Recover unique vector storage, or copy when another owner retains it.
    pub fn into_rgba_with_aspect(self) -> (u32, u32, Vec<u8>, (u32, u32)) {
        (
            self.width,
            self.height,
            owned_pixels(self.pixels),
            self.aspect_size,
        )
    }
    /// Transfer immutable pixel ownership without copying the allocation.
    pub fn into_shared_rgba_with_aspect(self) -> (u32, u32, Arc<Vec<u8>>, (u32, u32)) {
        (self.width, self.height, self.pixels, self.aspect_size)
    }
}
fn owned_pixels(pixels: Arc<Vec<u8>>) -> Vec<u8> {
    Arc::try_unwrap(pixels).unwrap_or_else(|shared| (*shared).clone())
}

#[cfg(test)]
mod shared_frame_tests {
    use super::*;

    #[test]
    fn shared_frames_transfer_the_exact_immutable_allocation() {
        let pixels = Arc::new(vec![40, 80, 120, 128]);
        let frame = MediaFrame::rgba_shared_with_aspect(
            1,
            1,
            pixels.clone(),
            (65, 37),
            Duration::from_millis(17),
        )
        .unwrap();
        assert_eq!(frame.dimensions(), (1, 1));
        assert_eq!(frame.aspect_size(), (65, 37));
        assert_eq!(frame.timestamp, Duration::from_millis(17));
        let (_, _, shared, _) = frame.into_shared_rgba_with_aspect();
        assert!(Arc::ptr_eq(&pixels, &shared));
        assert_eq!(shared.as_slice(), [40, 80, 120, 128]);
    }

    #[test]
    fn legacy_extraction_unwraps_unique_pixels_or_copies_retained_pixels() {
        let pixels = vec![20; 4];
        let pointer = pixels.as_ptr();
        let frame = MediaFrame::rgba(1, 1, pixels, Duration::ZERO).unwrap();
        assert_eq!(frame.into_rgba().2.as_ptr(), pointer);
        let shared = Arc::new(vec![60; 4]);
        let frame =
            MediaFrame::rgba_shared_with_aspect(1, 1, shared.clone(), (1, 1), Duration::ZERO)
                .unwrap();
        let mut owned = frame.into_rgba_with_aspect().2;
        assert_ne!(owned.as_ptr(), shared.as_ptr());
        owned[0] = 90;
        assert_eq!(shared.as_slice(), [60; 4]);
    }

    #[test]
    fn shared_storage_does_not_bypass_media_validation() {
        for (width, height, length, aspect) in [
            (0, 1, 4, (1, 1)),
            (1, 1, 3, (1, 1)),
            (1, 1, 4, (0, 1)),
            (4097, 1, 4, (1, 1)),
        ] {
            assert!(
                MediaFrame::rgba_shared_with_aspect(
                    width,
                    height,
                    Arc::new(vec![0; length]),
                    aspect,
                    Duration::ZERO,
                )
                .is_err()
            );
        }
    }
}
fn media_frame_byte_size(width: u32, height: u32) -> Result<u64> {
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|value| value.checked_mul(4));
    match bytes {
        Some(bytes)
            if width > 0
                && height > 0
                && width <= 4096
                && height <= 4096
                && bytes <= 64 * 1024 * 1024 =>
        {
            Ok(bytes)
        }
        _ => Err(DomainError::Invalid(
            "Decoded wallpaper frame exceeds the media envelope".into(),
        )),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackOptions {
    pub fps: WallpaperFrameRate,
    pub looping: bool,
    pub target: Option<WallpaperPixelTarget>,
}
impl Default for PlaybackOptions {
    fn default() -> Self {
        Self {
            fps: WallpaperFrameRate::default(),
            looping: true,
            target: None,
        }
    }
}
/// Physical preview bounds, separate from saved playback and cover demand.
/// Previewing never enlarges a source or admits media outside its envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallpaperPreviewOptions {
    width: u32,
    height: u32,
}
impl WallpaperPreviewOptions {
    pub const MAX_EDGE: u32 = 1024;

    pub fn new(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 || width > Self::MAX_EDGE || height > Self::MAX_EDGE {
            return Err(DomainError::Invalid(
                "Wallpaper preview bounds must be between 1 and 1024 physical pixels".into(),
            ));
        }
        Ok(Self { width, height })
    }
    pub fn size(self) -> (u32, u32) {
        (self.width, self.height)
    }
    /// Fit both physical edges without upscaling. Integer rounding can alter
    /// pixel aspect slightly; MediaFrame retains the natural source aspect.
    pub fn fit_size(self, source_width: u32, source_height: u32) -> Result<(u32, u32)> {
        media_frame_byte_size(source_width, source_height)?;
        if source_width <= self.width && source_height <= self.height {
            return Ok((source_width, source_height));
        }
        if u64::from(source_width) * u64::from(self.height)
            >= u64::from(source_height) * u64::from(self.width)
        {
            Ok((
                self.width,
                (u64::from(source_height) * u64::from(self.width) / u64::from(source_width)).max(1)
                    as u32,
            ))
        } else {
            Ok((
                (u64::from(source_width) * u64::from(self.height) / u64::from(source_height)).max(1)
                    as u32,
                self.height,
            ))
        }
    }
}
pub trait WallpaperPlaybackPort: Send + Sync {
    /// Discover up to 32 local media sources without decoding or selecting them.
    /// A source still needs `prepare` before it can become a saved preference.
    fn library(&self) -> Result<Vec<Wallpaper>> {
        Ok(vec![])
    }
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
    /// Read a local source without importing it or changing saved preferences.
    /// One caller-owned worker receives owned, straight-alpha RGBA frames at
    /// source cadence, looping until cancellation or a false callback result.
    /// Output fits the checked preview bounds and preserves aspect/timestamps.
    /// Cancellation is cooperative; return means the decoder body has ended.
    fn preview(
        &self,
        _source: &Wallpaper,
        _options: WallpaperPreviewOptions,
        emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
        stop: Arc<dyn StopSignal>,
    ) {
        if !stop.cancelled() {
            emit(Err(DomainError::Unavailable(
                "Live wallpaper previews are unavailable".into(),
            )));
        }
    }
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
    #[test]
    fn pixel_targets_retain_cover_density_without_enlarging_sources() {
        for (target, source, expected) in [
            ((1920, 1080), (3840, 2160), (1920, 1080)),
            ((1080, 1920), (2160, 3840), (1080, 1920)),
            ((1080, 1920), (3840, 2160), (3414, 1920)),
            ((1920, 1080), (2160, 3840), (1920, 3414)),
            ((16, 9), (65, 37), (16, 10)),
            ((16, 10), (65, 37), (18, 10)),
            ((1920, 1080), (16, 9), (16, 9)),
            ((1, 1), (4096, 4095), (2, 1)),
            ((1, 1), (4096, 1), (4096, 1)),
            ((u32::MAX, u32::MAX), (4096, 4096), (4096, 4096)),
        ] {
            let target = WallpaperPixelTarget::new(target.0, target.1).unwrap();
            assert_eq!(target.cover_size(source.0, source.1).unwrap(), expected);
        }
        for target in [(1, 1), (u32::MAX, 1), (1, u32::MAX)] {
            let value = WallpaperPixelTarget::new(target.0, target.1).unwrap();
            assert_eq!(value.size(), target);
        }
    }
    #[test]
    fn preview_bounds_contain_both_edges_without_enlarging_or_relaxing_admission() {
        for (target, source, expected) in [
            ((1024, 1024), (3840, 2160), (1024, 576)),
            ((1024, 1024), (2160, 3840), (576, 1024)),
            ((320, 180), (4096, 4096), (180, 180)),
            ((180, 320), (4096, 1), (180, 1)),
            ((320, 180), (1, 4096), (1, 180)),
            ((1, 1), (4096, 4095), (1, 1)),
            ((640, 480), (17, 9), (17, 9)),
            ((17, 9), (65, 37), (15, 9)),
        ] {
            let options = WallpaperPreviewOptions::new(target.0, target.1).unwrap();
            assert_eq!(options.size(), target);
            assert_eq!(options.fit_size(source.0, source.1).unwrap(), expected);
        }
        for (width, height) in [(0, 1), (1, 0), (1025, 1), (1, 1025), (u32::MAX, 1)] {
            assert!(WallpaperPreviewOptions::new(width, height).is_err());
        }
        let options = WallpaperPreviewOptions::new(1, 1).unwrap();
        for (width, height) in [(0, 1), (1, 0), (4097, 1), (1, 4097), (u32::MAX, u32::MAX)] {
            assert!(options.fit_size(width, height).is_err());
        }
    }
    #[test]
    fn display_demand_cannot_relax_source_admission() {
        assert!(WallpaperPixelTarget::new(0, 1).is_err());
        assert!(WallpaperPixelTarget::new(1, 0).is_err());
        let target = WallpaperPixelTarget::new(1, 1).unwrap();
        for (width, height) in [(0, 1), (1, 0), (4097, 1), (1, 4097), (u32::MAX, u32::MAX)] {
            assert!(target.cover_size(width, height).is_err());
            assert!(
                MediaFrame::rgba_with_aspect(1, 1, vec![0; 4], (width, height), Duration::ZERO)
                    .is_err()
            );
            assert!(
                MediaFrame::rgba_with_aspect(width, height, vec![0; 4], (1, 1), Duration::ZERO)
                    .is_err()
            );
        }
        assert!(
            MediaFrame::rgba_with_aspect(1, 1, vec![0; 3], (4096, 4096), Duration::ZERO).is_err()
        );
        assert_eq!(
            MediaFrame::rgba_with_aspect(1, 1, vec![0; 4], (4096, 4096), Duration::ZERO)
                .unwrap()
                .aspect_size(),
            (4096, 4096)
        );
    }
    #[test]
    fn media_frames_preserve_original_aspect_and_owned_pixels() {
        let pixels = [200, 100, 50, 128].repeat(16 * 10);
        let allocation = pixels.as_ptr();
        let timestamp = Duration::from_millis(125);
        let frame = MediaFrame::rgba_with_aspect(16, 10, pixels, (65, 37), timestamp).unwrap();
        assert_eq!(frame.dimensions(), (16, 10));
        assert_eq!(frame.aspect_size(), (65, 37));
        assert_eq!(frame.timestamp, timestamp);
        let (width, height, pixels, aspect) = frame.into_rgba_with_aspect();
        assert_eq!((width, height), (16, 10));
        assert_eq!(aspect, (65, 37));
        assert_eq!(pixels.as_ptr(), allocation);
        assert_eq!(pixels, [200, 100, 50, 128].repeat(16 * 10));

        let legacy = MediaFrame::rgba(2, 1, vec![80; 8], Duration::ZERO).unwrap();
        assert_eq!(legacy.aspect_size(), (2, 1));
        assert_eq!(legacy.into_rgba(), (2, 1, vec![80; 8]));
    }
    #[test]
    fn frame_rate_defaults_to_source_and_retains_validated_caps_and_labels() {
        for fps in [1, 15, 30, 60] {
            let rate = WallpaperFrameRate::Capped(WallpaperFps::new(fps).unwrap());
            assert_eq!(
                WallpaperFrameRate::from(WallpaperFps::new(fps).unwrap()),
                rate
            );
            assert_eq!(rate.cap().unwrap().value(), fps);
            assert_eq!(rate.to_string(), fps.to_string());
        }
        assert_eq!(WallpaperFrameRate::default(), WallpaperFrameRate::Source);
        assert_eq!(PlaybackOptions::default().fps, WallpaperFrameRate::Source);
        assert_eq!(PlaybackOptions::default().target, None);
        assert_eq!(WallpaperFrameRate::Source.cap(), None);
        assert_eq!(WallpaperFrameRate::Source.to_string(), "source");
    }
}
