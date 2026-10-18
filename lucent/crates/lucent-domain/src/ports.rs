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
pub trait WeatherPort: Send + Sync {
    fn snapshot(&self) -> Result<WeatherSnapshot>;
}
pub trait WallpaperPort: Send + Sync {
    fn list(&self) -> Vec<Wallpaper>;
    fn current(&self) -> Result<Option<WallpaperAsset>>;
    /// Snapshot source bytes without changing the selection. Callers must decode
    /// the returned asset before committing, then discard failed/stale candidates.
    fn stage(&self, source: &Wallpaper) -> Result<WallpaperAsset>;
    fn commit(&self, asset: &WallpaperAsset) -> Result<()>;
    fn discard(&self, asset: &WallpaperAsset) -> Result<()>;
}
/// Advisory observation for background work. It never grants authentication or
/// permission to release a secure session lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionLockState {
    Locked,
    Unlocked,
}
pub trait SessionPort: Send + Sync {
    fn lock(&self) -> Result<()>;
    /// Report changes and observation failures until cancelled. A failed read
    /// does not establish that a previously locked session is now unlocked.
    fn watch_lock(&self, emit: &mut dyn FnMut(Result<SessionLockState>), stop: &dyn StopSignal);
}
