//! Client presentation contracts. OS capability contracts live in lucent-domain.
use lucent_api::ImageData;
use lucent_domain::*;
use std::{collections::BTreeMap, sync::Arc};
pub type Images = Vec<(String, Arc<ImageData>)>;
/// Prepared previews retain density for the current cards through 2× scale.
pub const PREVIEW_DIMENSION: u32 = 1024;
/// Physical display size for background preparation; never a persisted preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackgroundTarget {
    width: u32,
    height: u32,
}
impl BackgroundTarget {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(DomainError::Invalid(
                "Background target must be positive".into(),
            ));
        }
        Ok(Self { width, height })
    }
    pub fn from_surface(width: f32, height: f32, scale: u32) -> Result<Self> {
        let edge = |value: f32| {
            if !value.is_finite() || value <= 0. || f64::from(value.ceil()) > f64::from(u32::MAX) {
                return None;
            }
            (value.ceil() as u32).checked_mul(scale)
        };
        match (edge(width), edge(height)) {
            (Some(width), Some(height)) => Self::new(width, height),
            _ => Err(DomainError::Invalid(
                "Background physical size is invalid".into(),
            )),
        }
    }
    pub fn size(self) -> (u32, u32) {
        (self.width, self.height)
    }
}
/// Presentation asset boundary: rendering data belongs here, not in the domain.
pub trait AssetPort: Send + Sync {
    fn initial(&self) -> BTreeMap<String, Arc<ImageData>>;
    fn application_icons(&self, apps: &[Application]) -> Images;
    /// Decode on a worker; both dimensions must be at most PREVIEW_DIMENSION.
    fn thumbnail(&self, path: &str) -> Result<Arc<ImageData>>;
    fn preview(&self, image: &ImageData) -> Arc<ImageData>;
    /// Retain sufficient pixels for cover at this physical size. None preserves
    /// source preparation until the compositor supplies geometry. Keep decoding
    /// limits in the adapter; target dimensions do not authorize larger sources.
    fn background(&self, path: &str, target: Option<BackgroundTarget>) -> Result<Arc<ImageData>>;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_target_uses_physical_pixels_and_rejects_invalid_or_overflowing_geometry() {
        assert_eq!(
            BackgroundTarget::from_surface(960., 540., 2)
                .unwrap()
                .size(),
            (1920, 1080)
        );
        assert_eq!(
            BackgroundTarget::from_surface(1920., 1080., 1)
                .unwrap()
                .size(),
            (1920, 1080)
        );
        for (width, height, scale) in [
            (0., 10., 1),
            (10., -1., 1),
            (f32::NAN, 10., 1),
            (f32::INFINITY, 10., 1),
            (10., 10., 0),
            (1_000_000_000., 10., 10),
        ] {
            assert!(BackgroundTarget::from_surface(width, height, scale).is_err());
        }
        assert!(BackgroundTarget::new(1, 0).is_err());
    }
}
