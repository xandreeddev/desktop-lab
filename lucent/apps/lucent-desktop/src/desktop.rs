use crate::adapters::{DesktopAdapters, WatchStop};
use lucent_api::{self as api, *};
use lucent_design::{component::panel, motion};
use lucent_domain::{self as domain, *};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

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
        let index = Self::ALL.iter().position(|mode| *mode == self).unwrap() as i32;
        Self::ALL[(index + delta).rem_euclid(Self::ALL.len() as i32) as usize]
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
#[derive(Clone, Copy, Debug)]
pub enum Action {
    Lock,
    VolumeUp,
    VolumeDown,
    Mute,
    PlayPause,
    Next,
    Previous,
    Terminal,
    Quit,
}
pub const COMMANDS: [(&str, Action, &str); 6] = [
    ("New terminal", Action::Terminal, "command"),
    ("Lock screen", Action::Lock, "lock"),
    ("Volume up", Action::VolumeUp, "volume"),
    ("Volume down", Action::VolumeDown, "volume"),
    ("Mute / unmute", Action::Mute, "volume"),
    ("Close Lucent", Action::Quit, "power"),
];
#[derive(Clone)]
pub enum Message {
    Notifications(crate::notifications::Message),
    Loaded(
        Vec<domain::Application>,
        domain::Result<DesktopSettings>,
        Vec<Wallpaper>,
        String,
    ),
    Images(Vec<(String, Arc<ImageData>)>),
    Compositor(domain::Result<CompositorSnapshot>),
    Clock(ClockSnapshot),
    System(domain::Result<SystemSnapshot>),
    Media(domain::Result<MediaSnapshot>),
    Weather(domain::Result<WeatherSnapshot>),
    ToggleLauncher,
    CloseLauncher,
    Mode(Mode),
    Query(String),
    Select(usize),
    Navigate(i32),
    Activate,
    Launch(AppId, bool),
    Workspace(i32),
    Action(Action),
    Wallpaper(usize, bool),
    WallpaperApplied(String, domain::Result<()>),
    ToggleWidget(String),
    MoveWidget(String, DragEvent),
    Notes(String),
    TimerToggle,
    TimerReset,
    Month(i32),
    Theme(bool),
    ResetLayout,
    Resize(&'static str, f32, f32),
    Completed(domain::Result<()>),
    Quit,
}
pub const SPATIAL: [f32; 4] = motion::SPATIAL;
pub const DECEL: [f32; 4] = motion::DECELERATE;
pub const WIDGETS: [(&str, &str, &str); 7] = [
    ("calendar", "Calendar", "Your month at a glance"),
    ("clock", "Clock", "Large, stacked time"),
    ("weather", "Weather", "Current local conditions"),
    ("media", "Media", "Now playing and controls"),
    ("system", "System", "Live CPU and memory"),
    ("notes", "Notes", "A thought to keep nearby"),
    ("timer", "Focus", "A 25-minute focus timer"),
];
pub struct Desktop {
    pub notifications: crate::notifications::Center,
    pub registry: WidgetRegistry<Desktop, Message>,
    pub apps: Vec<domain::Application>,
    pub settings: DesktopSettings,
    pub settings_writable: bool,
    pub images: BTreeMap<String, Arc<ImageData>>,
    pub compositor: CompositorSnapshot,
    pub clock: ClockSnapshot,
    pub system: SystemSnapshot,
    pub media: MediaSnapshot,
    pub weather: Option<WeatherSnapshot>,
    pub wallpapers: Vec<Wallpaper>,
    pub wallpaper_index: usize,
    pub applied_wallpaper: String,
    pub launcher: bool,
    pub mode: Mode,
    pub query: String,
    pub results: Vec<usize>,
    pub selected: usize,
    pub scroll: usize,
    pub reveal: Motion,
    pub panel_width: Motion,
    pub panel_height: Motion,
    pub carousel: Motion,
    pub viewport: (f32, f32),
    pub timer: FocusTimer,
    pub month_offset: i32,
    pub error: String,
    pub drag_origins: BTreeMap<String, Placement>,
    pub adapters: DesktopAdapters,
}
impl Desktop {
    pub fn new(adapters: DesktopAdapters) -> Self {
        let images = adapters.assets.initial();
        Self {
            notifications: crate::notifications::Center::new(adapters.notifications.clone()),
            registry: crate::widgets::registry(),
            apps: vec![],
            settings: DesktopSettings::default(),
            settings_writable: false,
            images,
            compositor: CompositorSnapshot::default(),
            clock: adapters.clock.now(),
            system: SystemSnapshot::default(),
            media: MediaSnapshot::default(),
            weather: None,
            wallpapers: vec![],
            wallpaper_index: 0,
            applied_wallpaper: String::new(),
            launcher: false,
            mode: Mode::Apps,
            query: String::new(),
            results: vec![],
            selected: 0,
            scroll: 0,
            reveal: Motion::fixed(0.),
            panel_width: Motion::fixed(panel::INITIAL_WIDTH),
            panel_height: Motion::fixed(panel::DOCK_HEIGHT),
            carousel: Motion::fixed(0.),
            viewport: (1920., 1080.),
            timer: FocusTimer::default(),
            month_offset: 0,
            error: String::new(),
            drag_origins: BTreeMap::new(),
            adapters,
        }
    }
    pub fn save(&self, effects: &mut Effects<Message>) {
        if self.settings_writable {
            let settings = self.settings.clone();
            let store = self.adapters.settings.clone();
            effects.task(move || Message::Completed(store.save(&settings)));
        }
    }
    pub fn dock_apps(&self) -> Vec<&domain::Application> {
        let mut apps: Vec<_> = self
            .settings
            .pinned
            .iter()
            .filter_map(|id| self.apps.iter().find(|a| &a.id == id))
            .collect();
        if apps.is_empty() {
            for query in [
                "foot", "firefox", "chromium", "nautilus", "spotify", "aether",
            ] {
                if let Some(app) = self
                    .apps
                    .iter()
                    .find(|a| a.id.0.eq_ignore_ascii_case(&format!("{query}.desktop")))
                    .or_else(|| {
                        self.apps
                            .iter()
                            .find(|a| a.id.0.to_lowercase().contains(query))
                    })
                    && !apps.iter().any(|a| a.id == app.id)
                {
                    apps.push(app);
                }
            }
        }
        for window in &self.compositor.windows {
            if let Some(app) = self.apps.iter().find(|a| {
                a.startup_class.eq_ignore_ascii_case(&window.app_class)
                    || a.id
                        .0
                        .trim_end_matches(".desktop")
                        .eq_ignore_ascii_case(&window.app_class)
            }) && !apps.iter().any(|a| a.id == app.id)
            {
                apps.push(app);
            }
        }
        apps.truncate(8.min(crate::shell_layout::dock_capacity(self.viewport.0).saturating_sub(1)));
        apps
    }
    pub fn dock_width(&self) -> f32 {
        crate::shell_layout::dock_width(self.dock_apps().len() + 1)
    }
    fn retarget_panel_transition(&mut self, now: f64) {
        let (width, height) = if self.launcher {
            crate::shell_layout::launcher_size(self.mode, self.results.len(), self.viewport)
        } else {
            (self.dock_width(), panel::DOCK_HEIGHT)
        };
        self.panel_width.target(width, now, motion::PANEL, SPATIAL);
        self.panel_height
            .target(height, now, motion::PANEL, SPATIAL);
        self.reveal.target(
            if self.launcher { 1. } else { 0. },
            now,
            if self.launcher {
                motion::ENTER
            } else {
                motion::EXIT
            },
            DECEL,
        );
    }
    fn choose(&mut self, index: usize, _now: f64) {
        let count = match self.mode {
            Mode::Apps => self.results.len(),
            Mode::Commands => COMMANDS.len(),
            Mode::Widgets => WIDGETS.len() + 1,
            Mode::Themes => 2,
            Mode::Wallpapers => self.wallpapers.len(),
        };
        self.selected = index.min(count.saturating_sub(1));
        let rows = crate::shell_layout::visible_rows(self.mode, self.results.len(), self.viewport);
        let row_count = if self.mode == Mode::Widgets {
            WIDGETS.len()
        } else {
            count
        };
        let selected = self.selected.min(row_count.saturating_sub(1));
        self.scroll = self
            .scroll
            .min(selected)
            .max((selected + 1).saturating_sub(rows))
            .min(row_count.saturating_sub(rows));
    }
    fn launch(&self, id: &AppId, prefer_running: bool, effects: &mut Effects<Message>) {
        let Some(app) = self.apps.iter().find(|a| &a.id == id).cloned() else {
            return;
        };
        let apps = self.adapters.apps.clone();
        let hypr = self.adapters.compositor.clone();
        effects.task(move || {
            Message::Completed(if let Some(hypr) = hypr {
                lucent_usecases::activate_application(
                    apps.as_ref(),
                    hypr.as_ref(),
                    &app,
                    prefer_running,
                )
            } else {
                apps.launch(&app)
            })
        });
    }
    fn perform(&mut self, action: Action, effects: &mut Effects<Message>) {
        if matches!(action, Action::Quit) {
            effects.quit();
            return;
        }
        if matches!(action, Action::Terminal) {
            if let Some(app) = self.apps.iter().find(|a| a.id.0 == "foot.desktop") {
                self.launch(&app.id, false, effects);
            }
            return;
        }
        let adapters = self.adapters.clone();
        effects.task(move || {
            Message::Completed(match action {
                Action::Lock => adapters.session.lock(),
                Action::VolumeUp => adapters.audio.control(AudioCommand::Raise),
                Action::VolumeDown => adapters.audio.control(AudioCommand::Lower),
                Action::Mute => adapters.audio.control(AudioCommand::ToggleMute),
                Action::PlayPause => adapters.media.control(MediaCommand::PlayPause),
                Action::Next => adapters.media.control(MediaCommand::Next),
                Action::Previous => adapters.media.control(MediaCommand::Previous),
                _ => Ok(()),
            })
        });
    }
}
impl Component for Desktop {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        self.theme().apply(match cx.surface {
            "bar" => self.bar(cx),
            "widgets" => self.widgets(cx),
            "dock" => self.dock(cx),
            "notifications" => self
                .notifications
                .view_with_theme(cx, self.theme())
                .map(Message::Notifications),
            _ => Element::empty(),
        })
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        let now = effects.now;
        match message {
            Message::Notifications(message) => effects.delegate(
                |e| self.notifications.update(message, e),
                Message::Notifications,
            ),
            Message::Loaded(apps, settings, wallpapers, current_wallpaper) => {
                self.apps = apps;
                match settings {
                    Ok(settings) => {
                        self.settings = settings;
                        self.settings_writable = true;
                    }
                    Err(e) => self.error = e.to_string(),
                }
                self.wallpapers = wallpapers;
                self.wallpaper_index = self
                    .wallpapers
                    .iter()
                    .position(|w| w.path == current_wallpaper)
                    .unwrap_or(0);
                self.carousel = Motion::fixed(self.wallpaper_index as f32);
                self.applied_wallpaper = current_wallpaper;
                self.results = lucent_usecases::search_applications(&self.apps, "");
                let assets = self.adapters.assets.clone();
                let apps = self.apps.clone();
                let walls = self.wallpapers.clone();
                effects.task(move || Message::Images(assets.load(&apps, &walls)));
                self.retarget_panel_transition(now);
                for id in ["bar", "widgets", "dock", "notifications"] {
                    effects.redraw(id);
                }
            }
            Message::Images(images) => {
                self.images.extend(images);
                effects.redraw("dock");
                effects.redraw("widgets");
            }
            Message::Compositor(result) => match result {
                Ok(value) => {
                    if self.compositor != value {
                        self.compositor = value;
                        self.retarget_panel_transition(now);
                        effects.redraw("bar");
                        effects.redraw("dock");
                    }
                }
                Err(e) => self.error = e.to_string(),
            },
            Message::Clock(clock) => {
                if self.clock.minute != clock.minute || self.clock.date != clock.date {
                    effects.redraw("bar");
                    effects.redraw("widgets");
                }
                let before = self.timer.remaining;
                self.timer.tick(clock.unix_seconds);
                if before != self.timer.remaining
                    && self.settings.visible_widgets.iter().any(|w| w == "timer")
                {
                    effects.redraw("widgets");
                }
                self.clock = clock;
            }
            Message::System(Ok(value)) => {
                if self.system.volume != value.volume
                    || self.system.muted != value.muted
                    || self.system.network != value.network
                {
                    effects.redraw("bar");
                }
                if self.settings.visible_widgets.iter().any(|w| w == "system") {
                    effects.redraw("widgets");
                }
                self.system = value;
            }
            Message::System(Err(_)) => {}
            Message::Media(result) => {
                let value = result.unwrap_or_default();
                if self.media != value {
                    self.media = value;
                    effects.redraw("bar");
                    effects.redraw("widgets");
                }
            }
            Message::Weather(result) => {
                self.weather = result.ok();
                effects.redraw("bar");
                effects.redraw("widgets");
            }
            Message::ToggleLauncher => {
                self.launcher = !self.launcher;
                if self.launcher {
                    self.mode = Mode::Apps;
                    self.query.clear();
                    self.results = lucent_usecases::search_applications(&self.apps, "");
                    self.choose(0, now);
                }
                self.retarget_panel_transition(now);
                effects.redraw("dock");
            }
            Message::CloseLauncher => {
                self.launcher = false;
                self.retarget_panel_transition(now);
                effects.redraw("dock");
            }
            Message::Mode(mode) => {
                self.launcher = true;
                self.mode = mode;
                self.query.clear();
                self.results = lucent_usecases::search_applications(&self.apps, "");
                self.scroll = 0;
                self.choose(0, now);
                self.retarget_panel_transition(now);
                effects.redraw("dock");
            }
            Message::Query(query) => {
                self.query = query;
                self.results = lucent_usecases::search_applications(&self.apps, &self.query);
                self.scroll = 0;
                self.choose(0, now);
                self.retarget_panel_transition(now);
                effects.redraw("dock");
            }
            Message::Select(index) => {
                self.choose(index, now);
                effects.redraw("dock");
            }
            Message::Navigate(delta) => {
                if self.mode == Mode::Wallpapers {
                    let index = (self.wallpaper_index as i32 + delta)
                        .clamp(0, self.wallpapers.len().saturating_sub(1) as i32)
                        as usize;
                    self.update(Message::Wallpaper(index, false), effects);
                } else {
                    self.choose((self.selected as i32 + delta).max(0) as usize, now);
                    effects.redraw("dock");
                }
            }
            Message::Activate => {
                if self.mode == Mode::Wallpapers {
                    self.update(Message::Wallpaper(self.wallpaper_index, true), effects);
                } else if self.mode == Mode::Themes {
                    self.update(Message::Theme(self.selected == 1), effects);
                } else if self.mode == Mode::Widgets {
                    let message = WIDGETS
                        .get(self.selected)
                        .map(|(id, _, _)| Message::ToggleWidget((*id).into()))
                        .unwrap_or(Message::ResetLayout);
                    self.update(message, effects);
                } else if self.mode == Mode::Commands {
                    if let Some((_, action, _)) = COMMANDS.get(self.selected) {
                        self.perform(*action, effects);
                    }
                } else if self.mode == Mode::Apps
                    && let Some(index) = self.results.get(self.selected)
                {
                    let id = self.apps[*index].id.clone();
                    self.launch(&id, false, effects);
                    self.launcher = false;
                    self.retarget_panel_transition(now);
                    effects.redraw("dock");
                }
            }
            Message::Launch(id, prefer) => {
                self.launch(&id, prefer, effects);
                self.launcher = false;
                self.retarget_panel_transition(now);
                effects.redraw("dock");
            }
            Message::Workspace(id) => {
                if let Some(hypr) = self.adapters.compositor.clone() {
                    effects.task(move || Message::Completed(hypr.switch_workspace(id)));
                }
            }
            Message::Action(action) => self.perform(action, effects),
            Message::Wallpaper(index, apply) => {
                if index < self.wallpapers.len() {
                    self.wallpaper_index = index;
                    self.carousel
                        .target(index as f32, now, motion::PANEL, SPATIAL);
                    if apply {
                        let path = self.wallpapers[index].path.clone();
                        let wallpaper = self.adapters.wallpaper.clone();
                        effects.task(move || {
                            let result = wallpaper.apply(&path);
                            Message::WallpaperApplied(path, result)
                        });
                    }
                    effects.redraw("dock");
                }
            }
            Message::WallpaperApplied(path, result) => {
                match result {
                    Ok(()) => {
                        self.applied_wallpaper = path;
                        self.error.clear();
                    }
                    Err(e) => self.error = e.to_string(),
                }
                effects.redraw("dock");
            }
            Message::ToggleWidget(id) => {
                lucent_usecases::toggle_widget(&mut self.settings, &id);
                self.save(effects);
                effects.redraw("widgets");
                effects.redraw("dock");
            }
            Message::MoveWidget(id, drag) => {
                let origin = *self.drag_origins.entry(id.clone()).or_insert_with(|| {
                    self.settings
                        .positions
                        .get(&id)
                        .copied()
                        .unwrap_or_else(|| crate::widgets::default_position(&id, self.viewport))
                });
                let size = crate::widgets::widget_size(&id);
                let mut placement = Placement {
                    x: origin.x + drag.dx,
                    y: origin.y + drag.dy,
                };
                if drag.finished {
                    let Ok(snapped) = lucent_usecases::snap_placement(
                        placement,
                        lucent_design::component::widget_layout::GRID_STEP,
                    ) else {
                        return;
                    };
                    placement = snapped;
                }
                let _ = lucent_usecases::move_widget(
                    &mut self.settings,
                    &id,
                    placement,
                    size,
                    self.viewport,
                );
                if drag.finished {
                    self.drag_origins.remove(&id);
                    self.save(effects);
                }
                effects.redraw("widgets");
            }
            Message::Notes(text) => {
                self.settings.notes = text;
                self.save(effects);
                effects.redraw("widgets");
            }
            Message::TimerToggle => {
                self.timer.toggle(self.clock.unix_seconds);
                effects.redraw("widgets");
            }
            Message::TimerReset => {
                self.timer = FocusTimer::default();
                effects.redraw("widgets");
            }
            Message::Month(delta) => {
                self.month_offset = (self.month_offset + delta).clamp(-120, 120);
                effects.redraw("widgets");
            }
            Message::Theme(light) => {
                self.settings.light = light;
                self.save(effects);
                for id in ["bar", "widgets", "dock", "notifications"] {
                    effects.redraw(id);
                }
            }
            Message::ResetLayout => {
                self.drag_origins.clear();
                self.settings.positions.clear();
                self.save(effects);
                effects.redraw("widgets");
            }
            Message::Resize(id, w, h) => {
                if matches!(id, "widgets" | "dock") {
                    self.viewport = (w, h);
                    effects.redraw("dock");
                }
                self.retarget_panel_transition(now);
                self.choose(self.selected, now);
                effects.redraw(id);
            }
            Message::Completed(result) => {
                self.error = result.err().map(|e| e.to_string()).unwrap_or_default();
                effects.redraw("dock");
            }
            Message::Quit => effects.quit(),
        }
    }
}
impl api::Application for Desktop {
    fn name(&self) -> &'static str {
        "lucent"
    }
    fn fonts(&self) -> Vec<&'static [u8]> {
        vec![
            include_bytes!("../assets/LucentSans.ttf"),
            include_bytes!("../assets/LucentDisplay.ttf"),
        ]
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![
            SurfaceSpec {
                id: "notifications",
                layer: Layer::Overlay,
                anchor: Anchor::Fill,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: Keyboard::OnDemand,
                visible: self.notifications.visible(),
                capture_all: false,
            },
            SurfaceSpec {
                id: "bar",
                layer: Layer::Top,
                anchor: Anchor::Top,
                width: 0,
                height: panel::BAR_HEIGHT as u32,
                exclusive_zone: panel::BAR_HEIGHT as i32,
                keyboard: Keyboard::None,
                visible: true,
                capture_all: false,
            },
            SurfaceSpec {
                id: "widgets",
                layer: Layer::Bottom,
                anchor: Anchor::Fill,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: Keyboard::OnDemand,
                visible: true,
                capture_all: false,
            },
            SurfaceSpec {
                id: "dock",
                layer: if self.launcher {
                    Layer::Overlay
                } else {
                    Layer::Top
                },
                anchor: Anchor::Fill,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: if self.launcher {
                    Keyboard::Exclusive
                } else {
                    Keyboard::None
                },
                visible: true,
                capture_all: self.launcher,
            },
        ]
    }
    fn init(&mut self, effects: &mut Effects<Message>) {
        let apps = self.adapters.apps.clone();
        let store = self.adapters.settings.clone();
        let wallpaper = self.adapters.wallpaper.clone();
        effects.task(move || {
            Message::Loaded(
                apps.discover().unwrap_or_default(),
                store.load(),
                wallpaper.list(),
                wallpaper.current(),
            )
        });
    }
    fn subscriptions(&self) -> Vec<Subscription<Message>> {
        let clock = self.adapters.clock.clone();
        let system = self.adapters.system.clone();
        let media = self.adapters.media.clone();
        let weather = self.adapters.weather.clone();
        let notifications = self.adapters.notifications.clone();
        let mut s = vec![
            Subscription::stream("notifications", move |out, cancel| {
                notifications.watch(
                    &mut |value| {
                        out.send(Message::Notifications(
                            crate::notifications::Message::Snapshot(value),
                        ))
                    },
                    &WatchStop(cancel),
                )
            }),
            Subscription::every("clock", Duration::from_secs(1), move || {
                Message::Clock(clock.now())
            }),
            Subscription::every("system", Duration::from_secs(5), move || {
                Message::System(system.sample())
            }),
            Subscription::every("media", Duration::from_secs(2), move || {
                Message::Media(media.snapshot())
            }),
            Subscription::every("weather", Duration::from_secs(15 * 60), move || {
                Message::Weather(weather.snapshot())
            }),
        ];
        if let Some(compositor) = self.adapters.compositor.clone() {
            s.push(Subscription::stream("compositor", move |out, cancel| {
                compositor.watch(
                    &mut |event| out.send(Message::Compositor(event)),
                    &WatchStop(cancel),
                );
            }));
        }
        s
    }
    fn event(&self, event: Event) -> Option<Message> {
        match event {
            Event::Resize {
                surface,
                width,
                height,
            } => Some(Message::Resize(surface, width, height)),
            Event::Outside { surface: "dock" } if self.launcher => Some(Message::CloseLauncher),
            Event::Key {
                surface: "dock",
                key,
            } if self.launcher => match key {
                Key::Tab => Some(Message::Mode(self.mode.cycle(1))),
                Key::BackTab => Some(Message::Mode(self.mode.cycle(-1))),
                Key::Escape => Some(Message::CloseLauncher),
                Key::Enter => Some(Message::Activate),
                Key::Up => Some(Message::Navigate(-1)),
                Key::Left if matches!(self.mode, Mode::Wallpapers | Mode::Themes) => {
                    Some(Message::Navigate(-1))
                }
                Key::Down => Some(Message::Navigate(1)),
                Key::Right if matches!(self.mode, Mode::Wallpapers | Mode::Themes) => {
                    Some(Message::Navigate(1))
                }
                Key::Home if self.mode == Mode::Wallpapers => Some(Message::Wallpaper(0, false)),
                _ => None,
            },
            Event::Scroll {
                surface: "dock",
                lines,
            } if self.launcher => Some(Message::Navigate(if lines > 0. { 1 } else { -1 })),
            _ => None,
        }
    }
    fn command(&self, command: &str) -> std::result::Result<Option<Message>, String> {
        Ok(Some(match command {
            "notifications toggle" => Message::Notifications(crate::notifications::Message::Toggle),
            "launcher toggle" => Message::ToggleLauncher,
            "launcher close" => Message::CloseLauncher,
            "launcher open" => Message::Mode(Mode::Apps),
            "wallpapers open" => Message::Mode(Mode::Wallpapers),
            "widgets open" => Message::Mode(Mode::Widgets),
            "quit" => Message::Quit,
            _ => return Err(
                "Use launcher toggle|open|close, wallpapers open, widgets open, inspect, or quit"
                    .into(),
            ),
        }))
    }
    fn inspect(&self) -> String {
        serde_json::json!({"notifications_ready":self.notifications.ready,"notification_count":self.notifications.snapshot.active.len(),"launcher":self.launcher,"mode":self.mode.name(),"query":self.query,"selected":self.selected,"result_count":self.results.len(),"applications":self.apps.len(),"mapped_icons":self.apps.iter().filter(|a|lucent_design::app_icons::lookup(&a.id.0).is_some()).count(),"workspaces":self.compositor.workspaces.iter().map(|w|serde_json::json!({"id":w.id,"active":w.active})).collect::<Vec<_>>(),"widgets":self.settings.visible_widgets,"positions":self.settings.positions,"widget_grid":!self.drag_origins.is_empty(),"notes":self.settings.notes,"timer_seconds":self.timer.remaining,"error":self.error}).to_string()
    }
    fn animating(&self, surface: &str, now: f64) -> bool {
        surface == "dock"
            && [
                self.reveal,
                self.panel_width,
                self.panel_height,
                self.carousel,
            ]
            .iter()
            .any(|m| m.active(now))
    }
}
