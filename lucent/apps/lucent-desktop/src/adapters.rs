//! Injected implementations, held through domain and presentation port contracts.
//! The composition root selects concrete adapters; components receive this bundle.
use crate::ports::AssetPort;
use lucent_api::Cancellation;
use lucent_domain::*;
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub struct DesktopAdapters {
    pub apps: Arc<dyn ApplicationPort>,
    pub compositor: Option<Arc<dyn CompositorPort>>,
    pub settings: Arc<dyn SettingsPort>,
    pub theme: Arc<dyn ThemePort>,
    pub palette_generation: Arc<dyn PaletteGenerationPort>,
    pub catalog: Arc<dyn WallpaperCatalogPort>,
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

/// Adapts the framework's cancellation handle to the domain stop-signal contract.
pub struct WatchStop(pub Cancellation);
impl StopSignal for WatchStop {
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
    fn wait(&self, duration: Duration) {
        self.0.sleep(duration);
    }
}
