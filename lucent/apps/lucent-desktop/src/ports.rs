//! Client presentation contracts. OS capability contracts live in lucent-domain.
use lucent_api::ImageData;
use lucent_domain::*;
use std::{collections::BTreeMap, sync::Arc};
pub type Images = Vec<(String, Arc<ImageData>)>;
/// Prepared previews retain density for the current cards through 2× scale.
pub const PREVIEW_DIMENSION: u32 = 1024;
/// Presentation asset boundary: rendering data belongs here, not in the domain.
pub trait AssetPort: Send + Sync {
    fn initial(&self) -> BTreeMap<String, Arc<ImageData>>;
    fn application_icons(&self, apps: &[Application]) -> Images;
    /// Decode on a worker; both dimensions must be at most PREVIEW_DIMENSION.
    fn thumbnail(&self, path: &str) -> Result<Arc<ImageData>>;
    fn preview(&self, image: &ImageData) -> Arc<ImageData>;
    fn background(&self, path: &str) -> Result<Arc<ImageData>>;
}
