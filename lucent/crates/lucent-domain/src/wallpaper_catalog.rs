//! Online catalogs contain metadata; rendering and local wallpaper activation are separate ports.
use crate::{Result, Wallpaper};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WallpaperProvider {
    Wallhaven,
    AlphaCoders,
}
impl WallpaperProvider {
    pub fn id(self) -> &'static str {
        match self {
            Self::Wallhaven => "wallhaven",
            Self::AlphaCoders => "alpha-coders",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Wallhaven => "Wallhaven",
            Self::AlphaCoders => "Alpha Coders",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteWallpaper {
    pub provider: WallpaperProvider,
    pub id: String,
    pub title: String,
    pub page_url: String,
    pub thumbnail_url: String,
    pub image_url: String,
    pub resolution: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct WallpaperPage {
    pub items: Vec<RemoteWallpaper>,
    pub page: u32,
    pub has_more: bool,
}
pub trait WallpaperCatalogPort: Send + Sync {
    fn search(&self, provider: WallpaperProvider, query: &str, page: u32) -> Result<WallpaperPage>;
    /// Cache a thumbnail locally. AssetPort owns decoding it into presentation pixels.
    fn preview(&self, item: &RemoteWallpaper) -> Result<String>;
    /// Download a validated image without activating it.
    fn download(&self, item: &RemoteWallpaper) -> Result<Wallpaper>;
}
