//! Desktop entities and ports. No compositor, D-Bus, UI or filesystem operations.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AppId(pub String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Application {
    pub id: AppId,
    pub name: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub icon: String,
    pub startup_class: String,
    pub command: LaunchCommand,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    pub directory: Option<String>,
    pub terminal: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub address: String,
    pub app_class: String,
    pub title: String,
    pub workspace: i32,
    pub focused: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: i32,
    pub name: String,
    pub windows: usize,
    pub active: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompositorSnapshot {
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainError {
    Unavailable(String),
    Invalid(String),
    Failed(String),
}
impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(s) | Self::Invalid(s) | Self::Failed(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for DomainError {}
pub type Result<T> = std::result::Result<T, DomainError>;

/// Implementations launch argument arrays directly, never through a shell.
pub trait ApplicationPort: Send + Sync {
    fn discover(&self) -> Result<Vec<Application>>;
    fn launch(&self, app: &Application) -> Result<()>;
}
pub trait CompositorPort: Send + Sync {
    fn snapshot(&self) -> Result<CompositorSnapshot>;
    fn switch_workspace(&self, id: i32) -> Result<()>;
    fn focus_window(&self, address: &str) -> Result<()>;
    fn watch(
        &self,
        emit: &mut dyn FnMut(Result<CompositorSnapshot>),
        stop: &dyn crate::StopSignal,
    ) {
        while !stop.cancelled() {
            emit(self.snapshot());
            stop.wait(std::time::Duration::from_secs(2));
        }
    }
}
pub trait SettingsPort: Send + Sync {
    fn load(&self) -> Result<DesktopSettings>;
    fn save(&self, settings: &DesktopSettings) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopSettings {
    pub version: u32,
    pub positions: BTreeMap<String, Placement>,
    pub visible_widgets: Vec<String>,
    pub notes: String,
    pub pinned: Vec<AppId>,
    pub light: bool,
}
impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            version: 1,
            positions: BTreeMap::new(),
            visible_widgets: ["calendar", "clock", "weather", "media"]
                .map(str::to_owned)
                .into(),
            notes: String::new(),
            pinned: Vec::new(),
            light: false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// Monday = 0.
    pub weekday: u32,
}
impl Date {
    pub fn days_in_month(year: i32, month: u32) -> u32 {
        match month {
            4 | 6 | 9 | 11 => 30,
            2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
            2 => 28,
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            _ => 0,
        }
    }
    pub fn first_weekday(self) -> u32 {
        (self.weekday + 7 - (self.day - 1) % 7) % 7
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClockSnapshot {
    pub date: Date,
    pub hour: u32,
    pub minute: u32,
    pub unix_seconds: u64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemSnapshot {
    pub cpu_percent: f32,
    pub memory_used_mib: u64,
    pub memory_total_mib: u64,
    pub volume: Option<u32>,
    pub muted: bool,
    pub network: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaSnapshot {
    pub player: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub art_url: String,
    pub position: u64,
    pub length: u64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherSnapshot {
    pub temperature: f32,
    pub description: String,
    pub code: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wallpaper {
    pub path: String,
    pub name: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimerPhase {
    #[default]
    Ready,
    Running,
    Paused,
    Finished,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusTimer {
    pub phase: TimerPhase,
    pub remaining: u64,
    deadline: u64,
}
impl Default for FocusTimer {
    fn default() -> Self {
        Self {
            phase: TimerPhase::Ready,
            remaining: 25 * 60,
            deadline: 0,
        }
    }
}
impl FocusTimer {
    pub fn toggle(&mut self, now: u64) {
        self.tick(now);
        match self.phase {
            TimerPhase::Running => self.phase = TimerPhase::Paused,
            TimerPhase::Finished => {
                *self = Self::default();
                self.toggle(now);
            }
            _ => {
                self.phase = TimerPhase::Running;
                self.deadline = now.saturating_add(self.remaining);
            }
        }
    }
    pub fn tick(&mut self, now: u64) {
        if self.phase == TimerPhase::Running {
            self.remaining = self.deadline.saturating_sub(now);
            if self.remaining == 0 {
                self.phase = TimerPhase::Finished;
            }
        }
    }
}
