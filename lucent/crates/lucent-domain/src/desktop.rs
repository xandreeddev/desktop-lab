//! Desktop entities and ports. No compositor, D-Bus, UI or filesystem operations.
use crate::{Date, MonitorId, WidgetLayout};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AppId(String);
impl AppId {
    /// An XDG desktop-file identity, not a filesystem path or command.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.len() <= ".desktop".len()
            || !value.ends_with(".desktop")
            || value.chars().any(|c| c.is_control() || c == '/')
        {
            return Err(DomainError::Invalid("Application ID must be a non-empty desktop-file identity without path separators or control characters".into()));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for AppId {
    type Error = DomainError;
    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}
impl From<AppId> for String {
    fn from(id: AppId) -> Self {
        id.0
    }
}
#[cfg(test)]
mod app_id_tests {
    use super::*;
    #[test]
    fn desktop_identity_is_validated_by_construction_and_deserialization() {
        use serde::de::value::{Error, StringDeserializer};
        for value in [
            "",
            ".desktop",
            "app",
            "/tmp/app.desktop",
            "app\0.desktop",
            "line\n.desktop",
        ] {
            assert!(AppId::new(value).is_err(), "accepted {value:?}");
            assert!(AppId::deserialize(StringDeserializer::<Error>::new(value.into())).is_err());
        }
        for value in [
            "org.example.App.desktop",
            "Google Contacts.desktop",
            "nested-app.desktop",
        ] {
            let id = AppId::new(value).unwrap();
            assert_eq!(id.as_str(), value);
            assert_eq!(
                AppId::deserialize(StringDeserializer::<Error>::new(value.into())).unwrap(),
                id
            );
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Application {
    pub id: AppId,
    pub name: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub icon: String,
    pub startup_class: String,
    pub launch: ApplicationLaunch,
}
impl Application {
    /// Match an observed compositor class using desktop-entry hints. Missing
    /// classes carry no identity and must never match another missing hint.
    pub fn matches_window_class(&self, class: &str) -> bool {
        !class.is_empty()
            && (self.startup_class.eq_ignore_ascii_case(class)
                || self
                    .id
                    .as_str()
                    .strip_suffix(".desktop")
                    .is_some_and(|id| id.eq_ignore_ascii_case(class)))
    }
}
/// Application launch behavior, independent of the concrete platform transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplicationLaunch {
    Command(LaunchCommand),
    DesktopService { fallback: Option<LaunchCommand> },
}
impl ApplicationLaunch {
    pub fn command(&self) -> Option<&LaunchCommand> {
        match self {
            Self::Command(command) => Some(command),
            Self::DesktopService { fallback } => fallback.as_ref(),
        }
    }
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
    pub workspace: Option<WorkspaceRef>,
    pub focused: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: WorkspaceRef,
    pub name: String,
    pub windows: usize,
    pub active: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompositorSnapshot {
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
    pub monitors: Vec<CompositorMonitor>,
}
/// Observed compositor workspace/focus state, separate from persisted layouts
/// and native output buffers. Multiple outputs may each have a current workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositorMonitor {
    pub id: MonitorId,
    pub focused: bool,
    pub active_workspace: Option<WorkspaceRef>,
    pub special_workspace: Option<WorkspaceRef>,
    /// An observed fullscreen window on this output's currently visible workspace.
    /// Maximized windows and fullscreen windows on hidden workspaces do not count.
    pub has_fullscreen_window: bool,
}
/// A desktop workspace identity. Names are labels in this domain, not native
/// selector expressions. Adapters encode their own numbered/named/special syntax.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspaceRef(WorkspaceIdentity);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum WorkspaceIdentity {
    Numbered(i32),
    Named(String),
    Special(String),
}
impl WorkspaceRef {
    pub fn numbered(id: i32) -> Result<Self> {
        if id < 1 {
            return Err(DomainError::Invalid(
                "Workspace number must be positive".into(),
            ));
        }
        Ok(Self(WorkspaceIdentity::Numbered(id)))
    }
    pub fn named(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        Self::validate_name(&name, false)?;
        Ok(Self(WorkspaceIdentity::Named(name)))
    }
    /// An empty name represents the compositor's default special workspace.
    pub fn special(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        Self::validate_name(&name, true)?;
        Ok(Self(WorkspaceIdentity::Special(name)))
    }
    fn validate_name(name: &str, allow_empty: bool) -> Result<()> {
        if (!allow_empty && name.is_empty())
            || name.len() > 512
            || name.chars().any(char::is_control)
        {
            return Err(DomainError::Invalid(
                "Workspace name must be bounded and contain no controls".into(),
            ));
        }
        Ok(())
    }
    pub fn number(&self) -> Option<i32> {
        match self.0 {
            WorkspaceIdentity::Numbered(id) => Some(id),
            _ => None,
        }
    }
    pub fn name(&self) -> Option<&str> {
        match &self.0 {
            WorkspaceIdentity::Named(name) | WorkspaceIdentity::Special(name) => Some(name),
            WorkspaceIdentity::Numbered(_) => None,
        }
    }
    pub fn is_special(&self) -> bool {
        matches!(self.0, WorkspaceIdentity::Special(_))
    }
}
/// Selecting a regular workspace differs from toggling a special overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceCommand {
    Select(WorkspaceRef),
    ToggleSpecial(WorkspaceRef),
}
impl CompositorSnapshot {
    pub fn monitor(&self, id: &MonitorId) -> Option<&CompositorMonitor> {
        let mut matches = self.monitors.iter().filter(|monitor| &monitor.id == id);
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }
    /// Ambiguous focus is not an authority to choose a display.
    pub fn focused_monitor(&self) -> Option<&MonitorId> {
        let mut focused = self.monitors.iter().filter(|monitor| monitor.focused);
        let first = focused.next()?;
        (focused.next().is_none() && self.monitor(&first.id).is_some()).then_some(&first.id)
    }
}
#[cfg(test)]
mod compositor_identity_tests {
    use super::*;
    #[test]
    fn workspace_references_validate_identity_and_duplicate_monitor_records_are_ambiguous() {
        assert!(WorkspaceRef::numbered(0).is_err());
        for address in ["", "bad\naddress", "bad\0address"] {
            assert!(WorkspaceRef::named(address).is_err());
        }
        assert!(WorkspaceRef::named("x".repeat(513)).is_err());
        assert!(WorkspaceRef::numbered(-99).is_err());
        assert!(WorkspaceRef::special("").unwrap().is_special());
        assert_ne!(
            WorkspaceRef::named("notes").unwrap(),
            WorkspaceRef::special("notes").unwrap()
        );
        assert_eq!(
            WorkspaceRef::special("notes").unwrap().name(),
            Some("notes")
        );
        let id = MonitorId::new("DP-1").unwrap();
        let first = CompositorMonitor {
            id: id.clone(),
            focused: true,
            active_workspace: None,
            special_workspace: None,
            has_fullscreen_window: false,
        };
        let mut duplicate = first.clone();
        duplicate.focused = false;
        let snapshot = CompositorSnapshot {
            monitors: vec![first, duplicate],
            ..Default::default()
        };
        assert!(snapshot.monitor(&id).is_none());
        assert!(snapshot.focused_monitor().is_none());
    }
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
    /// Adapters own catalog updates and observation failure/recovery.
    fn watch(&self, emit: &mut dyn FnMut(Result<Vec<Application>>), stop: &dyn crate::StopSignal);
    fn launch(&self, app: &Application) -> Result<()>;
}
pub trait CompositorPort: Send + Sync {
    fn snapshot(&self) -> Result<CompositorSnapshot>;
    /// Implementations validate command kind before native dispatch. Admission
    /// does not claim the observed workspace state has already changed.
    fn workspace(&self, command: WorkspaceCommand) -> Result<()>;
    fn focus_window(&self, address: &str) -> Result<()>;
    /// Adapters own event delivery, reconnects and any polling strategy.
    fn watch(&self, emit: &mut dyn FnMut(Result<CompositorSnapshot>), stop: &dyn crate::StopSignal);
}
pub trait SettingsPort: Send + Sync {
    fn load(&self) -> Result<DesktopSettings>;
    fn save(&self, settings: &DesktopSettings) -> Result<()>;
}

/// Preferred logical dimensions. Absent values use the client's design defaults.
/// Height is a ceiling: the launcher can shrink to fit its current contents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherSize {
    pub width: Option<u32>,
    pub max_height: Option<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DockSettings {
    pub auto_hide: bool,
    /// An explicit empty pin list differs from the initial default suggestions.
    pub default_pins: bool,
}
impl Default for DockSettings {
    fn default() -> Self {
        Self {
            auto_hide: false,
            default_pins: true,
        }
    }
}
/// UTF-8 byte admission shared by saved preferences and ordinary notes editing.
pub const NOTES_MAX_BYTES: usize = 8192;
pub fn validate_notes(value: &str) -> Result<()> {
    if value.len() > NOTES_MAX_BYTES {
        Err(DomainError::Invalid(format!(
            "Notes exceed the {NOTES_MAX_BYTES}-byte limit"
        )))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopSettings {
    pub version: u32,
    /// The default layout retains the original JSON keys for existing installs.
    #[serde(flatten)]
    pub layout: WidgetLayout,
    /// Disconnected monitor profiles remain available for reconnection.
    pub monitor_layouts: BTreeMap<MonitorId, WidgetLayout>,
    pub notes: String,
    pub pinned: Vec<AppId>,
    pub dock: DockSettings,
    /// Legacy preference, used only when no palette has been selected.
    pub light: bool,
    pub palette: Option<crate::ThemePalette>,
    pub saved_palettes: Vec<crate::ThemePalette>,
    pub launcher: LauncherSize,
    pub live_wallpapers: crate::LiveWallpapers,
}
impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            version: 1,
            layout: WidgetLayout::default(),
            monitor_layouts: BTreeMap::new(),
            notes: String::new(),
            pinned: Vec::new(),
            dock: DockSettings::default(),
            light: false,
            palette: None,
            saved_palettes: Vec::new(),
            launcher: LauncherSize::default(),
            live_wallpapers: Default::default(),
        }
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
}
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherSnapshot {
    pub temperature: f32,
    pub description: String,
    pub code: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wallpaper {
    pub path: String,
    pub name: String,
}
/// A stable copy of a wallpaper source. Image decoding belongs to presentation;
/// selection commits only after that copy has been decoded successfully.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WallpaperAsset {
    pub source: Wallpaper,
    pub path: String,
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
