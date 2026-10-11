//! Optional CPU wall-clock measurements. Driver calls may include waits; these
//! durations are neither GPU execution time nor compositor presentation time.
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default)]
pub struct ResourceProfile {
    pub text_raster: Duration,
    pub texture_upload: Duration,
    pub text_misses: u64,
    pub image_misses: u64,
    pub texture_upload_bytes: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct PaintProfile {
    /// Includes resource rasterization/uploads; those fields are nested breakdowns.
    pub prepare: Duration,
    pub encode: Duration,
    pub submit: Duration,
    pub retire: Duration,
    pub resources: ResourceProfile,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderProfile {
    pub configure: Duration,
    pub acquire: Duration,
    pub paint: PaintProfile,
    pub present: Duration,
    pub total: Duration,
}
pub(crate) fn start(enabled: bool) -> Option<Instant> {
    enabled.then(Instant::now)
}
pub(crate) fn elapsed(start: Option<Instant>) -> Duration {
    start.map_or(Duration::ZERO, |time| time.elapsed())
}
