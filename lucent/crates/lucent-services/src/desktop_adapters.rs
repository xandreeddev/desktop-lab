//! Command/probe adapters selected by the executable's composition root.
use lucent_domain::*;
use std::sync::Mutex;
#[derive(Default)]
pub struct LocalClock;
impl ClockPort for LocalClock {
    fn now(&self) -> ClockSnapshot {
        crate::clock()
    }
}
#[derive(Default)]
pub struct LinuxSystem(Mutex<crate::SystemProbe>);
impl SystemPort for LinuxSystem {
    fn sample(&self) -> Result<SystemSnapshot> {
        self.0
            .lock()
            .map_err(|_| DomainError::Failed("System probe lock poisoned".into()))?
            .sample()
    }
}
pub struct OpenMeteo;
impl WeatherPort for OpenMeteo {
    fn snapshot(&self) -> Result<WeatherSnapshot> {
        crate::weather()
    }
}
pub struct LucentTheme;
impl ThemePort for LucentTheme {
    fn apply(&self, palette: &ThemePalette) -> Result<()> {
        palette.validate()?;
        let home = std::env::var("HOME")
            .map_err(|_| DomainError::Unavailable("Missing home directory".into()))?;
        crate::command_with_timeout(
            "python3",
            &[
                &format!("{home}/.local/lib/lucent/theme.py"),
                "palette",
                &serde_json::to_string(palette).map_err(|e| DomainError::Invalid(e.to_string()))?,
            ],
            std::time::Duration::from_secs(45),
        )
        .map(|_| ())
    }
}
pub struct LucentWallpaper;
impl WallpaperPort for LucentWallpaper {
    fn list(&self) -> Vec<Wallpaper> {
        crate::wallpapers()
    }
    fn current(&self) -> Result<Option<WallpaperAsset>> {
        crate::wallpapers::WallpaperStore::default().current()
    }
    fn stage(&self, source: &Wallpaper) -> Result<WallpaperAsset> {
        crate::wallpapers::WallpaperStore::default().stage(source)
    }
    fn commit(&self, asset: &WallpaperAsset) -> Result<()> {
        crate::wallpapers::WallpaperStore::default().commit(asset)
    }
    fn discard(&self, asset: &WallpaperAsset) -> Result<()> {
        crate::wallpapers::WallpaperStore::default().discard(asset)
    }
}
