use crate::actions::Action;
use crate::adapters::{DesktopAdapters, WatchStop};
use crate::launcher::Mode;
use crate::launcher::{self, Launcher};
use crate::widgets::{self, catalog::WIDGETS};
use lucent_api::{self as api, *};
use lucent_design::{component::panel, motion};
use lucent_domain::{self as domain, *};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[derive(Clone)]
pub enum Message {
    Notifications(crate::notifications::Message),
    Loaded(
        Vec<domain::Application>,
        domain::Result<DesktopSettings>,
        Vec<Wallpaper>,
        domain::Result<Option<WallpaperAsset>>,
    ),
    Icons(Vec<(String, Arc<ImageData>)>),
    Previews(crate::previews::Message),
    WallpaperSelection(crate::wallpaper_selection::Message),
    Compositor(domain::Result<CompositorSnapshot>),
    Clock(ClockSnapshot),
    System(domain::Result<SystemSnapshot>),
    Audio(crate::audio::Message),
    Media(domain::Result<MediaSnapshot>),
    Weather(domain::Result<WeatherSnapshot>),
    Launcher(launcher::Message),
    Launch(AppId, bool),
    Workspace(i32),
    Action(Action),
    Wallpaper(usize, bool),
    Widgets(widgets::Message),
    Persistence(crate::persistence::Message),
    Theme(bool),
    Palette(ThemePalette),
    ExtractPalette,
    ExtractedPalette(crate::palette_intent::Extraction, domain::Result<Rgb>),
    Browser(crate::wallpaper_browser::Message),
    BrowserSearch(WallpaperProvider, String),
    ThemeApplied(domain::Result<()>),
    Osd(crate::osd::Feedback),
    OsdTick,
    OsdClose,
    Bar(bool),
    SetWallpaper(String),
    LauncherSize(LauncherSize),
    Resize(&'static str, f32, f32, u32),
    Completed(domain::Result<()>),
    Quit,
}
pub const SPATIAL: [f32; 4] = motion::SPATIAL;
pub struct Desktop {
    pub notifications: crate::notifications::Center,
    pub osd: crate::osd::Osd,
    pub bar_visible: bool,
    pub theme_error: String,
    pub widgets: widgets::Host,
    pub apps: Vec<domain::Application>,
    pub settings: DesktopSettings,
    pub persistence: crate::persistence::Persistence,
    pub settings_writable: bool,
    pub settings_error: String,
    pub images: BTreeMap<String, Arc<ImageData>>,
    pub previews: crate::previews::Previews,
    pub wallpaper: crate::wallpaper_selection::Selection,
    pub compositor: CompositorSnapshot,
    pub clock: ClockSnapshot,
    pub system: SystemSnapshot,
    pub audio: crate::audio::Audio,
    pub media: MediaSnapshot,
    pub weather: Option<WeatherSnapshot>,
    pub browser: crate::wallpaper_browser::Browser,
    browser_selection: Option<(u64, u64, Option<crate::palette_intent::Revision>)>,
    palette_intent: crate::palette_intent::Intent,
    pub wallpapers: Vec<Wallpaper>,
    pub wallpaper_index: usize,
    pub launcher: Launcher,
    pub carousel: Motion,
    pub viewport: (f32, f32),
    pub error: String,
    pub adapters: DesktopAdapters,
}
impl Desktop {
    pub fn new(adapters: DesktopAdapters) -> Self {
        let images = adapters.assets.initial();
        Self {
            osd: Default::default(),
            bar_visible: true,
            theme_error: String::new(),
            notifications: crate::notifications::Center::new(adapters.notifications.clone()),
            widgets: widgets::Host::default(),
            apps: vec![],
            settings: DesktopSettings::default(),
            persistence: crate::persistence::Persistence::new(adapters.settings.clone()),
            settings_writable: false,
            settings_error: String::new(),
            images,
            previews: crate::previews::Previews::new(adapters.assets.clone()),
            wallpaper: crate::wallpaper_selection::Selection::new(
                adapters.wallpaper.clone(),
                adapters.assets.clone(),
                adapters.palette_generation.clone(),
            ),
            compositor: CompositorSnapshot::default(),
            clock: adapters.clock.now(),
            system: SystemSnapshot::default(),
            audio: crate::audio::Audio::new(adapters.audio.clone()),
            media: MediaSnapshot::default(),
            weather: None,
            browser: Default::default(),
            browser_selection: None,
            palette_intent: Default::default(),
            wallpapers: vec![],
            wallpaper_index: 0,
            launcher: Launcher::default(),
            carousel: Motion::fixed(0.),
            viewport: (1920., 1080.),
            error: String::new(),
            adapters,
        }
    }
    fn choose_wallpaper(
        &mut self,
        choice: crate::wallpaper_selection::Choice,
        effects: &mut Effects<Message>,
    ) {
        effects.delegate(
            |child| self.wallpaper.choose(choice, child),
            Message::WallpaperSelection,
        );
    }
    fn refresh_previews(&mut self, effects: &mut Effects<Message>) {
        let mut requests = Vec::new();
        if self.launcher.mode() == Mode::Wallpapers
            && self.browser.source.is_none()
            && (self.launcher.open() || self.launcher.animating(effects.now))
        {
            // Target plus currently visible cards, including interrupted motion.
            // Only the client knows carousel geometry; views never start work.
            for center in [
                self.carousel.target_value(),
                self.carousel.value(effects.now),
            ] {
                for index in launcher::visible_wallpapers(self.wallpapers.len(), center) {
                    let wall = &self.wallpapers[index];
                    requests.push(crate::previews::Request {
                        key: wall.path.clone(),
                        path: if wall.path == self.wallpaper.source_path() {
                            self.wallpaper.asset_path().to_owned()
                        } else {
                            wall.path.clone()
                        },
                    });
                }
            }
        }
        effects.delegate(
            |child| self.previews.demand(requests, child),
            Message::Previews,
        );
    }
    fn apply_palette(&mut self, palette: ThemePalette, effects: &mut Effects<Message>) {
        if let Err(error) = palette.validate() {
            self.theme_error = error.to_string();
            effects.redraw("dock");
            return;
        }
        if palette.id.starts_with("wallpaper-")
            && !self
                .settings
                .saved_palettes
                .iter()
                .any(|p| p.id == palette.id)
        {
            if self.settings.saved_palettes.len() == 24 {
                self.settings.saved_palettes.remove(0);
            }
            self.settings.saved_palettes.push(palette.clone());
        }
        self.settings.light = palette.light();
        self.settings.palette = Some(palette.clone());
        self.save(effects);
        let theme = self.adapters.theme.clone();
        effects.task_on("theme", move || {
            Message::ThemeApplied(theme.apply(&palette))
        });
        for id in [
            "bar",
            "widgets",
            "dock",
            "notifications",
            "background",
            "osd",
        ] {
            effects.redraw(id);
        }
    }
    /// Feature-owned failures cannot be cleared by an unrelated successful task.
    /// Invalid settings take priority because saving remains disabled until fixed.
    pub fn error_message(&self) -> &str {
        if !self.settings_error.is_empty() {
            &self.settings_error
        } else if !self.persistence.error().is_empty() {
            self.persistence.error()
        } else if !self.wallpaper.error.is_empty() {
            &self.wallpaper.error
        } else if let Some(error) = self.wallpaper.presentation_error() {
            error
        } else {
            &self.error
        }
    }
    pub fn palette_choices(&self) -> Vec<ThemePalette> {
        let mut choices = lucent_design::palettes().to_vec();
        for palette in self
            .settings
            .saved_palettes
            .iter()
            .chain(self.settings.palette.iter())
        {
            if !choices.iter().any(|p| p.id == palette.id) {
                choices.push(palette.clone());
            }
        }
        choices
    }
    pub fn save(&mut self, effects: &mut Effects<Message>) {
        if self.settings_writable {
            effects.delegate(
                |child| self.persistence.request(&self.settings, child),
                Message::Persistence,
            );
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
                    .find(|a| {
                        a.id.as_str()
                            .eq_ignore_ascii_case(&format!("{query}.desktop"))
                    })
                    .or_else(|| {
                        self.apps
                            .iter()
                            .find(|a| a.id.as_str().to_lowercase().contains(query))
                    })
                    && !apps.iter().any(|a| a.id == app.id)
                {
                    apps.push(app);
                }
            }
        }
        for window in &self.compositor.windows {
            if let Some(app) = self
                .apps
                .iter()
                .find(|a| a.matches_window_class(&window.app_class))
                && !apps.iter().any(|a| a.id == app.id)
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
    fn launcher_geometry(&self) -> launcher::Geometry {
        launcher::Geometry {
            viewport: self.viewport,
            preferred: self.settings.launcher,
            dock_width: self.dock_width(),
            palettes: if self.launcher.mode() == Mode::Themes {
                self.palette_choices().len()
            } else {
                0
            },
            wallpapers: self.wallpapers.len(),
        }
    }
    fn retarget_panel_transition(&mut self, now: f64) {
        self.launcher.reconcile(self.launcher_geometry(), now);
    }
    fn update_launcher(&mut self, message: launcher::Message, effects: &mut Effects<Message>) {
        let palettes = if self.launcher.mode() == Mode::Themes
            || matches!(message, launcher::Message::Mode(Mode::Themes))
        {
            self.palette_choices()
        } else {
            Vec::new()
        };
        let mut geometry = self.launcher_geometry();
        geometry.palettes = palettes.len();
        let input = launcher::Input {
            apps: &self.apps,
            settings: &self.settings,
            palettes: &palettes,
            wallpapers: &self.wallpapers,
            wallpaper_index: self.wallpaper_index,
            browser: &self.browser,
        };
        let mut intent = None;
        effects.delegate(
            |child| intent = self.launcher.update(message, input, geometry, child),
            Message::Launcher,
        );
        if let Some(intent) = intent {
            use launcher::Intent;
            let message = match intent {
                Intent::Launch(id, prefer) => Message::Launch(id, prefer),
                Intent::Action(action) => Message::Action(action),
                Intent::Wallpaper(index, apply) => Message::Wallpaper(index, apply),
                Intent::Browser(message) => Message::Browser(message),
                Intent::ToggleWidget(id) => Message::Widgets(widgets::Message::Toggle(id)),
                Intent::ResetLayout => Message::Widgets(widgets::Message::ResetLayout),
                Intent::Palette(palette) => Message::Palette(palette),
                Intent::ExtractPalette => Message::ExtractPalette,
                Intent::LauncherSize(size) => Message::LauncherSize(size),
            };
            self.update(message, effects);
        }
    }
    fn update_widgets(&mut self, message: widgets::Message, effects: &mut Effects<Message>) {
        let mut intent = None;
        effects.delegate(
            |child| {
                intent = self.widgets.update(
                    message,
                    widgets::Update {
                        settings: &mut self.settings,
                        viewport: self.viewport,
                        unix_seconds: self.clock.unix_seconds,
                    },
                    child,
                )
            },
            Message::Widgets,
        );
        match intent {
            Some(widgets::Intent::Save) => self.save(effects),
            Some(widgets::Intent::RetrySave) => self.update(
                Message::Persistence(crate::persistence::Message::Retry),
                effects,
            ),
            Some(widgets::Intent::Action(action)) => self.perform(action, effects),
            Some(widgets::Intent::Error(error)) => {
                self.error = error.to_string();
                effects.redraw("dock");
            }
            None => {}
        }
    }
    fn launch(&self, id: &AppId, prefer_running: bool, effects: &mut Effects<Message>) {
        let Some(app) = self.apps.iter().find(|a| &a.id == id).cloned() else {
            return;
        };
        let apps = self.adapters.apps.clone();
        let hypr = self.adapters.compositor.clone();
        effects.task_on("launch", move || {
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
            if let Some(app) = self.apps.iter().find(|a| a.id.as_str() == "foot.desktop") {
                self.launch(&app.id, false, effects);
            }
            return;
        }
        let command = match action {
            Action::VolumeUp => Some(AudioCommand::Raise),
            Action::VolumeDown => Some(AudioCommand::Lower),
            Action::Mute => Some(AudioCommand::ToggleMute),
            _ => None,
        };
        if let Some(command) = command {
            effects.delegate(
                |child| {
                    self.audio
                        .update(crate::audio::Message::Request(command), child)
                },
                Message::Audio,
            );
            return;
        }
        let adapters = self.adapters.clone();
        effects.task_on(
            if matches!(action, Action::Lock) {
                "session"
            } else {
                "controls"
            },
            move || {
                Message::Completed(match action {
                    Action::Lock => adapters.session.lock(),
                    Action::PlayPause => adapters.media.control(MediaCommand::PlayPause),
                    Action::Next => adapters.media.control(MediaCommand::Next),
                    Action::Previous => adapters.media.control(MediaCommand::Previous),
                    _ => Ok(()),
                })
            },
        );
    }
}
impl Component for Desktop {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        self.theme().apply(match cx.surface {
            "audio" => self.audio.view(cx, self.theme()).map(Message::Audio),
            "osd" => self.osd.view(cx, self.theme()),
            "background" => self
                .wallpaper
                .background
                .as_ref()
                .map(|image| Element::image(image.clone()).cover().fill())
                .unwrap_or_else(|| Element::empty().fill().background(self.surface_color())),
            "bar" => self.bar(cx),
            "widgets" => widgets::View {
                host: &self.widgets,
                input: widgets::Input {
                    settings: &self.settings,
                    clock: &self.clock,
                    system: &self.system,
                    media: &self.media,
                    weather: self.weather.as_ref(),
                    save_status: self.persistence.status(self.settings_writable),
                },
                artwork: self.artwork(),
            }
            .render(cx)
            .map(Message::Widgets),
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
                        self.settings_error.clear();
                    }
                    Err(e) => {
                        self.settings_writable = false;
                        self.settings_error = e.to_string();
                    }
                }
                self.wallpapers = wallpapers;
                self.wallpaper_index = self
                    .wallpapers
                    .iter()
                    .position(|w| {
                        current_wallpaper
                            .as_ref()
                            .ok()
                            .and_then(|asset| asset.as_ref())
                            .is_some_and(|asset| w.path == asset.source.path)
                    })
                    .unwrap_or(0);
                self.carousel = Motion::fixed(self.wallpaper_index as f32);
                effects.delegate(
                    |child| self.wallpaper.load(current_wallpaper, child),
                    Message::WallpaperSelection,
                );
                self.launcher
                    .refresh(&self.apps, self.launcher_geometry(), now);
                let assets = self.adapters.assets.clone();
                let apps = self.apps.clone();
                effects.task_on("assets", move || {
                    Message::Icons(assets.application_icons(&apps))
                });
                let theme = self.adapters.theme.clone();
                let palette = lucent_design::selected_palette(&self.settings);
                effects.task_on("theme", move || {
                    Message::ThemeApplied(theme.apply(&palette))
                });
                self.retarget_panel_transition(now);
                for id in [
                    "bar",
                    "widgets",
                    "dock",
                    "notifications",
                    "background",
                    "osd",
                ] {
                    effects.redraw(id);
                }
            }
            Message::Icons(images) => {
                self.images.extend(images);
                effects.redraw("dock");
                effects.redraw("widgets");
            }
            Message::Previews(message) => effects.delegate(
                |child| self.previews.update(message, child),
                Message::Previews,
            ),
            Message::WallpaperSelection(message) => {
                use crate::wallpaper_selection::Outcome;
                let mut outcome = None;
                effects.delegate(
                    |child| outcome = self.wallpaper.update(message, child),
                    Message::WallpaperSelection,
                );
                match outcome {
                    Some(Outcome::Applied(prepared)) => {
                        let source = prepared.asset.source;
                        if !self.wallpapers.iter().any(|wall| wall.path == source.path) {
                            self.wallpapers.push(source.clone());
                        }
                        self.wallpaper_index = self
                            .wallpapers
                            .iter()
                            .position(|wall| wall.path == source.path)
                            .unwrap_or(0);
                        self.carousel = Motion::fixed(self.wallpaper_index as f32);
                        if let Err(error) = self.previews.insert(source.path, prepared.preview) {
                            self.error = error.to_string();
                        }
                        if let Some((revision, seed)) = prepared.seed
                            && self.palette_intent.accepts_wallpaper(revision)
                        {
                            self.apply_palette(lucent_design::wallpaper_palette(seed), effects);
                        }
                        if prepared.browser_generation == Some(self.browser.generation) {
                            self.browser.status = "Applied".into();
                        }
                    }
                    Some(Outcome::Failed {
                        browser_generation,
                        error,
                    }) if browser_generation == Some(self.browser.generation) => {
                        self.browser.status = error;
                    }
                    _ => {}
                }
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
                self.update_widgets(widgets::Message::Tick(clock.unix_seconds), effects);
                self.clock = clock;
            }
            Message::System(Ok(value)) => {
                if self.system.network != value.network {
                    effects.redraw("bar");
                }
                if self.settings.visible_widgets.iter().any(|w| w == "system") {
                    effects.redraw("widgets");
                }
                self.system = value;
            }
            Message::System(Err(_)) => {}
            Message::Audio(message) => {
                let opening = matches!(message, crate::audio::Message::Open)
                    || matches!(message, crate::audio::Message::Toggle) && !self.audio.open();
                if opening {
                    self.update_launcher(launcher::Message::Close, effects);
                }
                effects.delegate(|child| self.audio.update(message, child), Message::Audio);
            }
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
            Message::Launcher(message) => {
                self.update_launcher(message, effects);
                if self.launcher.open() {
                    effects.delegate(
                        |child| self.audio.update(crate::audio::Message::Close, child),
                        Message::Audio,
                    );
                }
            }
            Message::Launch(id, prefer) => {
                self.launch(&id, prefer, effects);
                self.update_launcher(launcher::Message::Close, effects);
            }
            Message::Workspace(id) => {
                if let Some(hypr) = self.adapters.compositor.clone() {
                    effects.task_on("controls", move || {
                        Message::Completed(hypr.switch_workspace(id))
                    });
                }
            }
            Message::Action(action) => self.perform(action, effects),
            Message::Wallpaper(index, apply) => {
                if index < self.wallpapers.len() {
                    self.wallpaper_index = index;
                    self.carousel
                        .target(index as f32, now, motion::PANEL, SPATIAL);
                    if apply {
                        self.choose_wallpaper(
                            crate::wallpaper_selection::Choice {
                                source: self.wallpapers[index].clone(),
                                colors: None,
                                browser_generation: None,
                            },
                            effects,
                        );
                    }
                    effects.redraw("dock");
                }
            }
            Message::Widgets(message) => self.update_widgets(message, effects),
            Message::Persistence(message) => {
                if self.settings_writable {
                    effects.delegate(
                        |child| self.persistence.update(message, child),
                        Message::Persistence,
                    );
                }
            }
            Message::ThemeApplied(result) => {
                self.theme_error = result
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_default();
                effects.redraw("dock");
            }
            Message::Osd(value) => {
                self.osd.show(value, now);
                effects.redraw("osd");
            }
            Message::OsdTick => {
                self.osd.tick(now);
                effects.redraw("osd");
            }
            Message::OsdClose => {
                self.osd.feedback = None;
                effects.redraw("osd");
            }
            Message::Bar(visible) => {
                self.bar_visible = visible;
                effects.redraw("bar");
            }
            Message::SetWallpaper(path) => {
                let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                self.choose_wallpaper(
                    crate::wallpaper_selection::Choice {
                        source: Wallpaper { path, name },
                        colors: None,
                        browser_generation: None,
                    },
                    effects,
                );
            }
            Message::Theme(light) => self.update(
                Message::Palette(lucent_design::palettes()[usize::from(light)].clone()),
                effects,
            ),
            Message::Palette(palette) => {
                self.palette_intent.explicit();
                self.apply_palette(palette, effects);
            }
            Message::ExtractPalette => {
                let extraction = self
                    .palette_intent
                    .extraction(self.wallpaper.asset_path().to_owned());
                let adapter = self.adapters.palette_generation.clone();
                effects.task_on("assets", move || {
                    let result = adapter.seed(&extraction.path);
                    Message::ExtractedPalette(extraction, result)
                });
            }
            Message::ExtractedPalette(extraction, result) => {
                if self
                    .palette_intent
                    .accepts_extraction(&extraction, self.wallpaper.asset_path())
                {
                    match result {
                        Ok(seed) => {
                            self.apply_palette(lucent_design::wallpaper_palette(seed), effects)
                        }
                        Err(error) => {
                            self.theme_error = error.to_string();
                            effects.redraw("dock");
                        }
                    }
                }
            }
            Message::BrowserSearch(provider, query) => {
                use crate::wallpaper_browser::Message as Browse;
                self.update(
                    Message::Launcher(launcher::Message::Mode(Mode::Wallpapers)),
                    effects,
                );
                for message in [
                    Browse::Source(Some(provider)),
                    Browse::Query(query),
                    Browse::Search(1),
                ] {
                    self.update(Message::Browser(message), effects);
                }
            }
            Message::Browser(message) => {
                use crate::wallpaper_browser::Message as Browse;
                let before = self.browser.generation;
                let download = matches!(message, Browse::Download(_));
                let colors = matches!(message, Browse::Download(true));
                let changed_source = matches!(message, Browse::Source(_));
                let mut superseded = false;
                if let Browse::Ready(id, result) = &message
                    && *id == self.browser.generation
                    && let Some((browser_id, ticket, revision)) = self.browser_selection
                    && browser_id == *id
                {
                    self.browser_selection = None;
                    superseded = !self.wallpaper.is_current_request(ticket);
                    match result {
                        Ok((source, _)) => effects.delegate(
                            |child| {
                                self.wallpaper.submit(
                                    ticket,
                                    crate::wallpaper_selection::Choice {
                                        source: source.clone(),
                                        colors: revision,
                                        browser_generation: Some(*id),
                                    },
                                    child,
                                )
                            },
                            Message::WallpaperSelection,
                        ),
                        Err(_) => self.wallpaper.cancel_reserved(ticket),
                    }
                }
                self.browser.update(message);
                if download && self.browser.generation != before {
                    self.browser_selection = Some((
                        self.browser.generation,
                        self.wallpaper.reserve(),
                        colors.then(|| self.palette_intent.wallpaper_colors()),
                    ));
                } else if changed_source && let Some((_, ticket, _)) = self.browser_selection.take()
                {
                    self.wallpaper.cancel_reserved(ticket);
                }
                if superseded {
                    self.browser.status = "Downloaded · selection superseded".into();
                }
                effects.redraw("dock");
            }
            Message::LauncherSize(size) => {
                self.settings.launcher = size;
                self.retarget_panel_transition(now);
                self.save(effects);
                effects.redraw("dock");
            }
            Message::Resize(id, w, h, scale) => {
                if id == "background" {
                    match crate::ports::BackgroundTarget::from_surface(w, h, scale) {
                        Ok(target) => effects.delegate(
                            |child| {
                                self.viewport = (w, h);
                                self.wallpaper.resize(target, child);
                            },
                            Message::WallpaperSelection,
                        ),
                        Err(error) => self.error = error.to_string(),
                    }
                }
                if id == "background" {
                    effects.redraw("dock");
                }
                self.retarget_panel_transition(now);
                effects.redraw(id);
            }
            Message::Completed(result) => {
                self.error = result.err().map(|e| e.to_string()).unwrap_or_default();
                effects.redraw("dock");
            }
            Message::Quit => effects.quit(),
        }
        self.refresh_previews(effects);
    }
}
impl api::Application for Desktop {
    fn runtime_failure(&self, failure: api::RuntimeFailure) -> Option<Message> {
        if failure.source == "effect queue audio" {
            return Some(Message::Audio(crate::audio::Message::Control(
                crate::audio::commands::Message::WorkerFailed(failure.to_string()),
            )));
        }
        if failure.source == "effect queue previews" {
            return Some(Message::Previews(crate::previews::Message::WorkerFailed(
                failure.to_string(),
            )));
        }
        if failure.source == "effect queue settings" {
            return Some(Message::Persistence(
                crate::persistence::Message::WorkerFailed(failure.to_string()),
            ));
        }
        // A panic/admission failure may leave commit completion ambiguous. Let
        // supervision restart the client and load the actual persisted selection
        // instead of leaving the coordinator busy or guessing whether it committed.
        if failure.source == "effect queue appearance" {
            return None;
        }
        Some(Message::Completed(Err(DomainError::Failed(
            failure.to_string(),
        ))))
    }
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
                id: "audio",
                layer: Layer::Overlay,
                anchor: Anchor::Fill,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: if self.audio.open() {
                    Keyboard::Exclusive
                } else {
                    Keyboard::None
                },
                visible: self.audio.visible(),
                capture_all: self.audio.open(),
            },
            SurfaceSpec {
                id: "osd",
                layer: Layer::Overlay,
                anchor: Anchor::Fill,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: Keyboard::None,
                visible: self.osd.feedback.is_some(),
                capture_all: false,
            },
            SurfaceSpec {
                id: "background",
                layer: Layer::Background,
                anchor: Anchor::Fill,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: Keyboard::None,
                visible: true,
                capture_all: false,
            },
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
                visible: self.bar_visible,
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
                layer: if self.launcher.open() {
                    Layer::Overlay
                } else {
                    Layer::Top
                },
                anchor: Anchor::Bottom,
                width: 0,
                height: if self.launcher.expanded() {
                    self.viewport.1.max(1.) as u32
                } else {
                    crate::shell_layout::dock_surface_height(self.viewport.1)
                },
                exclusive_zone: -1,
                keyboard: if self.launcher.open() {
                    Keyboard::Exclusive
                } else {
                    Keyboard::None
                },
                visible: true,
                capture_all: self.launcher.open(),
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
        let audio = self.adapters.audio.clone();
        let mut s = vec![
            Subscription::stream("audio", move |out, cancel| {
                audio.watch(
                    &mut |value| out.send(Message::Audio(crate::audio::Message::Observed(value))),
                    &WatchStop(cancel),
                );
            }),
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
        for subscription in self.audio.subscriptions() {
            s.push(Subscription::stream(subscription.id, move |out, cancel| {
                (subscription.run)(
                    Emitter::new(move |message| out.send(Message::Audio(message))),
                    cancel,
                );
            }));
        }
        for subscription in self.browser.subscriptions(&self.adapters) {
            s.push(Subscription::stream(subscription.id, move |out, cancel| {
                (subscription.run)(
                    Emitter::new(move |message| out.send(Message::Browser(message))),
                    cancel,
                )
            }));
        }
        if self.osd.feedback.is_some() {
            s.push(Subscription::every(
                "osd-expiry",
                Duration::from_millis(100),
                || Message::OsdTick,
            ));
        }
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
            Event::Key {
                surface: "audio",
                key: Key::Escape,
            } => Some(Message::Audio(crate::audio::Message::Close)),
            Event::Key {
                surface: "audio", ..
            } => None,
            Event::Resize {
                surface,
                width,
                height,
                scale,
            } => Some(Message::Resize(surface, width, height, scale)),
            event => self
                .launcher
                .event(event, self.browser.source.is_some())
                .map(Message::Launcher),
        }
    }
    fn command(&self, command: &str) -> std::result::Result<Option<Message>, String> {
        if command == "wallpaper retry" {
            return Ok(Some(Message::WallpaperSelection(
                crate::wallpaper_selection::Message::RetryPresentation,
            )));
        }
        if command == "wallpapers retry" {
            return Ok(Some(Message::Previews(crate::previews::Message::Retry)));
        }
        if command == "settings retry" {
            if !self.settings_writable {
                return Err(
                    "Settings are unavailable; fix the load error before retrying saves".into(),
                );
            }
            return Ok(Some(Message::Persistence(
                crate::persistence::Message::Retry,
            )));
        }
        if let Some(payload) = command.strip_prefix("osd show ") {
            return serde_json::from_str(payload)
                .map(|value| Some(Message::Osd(value)))
                .map_err(|_| "Invalid feedback payload".into());
        }
        if let Some(path) = command.strip_prefix("wallpaper set ") {
            return Ok(Some(Message::SetWallpaper(path.to_string())));
        }
        if let Some(id) = command.strip_prefix("notifications dismiss ") {
            return id
                .parse()
                .map(|id| {
                    Some(Message::Notifications(
                        crate::notifications::Message::Dismiss(id),
                    ))
                })
                .map_err(|_| "Invalid notification ID".into());
        }
        if let Some(id) = command
            .strip_prefix("widget ")
            .and_then(|s| s.strip_suffix(" show"))
        {
            if !WIDGETS.iter().any(|widget| widget.id == id) {
                return Err("Unknown widget".into());
            }
            return Ok(Some(Message::Widgets(widgets::Message::Show(id.into()))));
        }
        if matches!(command, "theme dark" | "theme light") && !self.settings_writable {
            return Err("Wait for saved settings to load before changing the theme".into());
        }
        if let Some(id) = command.strip_prefix("palette ") {
            if !self.settings_writable {
                return Err("Wait for saved settings to load".into());
            }
            if id == "wallpaper" {
                return Ok(Some(Message::ExtractPalette));
            }
            return self
                .palette_choices()
                .into_iter()
                .find(|p| p.id == id)
                .map(|p| Some(Message::Palette(p)))
                .ok_or_else(|| "Unknown palette".into());
        }
        if let Some(payload) = command.strip_prefix("wallpapers search ") {
            let (provider, query) = payload.split_once(' ').unwrap_or((payload, ""));
            let provider = match provider {
                "wallhaven" => WallpaperProvider::Wallhaven,
                "alpha-coders" => WallpaperProvider::AlphaCoders,
                _ => return Err("Unknown provider".into()),
            };
            return Ok(Some(Message::BrowserSearch(provider, query.to_string())));
        }
        let words: Vec<_> = command.split_whitespace().collect();
        if let ["launcher", "size", args @ ..] = words.as_slice() {
            use lucent_design::component::launcher;
            let usage = format!(
                "Use launcher size WIDTH MAX_HEIGHT (logical pixels, minimum {} × {}) or launcher size reset",
                launcher::MIN_WIDTH,
                panel::LAUNCHER_MIN_HEIGHT
            );
            let size = match args {
                ["reset"] => LauncherSize::default(),
                [width, height] => {
                    let width: u32 = width.parse().map_err(|_| usage.clone())?;
                    let height: u32 = height.parse().map_err(|_| usage.clone())?;
                    if width < launcher::MIN_WIDTH as u32
                        || height < panel::LAUNCHER_MIN_HEIGHT as u32
                    {
                        return Err(usage);
                    }
                    LauncherSize {
                        width: Some(width),
                        max_height: Some(height),
                    }
                }
                _ => return Err(usage),
            };
            if !self.settings_writable {
                return Err("Settings are not ready for saving; wait for loading or fix the settings error first".into());
            }
            return Ok(Some(Message::LauncherSize(size)));
        }
        Ok(Some(match command {
            "audio open" => Message::Audio(crate::audio::Message::Open),
            "audio toggle" => Message::Audio(crate::audio::Message::Toggle),
            "audio close" => Message::Audio(crate::audio::Message::Close),
            "bar show" => Message::Bar(true),
            "bar hide" => Message::Bar(false),
            "osd close" => Message::OsdClose,
            "notifications dismiss-last" => Message::Notifications(crate::notifications::Message::DismissLast),
            "notifications dismiss-all" => Message::Notifications(crate::notifications::Message::DismissAll),
            "notifications invoke-last" => Message::Notifications(crate::notifications::Message::InvokeLast),
            "notifications show" => Message::Notifications(crate::notifications::Message::Show),
            "notifications dnd" => Message::Notifications(crate::notifications::Message::Dnd),
            "theme light" => Message::Theme(true),
            "theme dark" => Message::Theme(false),
            "notifications toggle" => Message::Notifications(crate::notifications::Message::Toggle),
            "launcher toggle" => Message::Launcher(launcher::Message::Toggle),
            "launcher close" => Message::Launcher(launcher::Message::Close),
            "launcher open" => Message::Launcher(launcher::Message::Mode(Mode::Apps)),
            "wallpapers open" => Message::Launcher(launcher::Message::Mode(Mode::Wallpapers)),
            "widgets open" => Message::Launcher(launcher::Message::Mode(Mode::Widgets)),
            "themes open" => Message::Launcher(launcher::Message::Mode(Mode::Themes)),
            "quit" => Message::Quit,
            _ => return Err(
                "Use launcher toggle|open|close|size, wallpapers open, widgets open, themes open, inspect, or quit"
                    .into(),
            ),
        }))
    }
    fn inspect(&self) -> String {
        serde_json::json!({"desktop_viewport":self.viewport,"dock_surface_expanded":self.launcher.expanded(),"launcher_opening":self.launcher.opening(),"audio":self.audio.inspect(),"wallpaper_presentation":self.wallpaper.inspect_presentation(),"wallpaper_index":self.wallpaper_index, "wallpaper_count":self.wallpapers.len(), "prepared_assets": {
            "icons": crate::previews::usage(self.images.values()),
            "local_previews": self.previews.inspect(),
            "online_previews": crate::previews::usage(self.browser.previews.values()),
            "background": crate::previews::usage(self.wallpaper.background.iter()),
            "total": crate::previews::usage(self.images.values().chain(self.previews.images()).chain(self.browser.previews.values()).chain(self.wallpaper.background.iter())),
        }, "palette":lucent_design::selected_palette(&self.settings),"wallpaper_browser":{"source":self.browser.source,"query":self.browser.query,"page":self.browser.page,"count":self.browser.items.len(),"selected":self.browser.selected,"busy":self.browser.busy(),"status":self.browser.status},"theme": ThemeMode::from_light(self.settings.light).name(), "theme_error": self.theme_error, "bar_visible": self.bar_visible, "background_ready": self.wallpaper.background.is_some(), "wallpaper": self.wallpaper.current, "wallpaper_busy": self.wallpaper.busy(), "notifications_ready":self.notifications.ready,"notification_history_open":self.notifications.history_open,"notification_count":self.notifications.snapshot.active.len(),"launcher":self.launcher.open(),"launcher_size":self.settings.launcher,"launcher_panel_size":[self.launcher.panel_width().target_value(),self.launcher.panel_height().target_value()],"mode":self.launcher.mode().name(),"query":self.launcher.query(),"selected":self.launcher.selected(),"result_count":self.launcher.result_count(),"applications":self.apps.len(),"mapped_icons":self.apps.iter().filter(|a|lucent_design::app_icons::lookup(a.id.as_str()).is_some()).count(),"workspaces":self.compositor.workspaces.iter().map(|w|serde_json::json!({"id":w.id,"active":w.active})).collect::<Vec<_>>(),"widgets":self.settings.visible_widgets,"positions":self.settings.positions,"widget_grid":self.widgets.grid_visible(),"notes":self.settings.notes,"timer_seconds":self.widgets.timer_remaining(),"error":self.error_message(),"settings_error":self.settings_error,"settings_save_status":self.persistence.status(self.settings_writable).name(),"settings_save_error":self.persistence.error(),"wallpaper_error":self.wallpaper.error,"settings_writable":self.settings_writable}).to_string()
    }
    fn animating(&self, surface: &str, now: f64) -> bool {
        (surface == "audio" && self.audio.animating(now))
            || (surface == "osd" && self.osd.animating(now))
            || (surface == "dock" && (self.launcher.animating(now) || self.carousel.active(now)))
    }
}
