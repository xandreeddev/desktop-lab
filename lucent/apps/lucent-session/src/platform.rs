//! Session presentation adapters; binaries choose the source, components see pixels.
use crate::SessionAssets;
use lucent_api::ImageData;
use std::{path::PathBuf, sync::Arc};

/// Retains source detail up to a 4K display; decoding is bounded by the image adapter.
const WALLPAPER_PIXELS: u32 = 4096;
pub struct WallpaperFile(pub PathBuf);
impl WallpaperFile {
    pub fn current() -> Self {
        Self(PathBuf::from(lucent_services::current_wallpaper()))
    }
    pub fn greeter() -> Self {
        Self(
            std::env::var_os("LUCENT_GREETER_WALLPAPER")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/etc/greetd/lucent-wallpaper")),
        )
    }
}
impl SessionAssets for WallpaperFile {
    fn wallpaper(&self) -> Option<Arc<ImageData>> {
        lucent_services::images::load(&self.0, WALLPAPER_PIXELS)
    }
}
