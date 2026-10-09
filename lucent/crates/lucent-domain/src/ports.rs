//! Outbound service contracts. No renderer, transport, process or filesystem types.
use crate::*;
use std::time::Duration;

/// Cooperative cancellation for long-lived adapters, independent of any UI runtime.
pub trait StopSignal: Send + Sync {
    fn cancelled(&self) -> bool;
    fn wait(&self, duration: Duration);
}
pub trait ClockPort: Send + Sync {
    fn now(&self) -> ClockSnapshot;
}
pub trait SystemPort: Send + Sync {
    fn sample(&self) -> Result<SystemSnapshot>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCommand {
    Raise,
    Lower,
    ToggleMute,
}
pub trait AudioPort: Send + Sync {
    fn control(&self, command: AudioCommand) -> Result<()>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
}
pub trait MediaPort: Send + Sync {
    fn snapshot(&self) -> Result<MediaSnapshot>;
    fn control(&self, command: MediaCommand) -> Result<()>;
}
pub trait WeatherPort: Send + Sync {
    fn snapshot(&self) -> Result<WeatherSnapshot>;
}
pub trait WallpaperPort: Send + Sync {
    fn list(&self) -> Vec<Wallpaper>;
    fn current(&self) -> String;
    fn apply(&self, path: &str) -> Result<()>;
}
pub trait SessionPort: Send + Sync {
    fn lock(&self) -> Result<()>;
}
