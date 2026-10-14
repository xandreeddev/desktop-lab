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
    widgets::catalog::WIDGETS,
};
use lucent_api::*;
use lucent_design::{component::panel, motion};
use lucent_domain::{AppId, Application, DesktopSettings, LauncherSize, ThemePalette, Wallpaper};
pub use view::View;
/// The same range drives painting and worker demand; no scan of the full library.
pub fn visible_wallpapers(count: usize, center: f32) -> std::ops::Range<usize> {
    if !center.is_finite() {
        return 0..0;
    }
    let distance = lucent_design::component::wallpaper::VISIBLE_DISTANCE;
    let start = ((center - distance).floor() + 1.).max(0.) as usize;
    let end = (center + distance).ceil().max(0.) as usize;
    start.min(count)..end.min(count)
}
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
    Compacted(u64),
    Submitted { height: f32, now: f64 },
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
    Pin(AppId),
    Action(Action),
    Wallpaper(usize, bool),
    Browser(wallpaper_browser::Message),
    ToggleWidget(String),
    ResetLayout,
    EditWidgets(bool),
    Palette(ThemePalette),
    ExtractPalette,
    LauncherSize(LauncherSize),
}

/// Borrowed feature data. Rendering cannot mutate settings or execute a port.
#[derive(Clone, Copy)]
pub struct Input<'a> {
    pub apps: &'a [Application],
    pub application_status: crate::catalog::Status,
    pub widgets_editing: bool,
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
    expanded: bool,
    opening: bool,
    submitted_height: f32,
    full_height: f32,
    edit_mounted: bool,
    close_revision: u64,
    closing: Option<CloseRequest>,
    mode: Mode,
    query: String,
    results: Vec<usize>,
    selected: usize,
    scroll: usize,
    reveal: Motion,
    panel_width: Motion,
    panel_height: Motion,
}
#[derive(Clone, Copy)]
struct CloseRequest {
    revision: u64,
}
impl Default for Launcher {
    fn default() -> Self {
        Self {
            open: false,
            expanded: false,
            opening: false,
            submitted_height: 0.,
            full_height: 0.,
            edit_mounted: false,
            close_revision: 0,
            closing: None,
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
    /// Keep the native surface large throughout the closing morph. A
    /// successful settled closing submission may shrink it.
    pub fn expanded(&self) -> bool {
        self.expanded
    }
    pub fn opening(&self) -> bool {
        self.opening
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
    pub fn selected_application<'a>(&self, apps: &'a [Application]) -> Option<&'a AppId> {
        self.results
            .get(self.selected)
            .and_then(|index| apps.get(*index))
            .map(|app| &app.id)
    }
    pub fn refresh_preserving(
        &mut self,
        apps: &[Application],
        selected: Option<&AppId>,
        geometry: Geometry,
        now: f64,
    ) {
        self.results = lucent_usecases::search_applications(apps, &self.query);
        if let Some(position) =
            selected.and_then(|id| self.results.iter().position(|index| &apps[*index].id == id))
        {
            self.selected = position;
        }
        self.reconcile(geometry, now);
    }
    pub fn refresh(&mut self, apps: &[Application], geometry: Geometry, now: f64) {
        self.results = lucent_usecases::search_applications(apps, &self.query);
        self.reconcile(geometry, now);
    }
    pub fn reconcile(&mut self, geometry: Geometry, now: f64) {
        self.choose(self.selected, geometry);
        self.full_height = geometry.viewport.1;
        if self.open && self.submitted_height < self.full_height {
            if !self.opening {
                self.freeze(now);
            }
            self.opening = true;
        }
        if self.opening {
            return;
        }
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
            Mode::Widgets => WIDGETS.len() + 2,
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
        self.expanded = true;
        self.closing = None;
        self.edit_mounted = false;
        self.mode = mode;
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
        self.refresh(input.apps, geometry, now);
    }
    fn close(&mut self, geometry: Geometry, now: f64) {
        self.open = false;
        self.opening = false;
        self.reconcile(geometry, now);
        if self.expanded && self.closing.is_none() {
            // Exhaustion retains the large surface. Never reuse a revision
            // that an already queued completion could mistake for new work.
            if let Some(revision) = self.close_revision.checked_add(1) {
                self.close_revision = revision;
                self.closing = Some(CloseRequest { revision });
            }
        }
    }
    fn freeze(&mut self, now: f64) {
        self.panel_width = Motion::fixed(self.panel_width.value(now));
        self.panel_height = Motion::fixed(self.panel_height.value(now));
        self.reveal = Motion::fixed(self.reveal.value(now));
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
                    .map(|entry| Intent::ToggleWidget(entry.id.into()))
                    .unwrap_or_else(|| {
                        if self.selected == WIDGETS.len() {
                            Intent::ResetLayout
                        } else {
                            Intent::EditWidgets(!input.widgets_editing)
                        }
                    }),
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
        if let Message::Submitted {
            height,
            now: painted,
        } = message
        {
            if !height.is_finite() || height <= 0. || !painted.is_finite() {
                return None;
            }
            self.submitted_height = height;
            if self.open && self.opening && height >= geometry.viewport.1 {
                self.opening = false;
                self.reconcile(geometry, now);
                effects.redraw("dock");
            } else if self.open && !self.opening && self.reveal.value(painted) > 0.001 {
                self.edit_mounted = true;
            }
            return None;
        }
        let intent = match message {
            Message::Submitted { .. } => unreachable!(),
            Message::Toggle if self.open => {
                self.close(geometry, now);
                None
            }
            Message::Toggle => {
                self.open_mode(Mode::Apps, input, geometry, now);
                None
            }
            Message::Close => {
                self.close(geometry, now);
                None
            }
            Message::Compacted(revision) => {
                if !self.open
                    && !self.animating(now)
                    && self
                        .closing
                        .is_some_and(|request| request.revision == revision)
                {
                    self.closing = None;
                    self.expanded = false;
                }
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
        if let Event::FrameSubmitted {
            surface: "dock",
            height,
            now,
            ..
        } = event
        {
            if let Some(request) = self.closing
                && !self.animating(now)
            {
                return Some(Message::Compacted(request.revision));
            }
            return (height != self.submitted_height
                || (self.opening && height >= self.full_height)
                || (self.open && !self.edit_mounted && self.reveal.value(now) > 0.001))
                .then_some(Message::Submitted { height, now });
        }
        if !self.open {
            return None;
        }
        match event {
            Event::Outside { surface: "dock" } => Some(Message::Close),
            Event::Key {
                surface: "dock",
                key,
            } => match key {
                Key::Text(text) if self.mode == Mode::Apps && !self.edit_mounted => {
                    Some(Message::Query(format!("{}{text}", self.query)))
                }
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
                application_status: crate::catalog::Status::Ready,
                widgets_editing: false,
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
            // Existing model tests run against an already available full canvas.
            launcher.update(
                Message::Submitted {
                    height: self.geometry().viewport.1,
                    now: 0.,
                },
                self.input(),
                self.geometry(),
                &mut Effects::default(),
            );
            launcher.update(
                message,
                self.input(),
                self.geometry(),
                &mut Effects::default(),
            )
        }
    }
    fn update_at(fixture: &Fixture, launcher: &mut Launcher, message: Message, now: f64) {
        launcher.update(
            message,
            fixture.input(),
            fixture.geometry(),
            &mut Effects {
                now,
                ..Default::default()
            },
        );
    }
    fn submitted(launcher: &Launcher, height: f32, now: f64) -> Option<Message> {
        launcher.event(
            Event::FrameSubmitted {
                surface: "dock",
                width: 800.,
                height,
                now,
            },
            false,
        )
    }
    #[test]
    fn opening_waits_for_a_full_submission_and_keeps_early_keyboard_input() {
        let fixture = Fixture::new();
        let mut launcher = Launcher::default();
        update_at(
            &fixture,
            &mut launcher,
            Message::Submitted {
                height: 120.,
                now: 0.,
            },
            0.,
        );
        update_at(&fixture, &mut launcher, Message::Mode(Mode::Apps), 1.);
        assert!(launcher.expanded() && launcher.opening());
        assert_eq!(launcher.panel_height().value(5.), panel::DOCK_HEIGHT);
        assert_eq!(launcher.reveal().value(5.), 0.);
        let query = launcher
            .event(
                Event::Key {
                    surface: "dock",
                    key: Key::Text("beta".into()),
                },
                false,
            )
            .unwrap();
        update_at(&fixture, &mut launcher, query, 1.1);
        assert_eq!(launcher.query(), "beta");
        assert_eq!(launcher.result_count(), 1);
        assert!(submitted(&launcher, 120., 1.2).is_none());
        let ready = submitted(&launcher, 600., 2.).unwrap();
        update_at(&fixture, &mut launcher, ready, 2.);
        assert!(!launcher.opening());
        assert_eq!(launcher.reveal().value(2.), 0.);
        assert!(launcher.animating(2.1));
        let mounted = submitted(&launcher, 600., 2.1).unwrap();
        update_at(&fixture, &mut launcher, mounted, 2.1);
        assert!(
            launcher
                .event(
                    Event::Key {
                        surface: "dock",
                        key: Key::Text("a".into())
                    },
                    false
                )
                .is_none(),
            "the mounted framework editor owns subsequent text"
        );
        assert!(
            submitted(&launcher, 600., 5.).is_none(),
            "a settled submission must not request another frame"
        );
    }
    #[test]
    fn closing_shrinks_only_after_a_settled_frame_and_reopening_rejects_old_completion() {
        let fixture = Fixture::new();
        let mut launcher = Launcher::default();
        fixture.send(&mut launcher, Message::Mode(Mode::Apps));
        update_at(&fixture, &mut launcher, Message::Close, 1.);
        let old = launcher.closing.unwrap().revision;
        assert!(launcher.expanded());
        update_at(&fixture, &mut launcher, Message::Compacted(old), 1.01);
        assert!(
            launcher.expanded(),
            "a live closing morph still needs the full buffer"
        );
        assert!(submitted(&launcher, 600., 1.01).is_none());
        assert!(
            matches!(submitted(&launcher, 600., 2.), Some(Message::Compacted(id)) if id == old)
        );
        update_at(&fixture, &mut launcher, Message::Mode(Mode::Apps), 2.);
        update_at(&fixture, &mut launcher, Message::Compacted(old), 2.1);
        assert!(launcher.expanded() && launcher.open());
        update_at(&fixture, &mut launcher, Message::Close, 3.);
        let current = launcher.closing.unwrap().revision;
        assert_ne!(current, old);
        update_at(&fixture, &mut launcher, Message::Compacted(old), 4.);
        assert!(launcher.expanded());
        let complete = submitted(&launcher, 600., 4.).unwrap();
        update_at(&fixture, &mut launcher, complete, 4.);
        assert!(!launcher.expanded());
    }
    #[test]
    fn cancelled_opening_does_not_need_an_unpainted_timer_to_shrink() {
        let fixture = Fixture::new();
        let mut launcher = Launcher::default();
        update_at(&fixture, &mut launcher, Message::Mode(Mode::Apps), 1.);
        update_at(&fixture, &mut launcher, Message::Close, 1.1);
        assert!(launcher.expanded());
        let complete = submitted(&launcher, 120., 1.1).unwrap();
        update_at(&fixture, &mut launcher, complete, 1.1);
        assert!(!launcher.expanded() && !launcher.opening());
        launcher.close_revision = u64::MAX;
        fixture.send(&mut launcher, Message::Mode(Mode::Apps));
        update_at(&fixture, &mut launcher, Message::Close, 2.);
        update_at(&fixture, &mut launcher, Message::Compacted(0), 4.);
        assert!(
            launcher.expanded(),
            "exhaustion must never reuse a completion identity"
        );
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
        fixture.send(&mut launcher, Message::Select(WIDGETS.len()));
        assert!(matches!(
            fixture.send(&mut launcher, Message::Activate),
            Some(Intent::ResetLayout)
        ));
        fixture.send(&mut launcher, Message::Select(usize::MAX));
        assert!(matches!(
            fixture.send(&mut launcher, Message::Activate),
            Some(Intent::EditWidgets(true))
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
