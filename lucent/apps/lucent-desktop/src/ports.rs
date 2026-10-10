//! Client presentation contracts. OS capability contracts live in lucent-domain.
use lucent_api::ImageData;
use lucent_domain::*;
use std::{collections::BTreeMap, sync::Arc};
pub type Images = Vec<(String, Arc<ImageData>)>;
/// Presentation asset boundary: rendering data belongs here, not in the domain.
pub trait AssetPort: Send + Sync {
    fn initial(&self) -> BTreeMap<String, Arc<ImageData>>;
    fn load(&self, apps: &[Application], wallpapers: &[Wallpaper]) -> Images;
    fn background(&self, path: &str) -> Option<Arc<ImageData>>;
}
