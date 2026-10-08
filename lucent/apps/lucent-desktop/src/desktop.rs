use lucent_api::{self as api, *};
use lucent_domain::{self as domain, *};
use lucent_services::{
    self as services, JsonSettings, applications::XdgApplications, compositor::Hyprland,
};
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
#[derive(Clone, Debug)]
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
#[derive(Clone)]
pub enum Message {
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
pub const SPATIAL: [f32; 4] = [0.38, 1.21, 0.22, 1.];
pub const DECEL: [f32; 4] = [0.05, 0.7, 0.1, 1.];
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
    pub selection: Motion,
    pub carousel: Motion,
    pub viewport: (f32, f32),
    pub timer: FocusTimer,
    pub month_offset: i32,
    pub error: String,
    pub drag_origins: BTreeMap<String, Placement>,
    pub app_port: Arc<XdgApplications>,
    pub hypr: Option<Arc<Hyprland>>,
    pub store: Arc<JsonSettings>,
}
impl Desktop {
    pub fn new() -> Self {
        let mut images = BTreeMap::new();
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
            "sun",
            "cloud",
            "lock",
            "command",
        ] {
            images.insert(
                format!("symbol:{name}"),
                services::images::symbol(name, "#a8ecf6"),
            );
        }
        Self {
            registry: crate::widgets::registry(),
            apps: vec![],
            settings: DesktopSettings::default(),
            settings_writable: false,
            images,
            compositor: CompositorSnapshot::default(),
            clock: services::clock(),
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
            panel_width: Motion::fixed(350.),
            panel_height: Motion::fixed(54.),
            selection: Motion::fixed(0.),
            carousel: Motion::fixed(0.),
            viewport: (1920., 1080.),
            timer: FocusTimer::default(),
            month_offset: 0,
            error: String::new(),
            drag_origins: BTreeMap::new(),
            app_port: Arc::new(XdgApplications),
            hypr: Hyprland::from_env().ok().map(Arc::new),
            store: Arc::new(JsonSettings::default()),
        }
    }
    pub fn save(&self, effects: &mut Effects<Message>) {
        if self.settings_writable {
            let settings = self.settings.clone();
            let store = self.store.clone();
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
        apps.truncate(8);
        apps
    }
    pub fn dock_width(&self) -> f32 {
        (self.dock_apps().len() as f32 + 1.) * 49. + 28.
    }
    fn update_geometry(&mut self, now: f64) {
        let width = if self.launcher {
            if self.mode == Mode::Wallpapers {
                1248_f32.min(self.viewport.0 - 40.)
            } else {
                448_f32.min(self.viewport.0 - 40.)
            }
        } else {
            self.dock_width()
        };
        let height = if self.launcher {
            match self.mode {
                Mode::Wallpapers => 380.,
                Mode::Apps => (self.results.len().min(7) as f32 * 45. + 126.).clamp(218., 500.),
                Mode::Widgets => 506.,
                Mode::Commands => 440.,
                Mode::Themes => 272.,
            }
        } else {
            54.
        };
        self.panel_width.target(width, now, 0.5, SPATIAL);
        self.panel_height.target(height, now, 0.5, SPATIAL);
        self.reveal.target(
            if self.launcher { 1. } else { 0. },
            now,
            if self.launcher { 0.32 } else { 0.19 },
            DECEL,
        );
    }
    fn choose(&mut self, index: usize, now: f64) {
        self.selected = index.min(self.results.len().saturating_sub(1));
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        if self.selected >= self.scroll + 7 {
            self.scroll = self.selected - 6;
        }
        self.selection.target(
            (self.selected - self.scroll) as f32 * 45.,
            now,
            0.35,
            SPATIAL,
        );
    }
    fn launch(&self, id: &AppId, prefer_running: bool, effects: &mut Effects<Message>) {
        let Some(app) = self.apps.iter().find(|a| &a.id == id).cloned() else {
            return;
        };
        let apps = self.app_port.clone();
        let hypr = self.hypr.clone();
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
        effects.task(move || {
            Message::Completed(
                match action {
                    Action::Lock => services::command("omarchy-system-lock", &[]),
                    Action::VolumeUp => services::command(
                        "wpctl",
                        &["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "5%+"],
                    ),
                    Action::VolumeDown => {
                        services::command("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"])
                    }
                    Action::Mute => {
                        services::command("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
                    }
                    Action::PlayPause => services::command("playerctl", &["play-pause"]),
                    Action::Next => services::command("playerctl", &["next"]),
                    Action::Previous => services::command("playerctl", &["previous"]),
                    _ => Ok(String::new()),
                }
                .map(|_| ()),
            )
        });
    }
}
impl Component for Desktop {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        match cx.surface {
            "bar" => self.bar(cx),
            "widgets" => self.widgets(cx),
            "dock" => self.dock(cx),
            _ => Element::empty(),
        }
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        let now = effects.now;
        match message {
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
                let icons: Vec<_> = self.apps.iter().map(|a| a.icon.clone()).collect();
                let walls = self.wallpapers.clone();
                effects.task(move || {
                    let mut images = vec![];
                    let mut seen = std::collections::BTreeSet::new();
                    for icon in icons {
                        if seen.insert(icon.clone())
                            && let Some(image) = services::images::icon(&icon)
                        {
                            images.push((icon, image));
                        }
                    }
                    for wall in walls.iter().take(64) {
                        if let Some(image) =
                            services::images::load(std::path::Path::new(&wall.path), 512)
                        {
                            images.push((wall.path.clone(), image));
                        }
                    }
                    Message::Images(images)
                });
                self.update_geometry(now);
                for id in ["bar", "widgets", "dock"] {
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
                        self.update_geometry(now);
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
                self.update_geometry(now);
                effects.redraw("dock");
            }
            Message::CloseLauncher => {
                self.launcher = false;
                self.update_geometry(now);
                effects.redraw("dock");
            }
            Message::Mode(mode) => {
                self.launcher = true;
                self.mode = mode;
                self.query.clear();
                self.results = lucent_usecases::search_applications(&self.apps, "");
                self.scroll = 0;
                self.choose(0, now);
                self.update_geometry(now);
                effects.redraw("dock");
            }
            Message::Query(query) => {
                self.query = query;
                self.results = lucent_usecases::search_applications(&self.apps, &self.query);
                self.scroll = 0;
                self.choose(0, now);
                self.update_geometry(now);
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
                } else if self.mode == Mode::Apps
                    && let Some(index) = self.results.get(self.selected)
                {
                    let id = self.apps[*index].id.clone();
                    self.launch(&id, false, effects);
                    self.launcher = false;
                    self.update_geometry(now);
                    effects.redraw("dock");
                }
            }
            Message::Launch(id, prefer) => {
                self.launch(&id, prefer, effects);
                self.launcher = false;
                self.update_geometry(now);
                effects.redraw("dock");
            }
            Message::Workspace(id) => {
                if let Some(hypr) = self.hypr.clone() {
                    effects.task(move || Message::Completed(hypr.switch_workspace(id)));
                }
            }
            Message::Action(action) => self.perform(action, effects),
            Message::Wallpaper(index, apply) => {
                if index < self.wallpapers.len() {
                    self.wallpaper_index = index;
                    self.carousel.target(index as f32, now, 0.5, SPATIAL);
                    if apply {
                        let path = self.wallpapers[index].path.clone();
                        effects.task(move || {
                            let result = services::apply_wallpaper(&path);
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
                let _ = lucent_usecases::move_widget(
                    &mut self.settings,
                    &id,
                    Placement {
                        x: origin.x + drag.dx,
                        y: origin.y + drag.dy,
                    },
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
                for id in ["bar", "widgets", "dock"] {
                    effects.redraw(id);
                }
            }
            Message::ResetLayout => {
                self.settings.positions.clear();
                self.save(effects);
                effects.redraw("widgets");
            }
            Message::Resize(id, w, h) => {
                if id == "widgets" {
                    self.viewport = (w, h);
                }
                self.update_geometry(now);
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
                id: "bar",
                layer: Layer::Top,
                anchor: Anchor::Top,
                width: 0,
                height: 56,
                exclusive_zone: 56,
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
        let apps = self.app_port.clone();
        let store = self.store.clone();
        effects.task(move || {
            Message::Loaded(
                apps.discover().unwrap_or_default(),
                store.load(),
                services::wallpapers(),
                services::current_wallpaper(),
            )
        });
    }
    fn subscriptions(&self) -> Vec<Subscription<Message>> {
        let mut s = vec![
            Subscription::every("clock", Duration::from_secs(1), || {
                Message::Clock(services::clock())
            }),
            Subscription::stream("system", |out, cancel| {
                let mut probe = services::SystemProbe::default();
                while !cancel.cancelled() {
                    out.send(Message::System(probe.sample()));
                    cancel.sleep(Duration::from_secs(5));
                }
            }),
            Subscription::every("media", Duration::from_secs(2), || {
                Message::Media(services::media())
            }),
            Subscription::every("weather", Duration::from_secs(15 * 60), || {
                Message::Weather(services::weather())
            }),
        ];
        if let Some(hypr) = self.hypr.clone() {
            s.push(Subscription::stream("compositor", move |out, cancel| {
                hypr.watch(out, cancel, Message::Compositor)
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
                Key::Escape => Some(Message::CloseLauncher),
                Key::Enter => Some(Message::Activate),
                Key::Up | Key::Left => Some(Message::Navigate(-1)),
                Key::Down | Key::Right => Some(Message::Navigate(1)),
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
        serde_json::json!({"launcher":self.launcher,"mode":self.mode.name(),"query":self.query,"selected":self.selected,"result_count":self.results.len(),"applications":self.apps.len(),"workspaces":self.compositor.workspaces.iter().map(|w|serde_json::json!({"id":w.id,"active":w.active})).collect::<Vec<_>>(),"widgets":self.settings.visible_widgets,"positions":self.settings.positions,"notes":self.settings.notes,"timer_seconds":self.timer.remaining,"error":self.error}).to_string()
    }
    fn animating(&self, surface: &str, now: f64) -> bool {
        surface == "dock"
            && [
                self.reveal,
                self.panel_width,
                self.panel_height,
                self.selection,
                self.carousel,
            ]
            .iter()
            .any(|m| m.active(now))
    }
}
