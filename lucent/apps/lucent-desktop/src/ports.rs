//! Dependencies supplied by the executable. Components never select OS adapters.
use lucent_api::{Cancellation, ImageData};
use lucent_domain::*;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
pub type Images = Vec<(String, Arc<ImageData>)>;
/// Presentation asset boundary: rendering data belongs here, not in the domain.
pub trait AssetPort: Send + Sync {
    fn initial(&self) -> BTreeMap<String, Arc<ImageData>>;
    fn load(&self, apps: &[Application], wallpapers: &[Wallpaper]) -> Images;
}
#[derive(Clone)]
pub struct DesktopPorts {
    pub apps: Arc<dyn ApplicationPort>,
    pub compositor: Option<Arc<dyn CompositorPort>>,
    pub settings: Arc<dyn SettingsPort>,
    pub clock: Arc<dyn ClockPort>,
    pub system: Arc<dyn SystemPort>,
    pub audio: Arc<dyn AudioPort>,
    pub media: Arc<dyn MediaPort>,
    pub weather: Arc<dyn WeatherPort>,
    pub wallpaper: Arc<dyn WallpaperPort>,
    pub session: Arc<dyn SessionPort>,
    pub notifications: Arc<dyn NotificationPort>,
    pub assets: Arc<dyn AssetPort>,
}
pub struct WatchStop(pub Cancellation);
impl StopSignal for WatchStop {
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
    fn wait(&self, duration: Duration) {
        self.0.sleep(duration);
    }
}
