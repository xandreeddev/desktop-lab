//! Launcher interaction state and selector presentation.
//!
//! Borrowed inputs carry current desktop snapshots. This child owns query,
//! selection, scrolling and panel motion; it returns typed intents to the root,
//! which owns persistence and shared OS effects. No production adapter is selected
//! here, and neither domain policy nor the renderer depends on this component.
mod view;
use crate::{
    actions::{Action, COMMANDS},
    shell_layout, wallpaper_browser,
    widget_catalog::WIDGETS,
};
use lucent_api::*;
use lucent_design::{component::panel, motion};
use lucent_domain::{AppId, Application, DesktopSettings, LauncherSize, ThemePalette, Wallpaper};
pub use view::View;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Apps,
    Commands,
    Wallpapers,
    Themes,
    Widgets,
}
impl Mode {
    pub const ALL: [Self; 5] = [
        Self::Apps,
        Self::Commands,
        Self::Wallpapers,
        Self::Themes,
        Self::Widgets,
    ];
    pub fn cycle(self, delta: i32) -> Self {
        let index = Self::ALL.iter().position(|mode| *mode == self).unwrap() as i64;
        Self::ALL[(index + i64::from(delta)).rem_euclid(Self::ALL.len() as i64) as usize]
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Apps => "Apps",
            Self::Commands => "Commands",
            Self::Wallpapers => "Wallpapers",
            Self::Themes => "Themes",
            Self::Widgets => "Widgets",
        }
    }
}

#[derive(Clone)]
pub enum Message {
    Toggle,
    Close,
    Mode(Mode),
    Query(String),
    Select(usize),
    Navigate(i32),
    Activate,
    Request(Intent),
}

#[derive(Clone)]
pub enum Intent {
    Launch(AppId, bool),
    Action(Action),
    Wallpaper(usize, bool),
    Browser(wallpaper_browser::Message),
    ToggleWidget(String),
    ResetLayout,
    Palette(ThemePalette),
    ExtractPalette,
    LauncherSize(LauncherSize),
}

/// Borrowed feature data. Rendering cannot mutate settings or execute a port.
#[derive(Clone, Copy)]
pub struct Input<'a> {
    pub apps: &'a [Application],
    pub settings: &'a DesktopSettings,
    pub palettes: &'a [ThemePalette],
    pub wallpapers: &'a [Wallpaper],
    pub wallpaper_index: usize,
    pub browser: &'a wallpaper_browser::Browser,
}

/// Client layout constraints, kept out of domain models and rendering backends.
#[derive(Clone, Copy)]
pub struct Geometry {
    pub viewport: (f32, f32),
    pub preferred: LauncherSize,
    pub dock_width: f32,
    pub palettes: usize,
    pub wallpapers: usize,
}

pub struct Launcher {
    open: bool,
    mode: Mode,
    query: String,
    results: Vec<usize>,
    selected: usize,
    scroll: usize,
    reveal: Motion,
    panel_width: Motion,
    panel_height: Motion,
}
impl Default for Launcher {
    fn default() -> Self {
        Self {
            open: false,
            mode: Mode::Apps,
            query: String::new(),
            results: vec![],
            selected: 0,
            scroll: 0,
            reveal: Motion::fixed(0.),
            panel_width: Motion::fixed(panel::INITIAL_WIDTH),
            panel_height: Motion::fixed(panel::DOCK_HEIGHT),
        }
    }
}
impl Launcher {
    pub fn open(&self) -> bool {
        self.open
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn result_count(&self) -> usize {
        self.results.len()
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn reveal(&self) -> Motion {
        self.reveal
    }
    pub fn panel_width(&self) -> Motion {
        self.panel_width
    }
    pub fn panel_height(&self) -> Motion {
        self.panel_height
    }
    pub fn animating(&self, now: f64) -> bool {
        [self.reveal, self.panel_width, self.panel_height]
            .iter()
            .any(|m| m.active(now))
    }
    #[cfg(test)]
    pub fn fixture_query(&mut self, value: String) {
        self.query = value;
    }

    /// Discovery refreshes the active query, then clamps selection before paint.
    pub fn refresh(&mut self, apps: &[Application], geometry: Geometry, now: f64) {
        self.results = lucent_usecases::search_applications(apps, &self.query);
        self.reconcile(geometry, now);
    }
    pub fn reconcile(&mut self, geometry: Geometry, now: f64) {
        self.choose(self.selected, geometry);
        let (width, height) = if self.open {
            shell_layout::launcher_size(
                self.mode,
                self.results.len(),
                geometry.viewport,
                geometry.preferred,
            )
        } else {
            (geometry.dock_width, panel::DOCK_HEIGHT)
        };
        self.panel_width
            .target(width, now, motion::PANEL, motion::SPATIAL);
        self.panel_height
            .target(height, now, motion::PANEL, motion::SPATIAL);
        self.reveal.target(
            if self.open { 1. } else { 0. },
            now,
            if self.open {
                motion::ENTER
            } else {
                motion::EXIT
            },
            motion::DECELERATE,
        );
    }
    fn choose(&mut self, index: usize, geometry: Geometry) {
        let count = match self.mode {
            Mode::Apps => self.results.len(),
            Mode::Commands => COMMANDS.len(),
            Mode::Widgets => WIDGETS.len() + 1,
            Mode::Themes => geometry.palettes + 1 + shell_layout::LAUNCHER_PRESETS.len(),
            Mode::Wallpapers => geometry.wallpapers,
        };
        self.selected = index.min(count.saturating_sub(1));
        let mut rows = shell_layout::visible_rows(
            self.mode,
            self.results.len(),
            geometry.viewport,
            geometry.preferred,
        );
        if self.mode == Mode::Themes {
            let (_, height) =
                shell_layout::launcher_size(self.mode, 0, geometry.viewport, geometry.preferred);
            rows = shell_layout::palette_rows(
                height - lucent_design::component::dock::CONTENT_INSET * 2.,
            );
        }
        let row_count = match self.mode {
            Mode::Themes => geometry.palettes + 1,
            Mode::Widgets => WIDGETS.len(),
            _ => count,
        };
        let selected = self.selected.min(row_count.saturating_sub(1));
        self.scroll = self
            .scroll
            .min(selected)
            .max((selected + 1).saturating_sub(rows))
            .min(row_count.saturating_sub(rows));
    }
    fn open_mode(&mut self, mode: Mode, input: Input<'_>, geometry: Geometry, now: f64) {
        self.open = true;
        self.mode = mode;
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
        self.refresh(input.apps, geometry, now);
    }
    fn activate(&self, input: Input<'_>) -> Option<Intent> {
        match self.mode {
            Mode::Apps => self
                .results
                .get(self.selected)
                .and_then(|index| input.apps.get(*index))
                .map(|app| Intent::Launch(app.id.clone(), false)),
            Mode::Commands => COMMANDS
                .get(self.selected)
                .map(|(_, action, _)| Intent::Action(*action)),
            Mode::Widgets => Some(
                WIDGETS
                    .get(self.selected)
                    .map(|(id, _, _)| Intent::ToggleWidget((*id).into()))
                    .unwrap_or(Intent::ResetLayout),
            ),
            Mode::Themes => {
                if let Some(palette) = input.palettes.get(self.selected) {
                    Some(Intent::Palette(palette.clone()))
                } else if self.selected == input.palettes.len() {
                    Some(Intent::ExtractPalette)
                } else {
                    self.selected
                        .checked_sub(input.palettes.len() + 1)
                        .and_then(|index| shell_layout::LAUNCHER_PRESETS.get(index))
                        .map(|(_, size)| Intent::LauncherSize(*size))
                }
            }
            Mode::Wallpapers if input.browser.source.is_some() => Some(Intent::Browser(
                if input.browser.items.is_empty() || input.browser.query != input.browser.submitted
                {
                    wallpaper_browser::Message::Search(1)
                } else {
                    wallpaper_browser::Message::Download(true)
                },
            )),
            Mode::Wallpapers => Some(Intent::Wallpaper(input.wallpaper_index, true)),
        }
    }
    pub fn update(
        &mut self,
        message: Message,
        input: Input<'_>,
        geometry: Geometry,
        effects: &mut Effects<Message>,
    ) -> Option<Intent> {
        let now = effects.now;
        let intent = match message {
            Message::Toggle if self.open => {
                self.open = false;
                self.reconcile(geometry, now);
                None
            }
            Message::Toggle => {
                self.open_mode(Mode::Apps, input, geometry, now);
                None
            }
            Message::Close => {
                self.open = false;
                self.reconcile(geometry, now);
                None
            }
            Message::Mode(mode) => {
                self.open_mode(mode, input, geometry, now);
                None
            }
            Message::Query(query) => {
                self.query = query;
                self.selected = 0;
                self.scroll = 0;
                self.refresh(input.apps, geometry, now);
                None
            }
            Message::Select(index) => {
                self.choose(index, geometry);
                None
            }
            Message::Navigate(delta)
                if self.mode == Mode::Wallpapers && input.browser.source.is_some() =>
            {
                Some(Intent::Browser(wallpaper_browser::Message::Navigate(delta)))
            }
            Message::Navigate(delta) if self.mode == Mode::Wallpapers => Some(Intent::Wallpaper(
                input
                    .wallpaper_index
                    .saturating_add_signed(delta as isize)
                    .min(input.wallpapers.len().saturating_sub(1)),
                false,
            )),
            Message::Navigate(delta) => {
                self.choose(
                    self.selected.saturating_add_signed(delta as isize),
                    geometry,
                );
                None
            }
            Message::Activate => self.activate(input),
            Message::Request(intent) => Some(intent),
        };
        effects.redraw("dock");
        intent
    }
    pub fn event(&self, event: Event, online_wallpapers: bool) -> Option<Message> {
        if !self.open {
            return None;
        }
        match event {
            Event::Outside { surface: "dock" } => Some(Message::Close),
            Event::Key {
                surface: "dock",
                key,
            } => match key {
                Key::Tab => Some(Message::Mode(self.mode.cycle(1))),
                Key::BackTab => Some(Message::Mode(self.mode.cycle(-1))),
                Key::Escape => Some(Message::Close),
                Key::Enter => Some(Message::Activate),
                Key::Up => Some(Message::Navigate(-1)),
                Key::Left
                    if self.mode == Mode::Themes
                        || (self.mode == Mode::Wallpapers && !online_wallpapers) =>
                {
                    Some(Message::Navigate(-1))
                }
                Key::Down => Some(Message::Navigate(1)),
                Key::Right
                    if self.mode == Mode::Themes
                        || (self.mode == Mode::Wallpapers && !online_wallpapers) =>
                {
                    Some(Message::Navigate(1))
                }
                Key::Home if self.mode == Mode::Wallpapers => {
                    Some(Message::Request(Intent::Wallpaper(0, false)))
                }
                _ => None,
            },
            Event::Scroll {
                surface: "dock",
                lines,
            } => Some(Message::Navigate(if lines > 0. { 1 } else { -1 })),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_domain::LaunchCommand;
    struct Fixture {
        apps: Vec<Application>,
        settings: DesktopSettings,
        browser: wallpaper_browser::Browser,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                apps: ["Alpha", "Beta"]
                    .into_iter()
                    .map(|name| Application {
                        id: AppId::new(format!("{name}.desktop")).unwrap(),
                        name: name.into(),
                        description: String::new(),
                        keywords: vec![],
                        icon: String::new(),
                        startup_class: String::new(),
                        command: LaunchCommand {
                            program: "never-executed".into(),
                            args: vec![],
                            directory: None,
                            terminal: false,
                        },
                    })
                    .collect(),
                settings: DesktopSettings::default(),
                browser: Default::default(),
            }
        }
        fn input(&self) -> Input<'_> {
            Input {
                apps: &self.apps,
                settings: &self.settings,
                palettes: lucent_design::palettes(),
                wallpapers: &[],
                wallpaper_index: 0,
                browser: &self.browser,
            }
        }
        fn geometry(&self) -> Geometry {
            Geometry {
                viewport: (800., 600.),
                preferred: self.settings.launcher,
                dock_width: panel::INITIAL_WIDTH,
                palettes: lucent_design::palettes().len(),
                wallpapers: 0,
            }
        }
        fn send(&self, launcher: &mut Launcher, message: Message) -> Option<Intent> {
            launcher.update(
                message,
                self.input(),
                self.geometry(),
                &mut Effects::default(),
            )
        }
    }
    #[test]
    fn query_selection_and_activation_are_owned_by_the_child() {
        let fixture = Fixture::new();
        let mut launcher = Launcher::default();
        fixture.send(&mut launcher, Message::Mode(Mode::Apps));
        fixture.send(&mut launcher, Message::Query("beta".into()));
        fixture.send(&mut launcher, Message::Navigate(i32::MAX));
        assert_eq!(launcher.selected(), 0);
        assert_eq!(launcher.result_count(), 1);
        assert!(matches!(fixture.send(&mut launcher, Message::Activate),
            Some(Intent::Launch(id, false)) if id.as_str() == "Beta.desktop"));
        // An intent is data: the parent coordinates launching and closing.
        assert!(launcher.open());
        fixture.send(&mut launcher, Message::Query("missing".into()));
        assert!(fixture.send(&mut launcher, Message::Activate).is_none());
        fixture.send(&mut launcher, Message::Toggle);
        fixture.send(&mut launcher, Message::Toggle);
        assert!(launcher.query().is_empty());
        assert_eq!(launcher.result_count(), 2);
    }
    #[test]
    fn discovery_refresh_keeps_the_query_and_removes_stale_results() {
        let mut fixture = Fixture::new();
        let mut launcher = Launcher::default();
        fixture.send(&mut launcher, Message::Mode(Mode::Apps));
        fixture.send(&mut launcher, Message::Query("beta".into()));
        fixture.apps.clear();
        launcher.refresh(&fixture.apps, fixture.geometry(), 1.);
        assert_eq!(launcher.query(), "beta");
        assert_eq!(launcher.result_count(), 0);
        assert!(fixture.send(&mut launcher, Message::Activate).is_none());
    }
    #[test]
    fn interrupted_panel_motion_stays_continuous_and_settles() {
        let fixture = Fixture::new();
        let mut launcher = Launcher::default();
        fixture.send(&mut launcher, Message::Mode(Mode::Apps));
        let now = 0.08;
        let width = launcher.panel_width().value(now);
        let height = launcher.panel_height().value(now);
        let reveal = launcher.reveal().value(now);
        let mut effects = Effects {
            now,
            ..Effects::default()
        };
        launcher.update(
            Message::Close,
            fixture.input(),
            fixture.geometry(),
            &mut effects,
        );
        assert_eq!(launcher.panel_width().value(now), width);
        assert_eq!(launcher.panel_height().value(now), height);
        assert_eq!(launcher.reveal().value(now), reveal);
        assert!(!launcher.animating(10.));
        assert_eq!(launcher.panel_height().value(10.), panel::DOCK_HEIGHT);
    }
    #[test]
    fn child_routes_only_its_surface_and_maps_all_modes_to_typed_intents() {
        let fixture = Fixture::new();
        let mut launcher = Launcher::default();
        let event = || Event::Key {
            surface: "dock",
            key: Key::Tab,
        };
        assert!(launcher.event(event(), false).is_none());
        fixture.send(&mut launcher, Message::Mode(Mode::Apps));
        assert!(
            launcher
                .event(
                    Event::Key {
                        surface: "bar",
                        key: Key::Tab
                    },
                    false
                )
                .is_none()
        );
        let message = launcher.event(event(), false).unwrap();
        fixture.send(&mut launcher, message);
        assert!(matches!(
            fixture.send(&mut launcher, Message::Activate),
            Some(Intent::Action(Action::Terminal))
        ));
        fixture.send(&mut launcher, Message::Mode(Mode::Widgets));
        fixture.send(&mut launcher, Message::Select(usize::MAX));
        assert!(matches!(
            fixture.send(&mut launcher, Message::Activate),
            Some(Intent::ResetLayout)
        ));
        fixture.send(&mut launcher, Message::Mode(Mode::Themes));
        fixture.send(
            &mut launcher,
            Message::Select(lucent_design::palettes().len()),
        );
        assert!(matches!(
            fixture.send(&mut launcher, Message::Activate),
            Some(Intent::ExtractPalette)
        ));
        fixture.send(&mut launcher, Message::Navigate(1));
        assert!(matches!(
            fixture.send(&mut launcher, Message::Activate),
            Some(Intent::LauncherSize(_))
        ));
        assert_eq!(Mode::Apps.cycle(i32::MAX).cycle(-i32::MAX), Mode::Apps);
    }
}
