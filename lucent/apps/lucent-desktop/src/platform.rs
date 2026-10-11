//! Composition root: the only desktop module that chooses production adapters.
use crate::{adapters::DesktopAdapters, ports::*};
use lucent_api::ImageData;
use lucent_domain::*;
use lucent_services::{
    JsonSettings, applications::XdgApplications, compositor::Hyprland, desktop_adapters::*, images,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
pub fn desktop_adapters() -> DesktopAdapters {
    DesktopAdapters {
        apps: Arc::new(XdgApplications),
        compositor: Hyprland::from_env()
            .ok()
            .map(|h| Arc::new(h) as Arc<dyn CompositorPort>),
        settings: Arc::new(JsonSettings::default()),
        theme: Arc::new(LucentTheme),
        palette_generation: Arc::new(lucent_services::wallpaper_catalog::ImagePalette),
        catalog: Arc::new(lucent_services::wallpaper_catalog::OnlineWallpapers),
        clock: Arc::new(LocalClock),
        system: Arc::new(LinuxSystem::default()),
        audio: Arc::new(WirePlumber),
        media: Arc::new(Playerctl),
        weather: Arc::new(OpenMeteo),
        wallpaper: Arc::new(LucentWallpaper),
        session: Arc::new(LucentSession),
        assets: Arc::new(NativeAssets),
        notifications: Arc::new(
            lucent_services::notifications::FreedesktopNotifications::default(),
        ),
    }
}
pub struct NativeAssets;
impl AssetPort for NativeAssets {
    fn preview(&self, image: &ImageData) -> Arc<ImageData> {
        Arc::new(image.downsample(PREVIEW_DIMENSION))
    }
    fn thumbnail(&self, path: &str) -> Result<Arc<ImageData>> {
        images::try_load(std::path::Path::new(path), PREVIEW_DIMENSION)
            .map_err(|error| DomainError::Failed(error.to_string()))
    }
    fn background(&self, path: &str) -> Result<Arc<ImageData>> {
        images::try_load(std::path::Path::new(path), 4096)
            .map_err(|error| DomainError::Failed(error.to_string()))
    }
    fn initial(&self) -> BTreeMap<String, Arc<lucent_api::ImageData>> {
        let mut result = BTreeMap::new();
        for name in [
            "apps",
            "search",
            "wallpaper",
            "widgets",
            "palette",
            "power",
            "close",
            "left",
            "right",
            "music",
            "play",
            "pause",
            "next",
            "previous",
            "volume",
            "network",
            "sun",
            "cloud",
            "lock",
            "command",
            "notifications",
        ] {
            result.insert(format!("symbol:{name}"), images::symbol(name, "#ffffff"));
        }
        result.insert(
            "app:fallback".into(),
            images::svg(
                lucent_design::app_icons::FALLBACK.svg,
                images::ICON_PIXELS,
                "lucent-app:application".into(),
            )
            .expect("bundled fallback icon"),
        );
        result
    }
    fn application_icons(&self, apps: &[Application]) -> Images {
        let mut result = vec![];
        let mut seen = BTreeSet::new();
        for app in apps {
            if let Some(asset) = lucent_design::app_icons::lookup(app.id.as_str())
                && let Some(image) = images::svg(
                    asset.svg,
                    images::ICON_PIXELS,
                    format!("lucent-app:{}", asset.id),
                )
            {
                result.push((format!("app:{}", app.id.as_str()), image));
            } else if seen.insert(app.icon.clone())
                && let Some(image) = images::icon(&app.icon)
            {
                result.push((app.icon.clone(), image));
            }
        }
        result
    }
}
