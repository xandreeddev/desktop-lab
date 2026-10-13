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
        audio: Arc::new(lucent_services::audio::PulseAudio::default()),
        network: Arc::new(lucent_services::network::NetworkManager::default()),
        bluetooth: Arc::new(lucent_services::bluetooth::BlueZ::default()),
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
    fn background(&self, path: &str, target: Option<BackgroundTarget>) -> Result<Arc<ImageData>> {
        let image = images::try_load(std::path::Path::new(path), 4096)
            .map_err(|error| DomainError::Failed(error.to_string()))?;
        if let Some(target) = target {
            let (width, height) = target.size();
            return image
                .downsample_for_cover(width, height)
                .map(Arc::new)
                .map_err(|error| DomainError::Failed(error.to_string()));
        }
        Ok(image)
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
            "bluetooth",
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
            images::svg_mask(
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
                && let Some(image) = images::svg_mask(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_preparation_retains_physical_density_without_changing_source() {
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "lucent-background-{}-{nonce}.png",
            std::process::id()
        )));
        image::RgbaImage::from_pixel(80, 48, image::Rgba([30, 60, 90, 255]))
            .save(&fixture.0)
            .unwrap();
        let original = std::fs::read(&fixture.0).unwrap();
        let path = fixture.0.to_str().unwrap();
        let source = NativeAssets.background(path, None).unwrap();
        for (width, height) in [(20, 12), (40, 24)] {
            let prepared = NativeAssets
                .background(path, Some(BackgroundTarget::new(width, height).unwrap()))
                .unwrap();
            assert_eq!((prepared.width(), prepared.height()), (width, height));
            assert_eq!(prepared.aspect_size(), (80, 48));
            assert!(prepared.bytes() < source.bytes());
        }
        for (width, height) in [(20, 48), (160, 96)] {
            let prepared = NativeAssets
                .background(path, Some(BackgroundTarget::new(width, height).unwrap()))
                .unwrap();
            assert_eq!((prepared.width(), prepared.height()), (80, 48));
        }
        assert_eq!(std::fs::read(&fixture.0).unwrap(), original);
    }
}
