//! Composition root: the only desktop module that chooses production adapters.
use crate::{adapters::DesktopAdapters, ports::*};
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
        clock: Arc::new(LocalClock),
        system: Arc::new(LinuxSystem::default()),
        audio: Arc::new(WirePlumber),
        media: Arc::new(Playerctl),
        weather: Arc::new(OpenMeteo),
        wallpaper: Arc::new(OmarchyWallpaper),
        session: Arc::new(OmarchySession),
        assets: Arc::new(NativeAssets),
        notifications: Arc::new(
            lucent_services::notifications::FreedesktopNotifications::default(),
        ),
    }
}
pub struct NativeAssets;
impl AssetPort for NativeAssets {
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
    fn load(&self, apps: &[Application], wallpapers: &[Wallpaper]) -> Images {
        let mut result = vec![];
        let mut seen = BTreeSet::new();
        for app in apps {
            if let Some(asset) = lucent_design::app_icons::lookup(&app.id.0)
                && let Some(image) = images::svg(
                    asset.svg,
                    images::ICON_PIXELS,
                    format!("lucent-app:{}", asset.id),
                )
            {
                result.push((format!("app:{}", app.id.0), image));
            } else if seen.insert(app.icon.clone())
                && let Some(image) = images::icon(&app.icon)
            {
                result.push((app.icon.clone(), image));
            }
        }
        for wall in wallpapers.iter().take(64) {
            if let Some(image) =
                images::load(std::path::Path::new(&wall.path), images::PREVIEW_PIXELS)
            {
                result.push((wall.path.clone(), image));
            }
        }
        result
    }
}
