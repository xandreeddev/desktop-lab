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
pub struct WirePlumber;
impl AudioPort for WirePlumber {
    fn control(&self, command: AudioCommand) -> Result<()> {
        let args: &[&str] = match command {
            AudioCommand::Raise => &["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "5%+"],
            AudioCommand::Lower => &["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"],
            AudioCommand::ToggleMute => &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"],
        };
        crate::command("wpctl", args).map(|_| ())
    }
}
pub struct Playerctl;
impl MediaPort for Playerctl {
    fn snapshot(&self) -> Result<MediaSnapshot> {
        crate::media()
    }
    fn control(&self, command: MediaCommand) -> Result<()> {
        crate::command(
            "playerctl",
            &[match command {
                MediaCommand::PlayPause => "play-pause",
                MediaCommand::Next => "next",
                MediaCommand::Previous => "previous",
            }],
        )
        .map(|_| ())
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
    fn current(&self) -> String {
        crate::current_wallpaper()
    }
    fn apply(&self, path: &str) -> Result<()> {
        crate::apply_wallpaper(path)
    }
}
pub struct LucentSession;
impl SessionPort for LucentSession {
    fn lock(&self) -> Result<()> {
        let home = std::env::var("HOME")
            .map_err(|_| DomainError::Unavailable("Missing home directory".into()))?;
        crate::command("python3", &[&format!("{home}/.local/lib/lucent/lock.py")]).map(|_| ())
    }
}
