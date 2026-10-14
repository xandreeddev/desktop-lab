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
    Outputs(Vec<DisplayOutput>),
    OnMonitor(crate::monitors::Route, Box<Message>),
    SelectMonitor(MonitorId),
    Notifications(crate::notifications::Message),
    Loaded(
        Box<domain::Result<DesktopSettings>>,
        Vec<Wallpaper>,
        domain::Result<Option<WallpaperAsset>>,
    ),
    Catalog(crate::catalog::Message),
    Dock(crate::dock::Message),
    Previews(crate::previews::Message),
    WallpaperSelection(crate::wallpaper_selection::Message),
    Compositor(domain::Result<CompositorSnapshot>),
    Clock(ClockSnapshot),
    System(domain::Result<SystemSnapshot>),
    Audio(crate::audio::Message),
    Network(crate::network::Message),
    Bluetooth(crate::bluetooth::Message),
    Power(crate::power::Message),
    Tray(crate::tray::Message),
    Media(domain::Result<MediaSnapshot>),
    Weather(domain::Result<WeatherSnapshot>),
    Launcher(launcher::Message),
    Launch(AppId, bool),
    Workspace(WorkspaceCommand),
    LiveWallpaper(crate::live_wallpaper::Message),
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
    Resize(&'static str, f32, f32, Density),
    Completed(domain::Result<()>),
    Quit,
}
pub const SPATIAL: [f32; 4] = motion::SPATIAL;
fn widget_surface_specs(
    host: &widgets::Host,
    layout: &WidgetLayout,
    viewport: (f32, f32),
    instance: Option<SurfaceInstance>,
    output: Option<OutputName>,
) -> Vec<SurfaceSpec> {
    host.surfaces(layout, viewport)
        .into_iter()
        .map(|source| SurfaceSpec {
            id: source.id,
            instance: instance.clone(),
            output: output.clone(),
            layer: Layer::Bottom,
            anchor: Anchor::TopLeft,
            margins: SurfaceMargins {
                top: source.rect.y as i32,
                left: source.rect.x as i32,
                ..Default::default()
            },
            redraw_group: Some("widgets"),
            width: source.rect.w.ceil() as u32,
            height: source.rect.h.ceil() as u32,
            exclusive_zone: -1,
            keyboard: Keyboard::OnDemand,
            visible: true,
            pointer_input: PointerInput::Content,
        })
        .collect()
}
pub struct Desktop {
    pub live_wallpaper: crate::live_wallpaper::LiveWallpaper,
    pub display_catalog_seen: bool,
    pub live_suspended: bool,
    pub notifications: crate::notifications::Center,
    pub osd: crate::osd::Osd,
    pub bar_visible: bool,
    pub theme_error: String,
    pub widgets: widgets::Host,
    pub monitors: crate::monitors::Monitors,
    pub apps: Vec<domain::Application>,
    pub catalog: crate::catalog::Catalog,
    pub dock: crate::dock::Dock,
    pub settings: DesktopSettings,
    pub persistence: crate::persistence::Persistence,
    pub settings_writable: bool,
    pub settings_error: String,
    pub images: BTreeMap<String, Arc<ImageData>>,
    pub previews: crate::previews::Previews,
    pub wallpaper: crate::wallpaper_selection::Selection,
    pub compositor: CompositorSnapshot,
    pub compositor_available: bool,
    pub compositor_error: String,
    pub clock: ClockSnapshot,
    pub system: SystemSnapshot,
    pub audio: crate::audio::Audio,
    pub network: crate::network::Network,
    pub bluetooth: crate::bluetooth::Bluetooth,
    pub power: crate::power::Power,
    pub tray: crate::tray::Tray,
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
            live_wallpaper: crate::live_wallpaper::LiveWallpaper::new(
                adapters.playback.clone(),
                adapters.assets.clone(),
            ),
            display_catalog_seen: false,
            live_suspended: false,
            osd: Default::default(),
            bar_visible: true,
            theme_error: String::new(),
            notifications: crate::notifications::Center::new(
                adapters.notifications.clone(),
                adapters.uri.clone(),
                adapters.assets.clone(),
            ),
            widgets: widgets::Host::default(),
            monitors: Default::default(),
            apps: vec![],
            catalog: crate::catalog::Catalog::new(adapters.assets.clone()),
            dock: Default::default(),
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
            compositor_available: false,
            compositor_error: String::new(),
            clock: adapters.clock.now(),
            system: SystemSnapshot::default(),
            audio: crate::audio::Audio::new(adapters.audio.clone()),
            network: crate::network::Network::new(adapters.network.clone()),
            bluetooth: crate::bluetooth::Bluetooth::new(adapters.bluetooth.clone()),
            power: crate::power::Power::new(adapters.power.clone()),
            tray: crate::tray::Tray::new(adapters.tray.clone(), adapters.assets.clone()),
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
    fn update_live_wallpaper(
        &mut self,
        message: crate::live_wallpaper::Message,
        effects: &mut Effects<Message>,
    ) {
        if !self.settings_writable
            && matches!(
                message,
                crate::live_wallpaper::Message::Import
                    | crate::live_wallpaper::Message::ImportPath(..)
                    | crate::live_wallpaper::Message::Pause(_)
                    | crate::live_wallpaper::Message::Off
                    | crate::live_wallpaper::Message::Rate(_)
            )
        {
            self.live_wallpaper.error = "Saved preferences are not writable".into();
            effects.redraw("live-wallpaper");
            return;
        }
        let is_frame = matches!(
            message,
            crate::live_wallpaper::Message::Wake(..) | crate::live_wallpaper::Message::Frame(_)
        );
        let mut intent = None;
        effects.delegate(
            |child| intent = self.live_wallpaper.update(message, child),
            Message::LiveWallpaper,
        );
        if let Some(intent) = intent {
            use crate::live_wallpaper::Intent;
            match intent {
                Intent::FrameChanged(targets) => {
                    for target in targets {
                        if let Some(id) = target {
                            if let Some(host) = self.monitors.connected.get(&id) {
                                effects.redraw_surface(
                                    SurfaceAddress::new("background")
                                        .instance(host.instance.clone()),
                                );
                            }
                        } else {
                            effects.redraw("background");
                        }
                    }
                }
                intent => {
                    let mut next = self.settings.clone();
                    let mut poster = None;
                    match intent {
                        Intent::Apply(target, value) => {
                            if let Some(target) = target {
                                next.live_wallpapers.monitors.insert(target, value);
                            } else {
                                poster = Some(value.asset.poster.clone());
                                next.live_wallpapers.default = Some(value);
                            }
                            next.live_wallpapers.paused = false;
                        }
                        Intent::Pause(value) => next.live_wallpapers.paused = value,
                        Intent::Off(target) => {
                            if let Some(target) = target {
                                if let Some(mut setting) = next
                                    .live_wallpapers
                                    .monitors
                                    .get(&target)
                                    .or(next.live_wallpapers.default.as_ref())
                                    .cloned()
                                {
                                    setting.enabled = false;
                                    next.live_wallpapers.monitors.insert(target, setting);
                                }
                            } else {
                                next.live_wallpapers.default = None;
                            }
                        }
                        Intent::Rate(target, value) => {
                            if let Some(target) = &target
                                && !next.live_wallpapers.monitors.contains_key(target)
                                && let Some(default) = next.live_wallpapers.default.clone()
                            {
                                next.live_wallpapers
                                    .monitors
                                    .insert(target.clone(), default);
                            }
                            let selected = target
                                .as_ref()
                                .and_then(|id| next.live_wallpapers.monitors.get_mut(id));
                            if let Some(selected) = selected {
                                selected.fps = value;
                            } else if let Some(default) = next.live_wallpapers.default.as_mut() {
                                default.fps = value;
                            }
                        }
                        Intent::FrameChanged(_) => unreachable!(),
                    }
                    if let Err(error) = lucent_usecases::validate_settings(&next) {
                        self.live_wallpaper.error = error.to_string();
                    } else {
                        self.settings = next;
                        self.save(effects);
                        if let Some(source) = poster {
                            self.choose_wallpaper(
                                crate::wallpaper_selection::Choice {
                                    source,
                                    colors: None,
                                    browser_generation: None,
                                },
                                effects,
                            );
                        }
                        effects.redraw("background");
                    }
                }
            }
        }
        self.reconcile_live_wallpapers();
        if !is_frame {
            effects.redraw("live-wallpaper");
        }
    }
    fn reconcile_live_wallpapers(&mut self) {
        let monitors = self.monitors.connected.keys().cloned().collect::<Vec<_>>();
        self.live_wallpaper.reconcile(
            &self.settings.live_wallpapers,
            &monitors,
            self.display_catalog_seen,
            self.live_suspended,
        );
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
        effects.task_on(crate::work::APPEARANCE, move || {
            Message::ThemeApplied(theme.apply(&palette))
        });
        for id in [
            "bar",
            "widgets",
            "dock",
            "notifications",
            "background",
            "osd",
            "audio",
            "network",
            "bluetooth",
            "power",
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
        } else if let Some(error) = self.catalog.error() {
            error
        } else if let Some(error) = self.wallpaper.presentation_error() {
            error
        } else if !self.compositor_error.is_empty() {
            &self.compositor_error
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
        self.dock.apps(
            crate::dock::Input {
                apps: &self.apps,
                compositor: &self.compositor,
                settings: &self.settings,
            },
            crate::shell_layout::dock_capacity(self.viewport.0).saturating_sub(1),
        )
    }
    fn update_dock(&mut self, message: crate::dock::Message, effects: &mut Effects<Message>) {
        let visible = self
            .dock_apps()
            .iter()
            .map(|app| app.id.clone())
            .collect::<Vec<_>>();
        let mut intent = None;
        effects.delegate(
            |child| {
                intent = self
                    .dock
                    .update(message, &mut self.settings, &self.apps, &visible, child)
            },
            Message::Dock,
        );
        match intent {
            Some(crate::dock::Intent::Save) => {
                self.save(effects);
                self.retarget_panel_transition(effects.now);
            }
            Some(crate::dock::Intent::Error(error)) => self.error = error,
            None => (),
        }
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
            application_status: self.catalog.status(),
            widgets_editing: self.active_widgets().editing(),
            settings: &self.settings,
            widget_layout: self
                .monitors
                .active
                .as_ref()
                .and_then(|id| self.settings.monitor_layouts.get(id))
                .unwrap_or(&self.settings.layout),
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
        self.update_dock(crate::dock::Message::Forced(self.launcher.open()), effects);
        if self.launcher.open() {
            effects.delegate(
                |child| self.power.update(crate::power::Message::Close, child),
                Message::Power,
            );
            effects.delegate(
                |child| {
                    self.bluetooth
                        .update(crate::bluetooth::Message::Close, child)
                },
                Message::Bluetooth,
            );
            effects.delegate(
                |child| self.network.update(crate::network::Message::Close, child),
                Message::Network,
            );
            effects.delegate(
                |child| self.audio.update(crate::audio::Message::Close, child),
                Message::Audio,
            );
        }
        if let Some(intent) = intent {
            use launcher::Intent;
            let message = match intent {
                Intent::Error(error) => Message::Completed(Err(error)),
                Intent::Launch(id, prefer) => Message::Launch(id, prefer),
                Intent::Pin(id) => Message::Dock(crate::dock::Message::Pin(id)),
                Intent::Action(action) => Message::Action(action),
                Intent::Wallpaper(index, apply) => Message::Wallpaper(index, apply),
                Intent::Browser(message) => Message::Browser(message),
                Intent::ToggleWidget(id) => Message::Widgets(widgets::Message::Toggle(id)),
                Intent::ResetLayout => Message::Widgets(widgets::Message::ResetLayout),
                Intent::EditWidgets(enabled) => {
                    self.update_launcher(launcher::Message::Close, effects);
                    Message::Widgets(widgets::Message::Edit(enabled))
                }
                Intent::Palette(palette) => Message::Palette(palette),
                Intent::ExtractPalette => Message::ExtractPalette,
                Intent::LauncherSize(size) => Message::LauncherSize(size),
            };
            self.update(message, effects);
        }
    }
    pub fn active_widgets(&self) -> &widgets::Host {
        self.monitors
            .current()
            .map_or(&self.widgets, |(_, monitor)| &monitor.widgets)
    }
    pub fn widget_layout(&self, id: Option<&MonitorId>) -> &WidgetLayout {
        id.and_then(|id| self.settings.monitor_layouts.get(id))
            .unwrap_or(&self.settings.layout)
    }
    pub fn active_widget_layout(&self) -> &WidgetLayout {
        self.widget_layout(self.monitors.active.as_ref())
    }
    fn select_monitor(&mut self, id: &MonitorId, effects: &mut Effects<Message>) {
        let Some(monitor) = self.monitors.connected.get(id) else {
            return;
        };
        let viewport = monitor.viewport;
        if self.monitors.active.as_ref() == Some(id) && self.viewport == viewport {
            return;
        }
        self.monitors.active = Some(id.clone());
        self.viewport = viewport;
        self.update_dock(crate::dock::Message::Cancel, effects);
        self.retarget_panel_transition(effects.now);
        for role in [
            "dock",
            "audio",
            "network",
            "bluetooth",
            "power",
            "tray",
            "notifications",
            "osd",
        ] {
            effects.redraw(role);
        }
    }
    fn explicitly_select_monitor(&mut self, id: &MonitorId, effects: &mut Effects<Message>) {
        if self.monitors.connected.contains_key(id) {
            self.monitors.pending_focus = None;
            self.select_monitor(id, effects);
        }
    }
    fn resize_monitor_background(&mut self, effects: &mut Effects<Message>) {
        match self.monitors.background_target() {
            Ok(Some(target)) => effects.delegate(
                |child| self.wallpaper.resize(target, child),
                Message::WallpaperSelection,
            ),
            Ok(None) => {}
            Err(error) => {
                self.error = error.to_string();
                effects.redraw("dock");
            }
        }
    }
    fn widget_view_on(&self, id: Option<&MonitorId>) -> widgets::View<'_> {
        let host = id
            .and_then(|id| self.monitors.connected.get(id))
            .map_or(&self.widgets, |monitor| &monitor.widgets);
        widgets::View {
            host,
            input: widgets::Input {
                layout: self.widget_layout(id),
                notes: &self.settings.notes,
                clock: &self.clock,
                system: &self.system,
                network_status: self.network.status(),
                media: &self.media,
                weather: self.weather.as_ref(),
                save_status: self.persistence.status(self.settings_writable),
            },
            artwork: self.artwork(),
        }
    }
    fn update_widgets(&mut self, message: widgets::Message, effects: &mut Effects<Message>) {
        let selected = self.monitors.active.clone();
        self.update_widgets_on(selected.as_ref(), message, effects);
    }
    fn update_widgets_on(
        &mut self,
        id: Option<&MonitorId>,
        message: widgets::Message,
        effects: &mut Effects<Message>,
    ) {
        if let (Some(id), widgets::Message::Tick(now)) = (id, &message) {
            let layout = self
                .settings
                .monitor_layouts
                .get(id)
                .unwrap_or(&self.settings.layout);
            let Some(monitor) = self.monitors.connected.get_mut(id) else {
                return;
            };
            if monitor
                .widgets
                .tick(*now, layout.visible_widgets.iter().any(|id| id == "timer"))
            {
                for source in monitor.widgets.surfaces(layout, monitor.viewport) {
                    effects.redraw_surface(
                        SurfaceAddress::new(source.id).instance(monitor.instance.clone()),
                    );
                }
            }
            return;
        }
        let mut intent = None;
        let route = id.and_then(|id| {
            self.monitors
                .connected
                .get(id)
                .map(|monitor| monitor.route(id))
        });
        let instance = route
            .as_ref()
            .and_then(|route| self.monitors.connected.get(&route.id))
            .map(|monitor| monitor.instance.clone());
        let shared_notes = matches!(message, widgets::Message::EditNotes(_));
        let (host, layout, viewport) = if let Some(route) = &route {
            if let Err(error) =
                lucent_usecases::ensure_monitor_layout(&mut self.settings, &route.id)
            {
                self.error = error.to_string();
                effects.redraw("dock");
                return;
            }
            let layout = self
                .settings
                .monitor_layouts
                .get_mut(&route.id)
                .expect("initialized monitor layout");
            let monitor = self
                .monitors
                .connected
                .get_mut(&route.id)
                .expect("current route");
            (&mut monitor.widgets, layout, monitor.viewport)
        } else {
            (&mut self.widgets, &mut self.settings.layout, self.viewport)
        };
        effects.delegate(
            |child| {
                intent = host.update(
                    message,
                    widgets::Update {
                        layout,
                        notes: &mut self.settings.notes,
                        instance: instance.clone(),
                        viewport,
                        unix_seconds: self.clock.unix_seconds,
                    },
                    child,
                );
                if let Some(instance) = &instance
                    && !shared_notes
                    && child.redraw.remove("widgets")
                {
                    child.redraw_surface(SurfaceAddress::new("widgets").instance(instance.clone()));
                    for source in host.surfaces(layout, viewport) {
                        child.redraw_surface(
                            SurfaceAddress::new(source.id).instance(instance.clone()),
                        );
                    }
                }
            },
            move |message| match &route {
                Some(route) => {
                    Message::OnMonitor(route.clone(), Box::new(Message::Widgets(message)))
                }
                None => Message::Widgets(message),
            },
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
        effects.task_on(crate::work::CONTROLS, move || {
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
        if matches!(action, Action::DockAutoHide) {
            self.update_dock(
                crate::dock::Message::AutoHide(!self.settings.dock.auto_hide),
                effects,
            );
            return;
        }
        if matches!(action, Action::Quit) {
            effects.quit();
            return;
        }
        if matches!(action, Action::LiveWallpapers) {
            self.update(
                Message::LiveWallpaper(crate::live_wallpaper::Message::Open),
                effects,
            );
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
                crate::work::SESSION
            } else {
                crate::work::CONTROLS
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
        let route = cx
            .instance
            .as_ref()
            .and_then(|instance| self.monitors.route(instance));
        if cx.instance.is_some() && route.is_none() {
            return Element::empty();
        }
        let monitor = route
            .as_ref()
            .and_then(|route| self.monitors.connected.get(&route.id));
        let viewport = monitor.map_or(self.viewport, |monitor| monitor.viewport);
        let tree = self.theme().apply(match cx.surface {
            "audio" => self.audio.view(cx, self.theme()).map(Message::Audio),
            "network" => self.network.view(cx, self.theme()).map(Message::Network),
            "bluetooth" => self
                .bluetooth
                .view(cx, self.theme())
                .map(Message::Bluetooth),
            "power" => self.power.view(cx, self.theme()).map(Message::Power),
            "tray" => self.tray.view(cx, self.theme()).map(Message::Tray),
            "osd" => self.osd.view(cx, self.theme()),
            "background" => self
                .live_wallpaper
                .frame(route.as_ref().map(|route| &route.id))
                .or(self.wallpaper.background.as_ref())
                .map(|image| Element::image(image.clone()).cover().fill())
                .unwrap_or_else(|| Element::empty().fill().background(self.surface_color())),
            "live-wallpaper" => self
                .live_wallpaper
                .view(
                    cx,
                    &self.settings.live_wallpapers,
                    &self.monitors.connected.keys().cloned().collect::<Vec<_>>(),
                    self.theme(),
                )
                .map(Message::LiveWallpaper),
            "bar" => self.bar(cx),
            "widgets" => self
                .widget_view_on(route.as_ref().map(|route| &route.id))
                .render(cx)
                .map(Message::Widgets),
            name if widgets::catalog::find(name).is_some() => self
                .widget_view_on(route.as_ref().map(|route| &route.id))
                .render_native(cx, viewport)
                .map(Message::Widgets),
            "dock" => self.dock(cx),
            "notifications" => self
                .notifications
                .view_with_theme(cx, self.theme())
                .map(Message::Notifications),
            _ => Element::empty(),
        });
        match route {
            Some(route) => {
                tree.map(move |message| Message::OnMonitor(route.clone(), Box::new(message)))
            }
            None => tree,
        }
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        let now = effects.now;
        let other_panel = match &message {
            Message::Power(crate::power::Message::Open)
            | Message::Network(crate::network::Message::Open)
            | Message::Bluetooth(crate::bluetooth::Message::Open)
            | Message::Audio(crate::audio::Message::Open)
            | Message::Launcher(launcher::Message::Mode(_)) => true,
            Message::Power(crate::power::Message::Toggle) => !self.power.open(),
            Message::Network(crate::network::Message::Toggle) => !self.network.open(),
            Message::Bluetooth(crate::bluetooth::Message::Toggle) => !self.bluetooth.open(),
            Message::Audio(crate::audio::Message::Toggle) => !self.audio.open(),
            Message::Launcher(launcher::Message::Toggle) => !self.launcher.open(),
            _ => false,
        };
        if other_panel {
            self.update_live_wallpaper(crate::live_wallpaper::Message::Close, effects);
            effects.delegate(
                |child| self.tray.update(crate::tray::Message::Close, child),
                Message::Tray,
            );
        }
        match message {
            Message::Outputs(catalog) => {
                self.display_catalog_seen = true;
                self.monitors.reconcile(catalog, self.viewport);
                let pending = self
                    .monitors
                    .pending_focus
                    .as_ref()
                    .filter(|id| self.monitors.connected.contains_key(*id))
                    .cloned();
                let selected = pending.clone().or_else(|| self.monitors.active.clone());
                if pending.is_some() {
                    self.monitors.pending_focus = None;
                }
                if let Some(id) = selected {
                    self.select_monitor(&id, effects);
                }
                self.resize_monitor_background(effects);
                for role in [
                    "background",
                    "bar",
                    "widgets",
                    "dock",
                    "audio",
                    "network",
                    "bluetooth",
                    "power",
                    "tray",
                    "notifications",
                    "osd",
                ] {
                    effects.redraw(role);
                }
            }
            Message::SelectMonitor(id) => self.explicitly_select_monitor(&id, effects),
            Message::OnMonitor(route, message) => {
                if !self.monitors.accepts(&route) {
                    return;
                }
                match *message {
                    Message::Resize("background", width, height, density) => {
                        if crate::ports::BackgroundTarget::from_density(width, height, density)
                            .is_err()
                        {
                            return;
                        }
                        let monitor = self
                            .monitors
                            .connected
                            .get_mut(&route.id)
                            .expect("validated route");
                        if monitor.viewport != (width, height) || monitor.density != density {
                            monitor.widgets.cancel_interactions();
                            monitor.viewport = (width, height);
                            monitor.density = density;
                        }
                        if self.monitors.active.as_ref() == Some(&route.id) {
                            self.select_monitor(&route.id, effects);
                        }
                        self.resize_monitor_background(effects);
                        effects.redraw("widgets");
                        effects.redraw_surface(
                            SurfaceAddress::new("background")
                                .instance(self.monitors.connected[&route.id].instance.clone()),
                        );
                    }
                    Message::Resize(role, _, _, _) => {
                        effects.redraw_surface(
                            SurfaceAddress::new(role)
                                .instance(self.monitors.connected[&route.id].instance.clone()),
                        );
                    }
                    Message::Widgets(message) => {
                        if !matches!(
                            message,
                            widgets::Message::EditNotes(TextEdit::ClipboardReply(..))
                                | widgets::Message::Tick(_)
                                | widgets::Message::CancelDrags
                        ) {
                            self.explicitly_select_monitor(&route.id, effects);
                        }
                        self.update_widgets_on(Some(&route.id), message, effects);
                    }
                    message => {
                        self.explicitly_select_monitor(&route.id, effects);
                        self.update(message, effects);
                    }
                }
            }
            Message::Notifications(message) => effects.delegate(
                |e| self.notifications.update(message, e),
                Message::Notifications,
            ),
            Message::Loaded(settings, wallpapers, current_wallpaper) => {
                self.monitors.cancel_interactions();
                self.update_widgets(widgets::Message::CancelDrags, effects);
                match *settings {
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
                self.update_dock(crate::dock::Message::Cancel, effects);
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
                let theme = self.adapters.theme.clone();
                let palette = lucent_design::selected_palette(&self.settings);
                effects.task_on(crate::work::APPEARANCE, move || {
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
            Message::Dock(message) => self.update_dock(message, effects),
            Message::Catalog(message) => {
                let selected = self.launcher.selected_application(&self.apps).cloned();
                let mut changed = false;
                effects.delegate(
                    |child| {
                        changed =
                            self.catalog
                                .update(message, &mut self.apps, &mut self.images, child)
                    },
                    Message::Catalog,
                );
                if changed {
                    self.update_dock(crate::dock::Message::Cancel, effects);
                    self.launcher.refresh_preserving(
                        &self.apps,
                        selected.as_ref(),
                        self.launcher_geometry(),
                        now,
                    );
                    effects.redraw("dock");
                }
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
                        if self
                            .settings
                            .live_wallpapers
                            .default
                            .as_ref()
                            .is_some_and(|live| live.asset.poster.path != source.path)
                        {
                            self.settings.live_wallpapers.default = None;
                            self.save(effects);
                        }
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
                    let recovered = !self.compositor_available;
                    let focus_changed =
                        recovered || self.compositor.focused_monitor() != value.focused_monitor();
                    self.compositor_available = true;
                    self.compositor_error.clear();
                    if self.compositor != value || recovered {
                        self.update_dock(crate::dock::Message::Cancel, effects);
                        self.compositor = value;
                        if focus_changed {
                            self.monitors.pending_focus =
                                self.compositor.focused_monitor().cloned();
                            if let Some(id) = self.monitors.pending_focus.clone()
                                && self.monitors.connected.contains_key(&id)
                            {
                                self.monitors.pending_focus = None;
                                self.select_monitor(&id, effects);
                            }
                        }
                        self.retarget_panel_transition(now);
                        effects.redraw("bar");
                        effects.redraw("dock");
                    }
                }
                Err(e) => {
                    self.compositor_available = false;
                    self.compositor_error = e.to_string();
                    self.monitors.pending_focus = None;
                    effects.redraw("dock");
                }
            },
            Message::Clock(clock) => {
                if self.clock.minute != clock.minute || self.clock.date != clock.date {
                    effects.redraw("bar");
                    effects.redraw("widgets");
                }
                if self.monitors.legacy() {
                    self.update_widgets_on(
                        None,
                        widgets::Message::Tick(clock.unix_seconds),
                        effects,
                    );
                }
                let monitors: Vec<_> = self.monitors.connected.keys().cloned().collect();
                for id in monitors {
                    self.update_widgets_on(
                        Some(&id),
                        widgets::Message::Tick(clock.unix_seconds),
                        effects,
                    );
                }
                self.clock = clock;
            }
            Message::System(Ok(value)) => {
                if self
                    .settings
                    .layout
                    .visible_widgets
                    .iter()
                    .any(|w| w == "system")
                    || self
                        .settings
                        .monitor_layouts
                        .values()
                        .any(|layout| layout.visible_widgets.iter().any(|w| w == "system"))
                {
                    effects.redraw("widgets");
                }
                self.system = value;
            }
            Message::System(Err(_)) => {}
            Message::Audio(message) => {
                let opening = matches!(message, crate::audio::Message::Open)
                    || matches!(message, crate::audio::Message::Toggle) && !self.audio.open();
                if opening {
                    effects.delegate(
                        |child| self.power.update(crate::power::Message::Close, child),
                        Message::Power,
                    );
                    effects.delegate(
                        |child| {
                            self.bluetooth
                                .update(crate::bluetooth::Message::Close, child)
                        },
                        Message::Bluetooth,
                    );
                    self.update_launcher(launcher::Message::Close, effects);
                    effects.delegate(
                        |child| self.network.update(crate::network::Message::Close, child),
                        Message::Network,
                    );
                }
                effects.delegate(|child| self.audio.update(message, child), Message::Audio);
            }
            Message::Network(message) => {
                let opening = matches!(message, crate::network::Message::Open)
                    || matches!(message, crate::network::Message::Toggle) && !self.network.open();
                if opening {
                    effects.delegate(
                        |child| self.power.update(crate::power::Message::Close, child),
                        Message::Power,
                    );
                    effects.delegate(
                        |child| {
                            self.bluetooth
                                .update(crate::bluetooth::Message::Close, child)
                        },
                        Message::Bluetooth,
                    );
                    self.update_launcher(launcher::Message::Close, effects);
                    effects.delegate(
                        |child| self.audio.update(crate::audio::Message::Close, child),
                        Message::Audio,
                    );
                }
                effects.delegate(
                    |child| self.network.update(message, child),
                    Message::Network,
                );
            }
            Message::Bluetooth(message) => {
                let before = self.bluetooth.open();
                effects.delegate(
                    |child| self.bluetooth.update(message, child),
                    Message::Bluetooth,
                );
                if self.bluetooth.open() && !before {
                    effects.delegate(
                        |child| self.power.update(crate::power::Message::Close, child),
                        Message::Power,
                    );
                    self.update_launcher(launcher::Message::Close, effects);
                    effects.delegate(
                        |child| self.audio.update(crate::audio::Message::Close, child),
                        Message::Audio,
                    );
                    effects.delegate(
                        |child| self.network.update(crate::network::Message::Close, child),
                        Message::Network,
                    );
                }
            }
            Message::Tray(message) => {
                let opening = matches!(message, crate::tray::Message::Open)
                    || matches!(message, crate::tray::Message::Toggle) && !self.tray.open();
                if opening {
                    self.update_launcher(launcher::Message::Close, effects);
                    effects.delegate(
                        |child| self.power.update(crate::power::Message::Close, child),
                        Message::Power,
                    );
                    effects.delegate(
                        |child| self.audio.update(crate::audio::Message::Close, child),
                        Message::Audio,
                    );
                    effects.delegate(
                        |child| self.network.update(crate::network::Message::Close, child),
                        Message::Network,
                    );
                    effects.delegate(
                        |child| {
                            self.bluetooth
                                .update(crate::bluetooth::Message::Close, child)
                        },
                        Message::Bluetooth,
                    );
                }
                effects.delegate(|child| self.tray.update(message, child), Message::Tray);
            }
            Message::Power(message) => {
                if let crate::power::Message::Observed(Ok(snapshot)) = &message {
                    self.live_suspended = snapshot.preparing_for_sleep;
                }
                let opening = matches!(message, crate::power::Message::Open)
                    || matches!(message, crate::power::Message::Toggle) && !self.power.open();
                if opening {
                    self.update_launcher(launcher::Message::Close, effects);
                    effects.delegate(
                        |child| self.audio.update(crate::audio::Message::Close, child),
                        Message::Audio,
                    );
                    effects.delegate(
                        |child| self.network.update(crate::network::Message::Close, child),
                        Message::Network,
                    );
                    effects.delegate(
                        |child| {
                            self.bluetooth
                                .update(crate::bluetooth::Message::Close, child)
                        },
                        Message::Bluetooth,
                    );
                }
                effects.delegate(|child| self.power.update(message, child), Message::Power);
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
            }
            Message::Launch(id, prefer) => {
                self.launch(&id, prefer, effects);
                self.update_launcher(launcher::Message::Close, effects);
            }
            Message::LiveWallpaper(message) => {
                if matches!(message, crate::live_wallpaper::Message::Open) {
                    self.update_launcher(launcher::Message::Close, effects);
                    effects.delegate(
                        |child| self.audio.update(crate::audio::Message::Close, child),
                        Message::Audio,
                    );
                    effects.delegate(
                        |child| self.network.update(crate::network::Message::Close, child),
                        Message::Network,
                    );
                    effects.delegate(
                        |child| {
                            self.bluetooth
                                .update(crate::bluetooth::Message::Close, child)
                        },
                        Message::Bluetooth,
                    );
                    effects.delegate(
                        |child| self.power.update(crate::power::Message::Close, child),
                        Message::Power,
                    );
                    effects.delegate(
                        |child| self.tray.update(crate::tray::Message::Close, child),
                        Message::Tray,
                    );
                }
                self.update_live_wallpaper(message, effects)
            }
            Message::Workspace(command) => {
                // Retained named/special tree actions cannot invent a stale
                // observed destination. Numbered shortcuts remain client policy.
                let id = match &command {
                    WorkspaceCommand::Select(id) | WorkspaceCommand::ToggleSpecial(id) => id,
                };
                let present = id.number().is_some()
                    || self
                        .compositor
                        .workspaces
                        .iter()
                        .filter(|workspace| &workspace.id == id)
                        .count()
                        == 1;
                if present
                    && self.compositor_available
                    && let Some(adapter) = self.adapters.compositor.clone()
                {
                    effects.task_on(crate::work::CONTROLS, move || {
                        Message::Completed(adapter.workspace(command))
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
                effects.task_on(crate::work::ASSETS, move || {
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
                effects.delegate(
                    |child| self.browser.update(message, child),
                    Message::Browser,
                );
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
            Message::Resize(id, w, h, density) => {
                self.update_dock(crate::dock::Message::Cancel, effects);
                if id == "background" {
                    match crate::ports::BackgroundTarget::from_density(w, h, density) {
                        Ok(target) => {
                            self.update_widgets(widgets::Message::CancelDrags, effects);
                            effects.delegate(
                                |child| {
                                    self.viewport = (w, h);
                                    self.wallpaper.resize(target, child);
                                },
                                Message::WallpaperSelection,
                            );
                        }
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
        self.reconcile_live_wallpapers();
        self.refresh_previews(effects);
    }
}
impl api::Application for Desktop {
    fn clipboard_enabled(&self) -> bool {
        true
    }
    fn runtime_failure(&self, failure: api::RuntimeFailure) -> Option<Message> {
        if failure.source == "effect queue bluetooth" {
            // A native write may have run before a panic/admission failure.
            // Recreate its owner rather than guessing or replaying the outcome.
            return None;
        }
        if failure.source == "effect queue network" {
            return Some(Message::Network(crate::network::Message::WorkerFailed(
                failure.to_string(),
            )));
        }
        if failure.source == "effect queue audio" {
            return Some(Message::Audio(crate::audio::Message::Control(
                crate::audio::commands::Message::WorkerFailed(failure.to_string()),
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
        if matches!(
            failure.source.as_str(),
            "effect queue appearance" | "effect queue assets"
        ) {
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
            include_bytes!("../assets/LucentBold.ttf"),
            include_bytes!("../assets/LucentItalic.ttf"),
            include_bytes!("../assets/LucentBoldItalic.ttf"),
        ]
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        let mut surfaces = vec![
            SurfaceSpec {
                id: "live-wallpaper",
                instance: None,
                output: None,
                layer: Layer::Overlay,
                anchor: Anchor::Fill,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: if self.live_wallpaper.open {
                    Keyboard::OnDemand
                } else {
                    Keyboard::None
                },
                visible: self.live_wallpaper.mounted,
                pointer_input: if self.live_wallpaper.open {
                    PointerInput::All
                } else {
                    PointerInput::None
                },
            },
            SurfaceSpec {
                id: "tray",
                instance: None,
                output: None,
                layer: Layer::Overlay,
                anchor: Anchor::Fill,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: if self.tray.open() {
                    Keyboard::Exclusive
                } else {
                    Keyboard::None
                },
                visible: self.tray.visible(),
                pointer_input: if self.tray.open() {
                    PointerInput::All
                } else {
                    PointerInput::None
                },
            },
            SurfaceSpec {
                id: "power",
                instance: None,
                output: None,
                layer: Layer::Overlay,
                anchor: Anchor::Fill,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: if self.power.open() {
                    Keyboard::Exclusive
                } else {
                    Keyboard::None
                },
                visible: self.power.visible(),
                pointer_input: if self.power.open() {
                    PointerInput::All
                } else {
                    PointerInput::None
                },
            },
            SurfaceSpec {
                id: "bluetooth",
                instance: None,
                output: None,
                layer: Layer::Overlay,
                anchor: Anchor::Fill,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: if self.bluetooth.open() {
                    Keyboard::Exclusive
                } else {
                    Keyboard::None
                },
                visible: self.bluetooth.visible(),
                pointer_input: if self.bluetooth.open() {
                    PointerInput::All
                } else {
                    PointerInput::None
                },
            },
            SurfaceSpec {
                id: "network",
                instance: None,
                output: None,
                layer: Layer::Overlay,
                anchor: Anchor::Fill,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: if self.network.open() {
                    Keyboard::Exclusive
                } else {
                    Keyboard::None
                },
                visible: self.network.visible(),
                pointer_input: if self.network.open() {
                    PointerInput::All
                } else {
                    PointerInput::None
                },
            },
            SurfaceSpec {
                id: "audio",
                instance: None,
                output: None,
                layer: Layer::Overlay,
                anchor: Anchor::Fill,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: if self.audio.open() {
                    Keyboard::Exclusive
                } else {
                    Keyboard::None
                },
                visible: self.audio.visible(),
                pointer_input: if self.audio.open() {
                    PointerInput::All
                } else {
                    PointerInput::Content
                },
            },
            SurfaceSpec {
                id: "osd",
                instance: None,
                output: None,
                layer: Layer::Overlay,
                anchor: Anchor::Fill,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: Keyboard::None,
                visible: self.osd.feedback.is_some(),
                pointer_input: lucent_api::PointerInput::Content,
            },
            SurfaceSpec {
                id: "background",
                instance: None,
                output: None,
                layer: Layer::Background,
                anchor: Anchor::Fill,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: Keyboard::None,
                visible: true,
                pointer_input: lucent_api::PointerInput::Content,
            },
            SurfaceSpec {
                id: "notifications",
                instance: None,
                output: None,
                layer: Layer::Overlay,
                anchor: Anchor::Fill,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: Keyboard::OnDemand,
                visible: self.notifications.visible(),
                pointer_input: lucent_api::PointerInput::Content,
            },
            SurfaceSpec {
                id: "bar",
                instance: None,
                output: None,
                layer: Layer::Top,
                anchor: Anchor::Top,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: panel::BAR_HEIGHT as u32,
                exclusive_zone: panel::BAR_HEIGHT as i32,
                keyboard: Keyboard::None,
                visible: self.bar_visible,
                pointer_input: lucent_api::PointerInput::Content,
            },
            SurfaceSpec {
                id: "widgets",
                instance: None,
                output: None,
                layer: Layer::Bottom,
                anchor: Anchor::Fill,
                margins: Default::default(),
                redraw_group: Some("widgets"),
                width: 0,
                height: 0,
                exclusive_zone: -1,
                keyboard: Keyboard::None,
                visible: self.widgets.grid_visible(),
                pointer_input: PointerInput::None,
            },
            SurfaceSpec {
                id: "dock",
                instance: None,
                output: None,
                layer: if self.launcher.open() {
                    Layer::Overlay
                } else {
                    Layer::Top
                },
                anchor: Anchor::Bottom,
                margins: Default::default(),
                redraw_group: None,
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
                pointer_input: if self.launcher.open() {
                    PointerInput::All
                } else {
                    PointerInput::Content
                },
            },
        ];
        if !self.monitors.legacy() {
            if self.monitors.connected.is_empty() {
                return vec![];
            }
            let repeated: Vec<_> = surfaces
                .iter()
                .filter(|spec| ["bar", "background", "widgets"].contains(&spec.id))
                .cloned()
                .collect();
            surfaces.retain(|spec| !["bar", "background", "widgets"].contains(&spec.id));
            let active = self
                .monitors
                .current()
                .map(|(_, monitor)| monitor.target.clone());
            for spec in &mut surfaces {
                spec.output = active.clone();
            }
            for (id, monitor) in &self.monitors.connected {
                for template in &repeated {
                    let mut spec = template.clone();
                    spec.output = Some(monitor.target.clone());
                    spec.instance = Some(monitor.instance.clone());
                    if spec.id == "widgets" {
                        spec.visible = monitor.widgets.grid_visible();
                    }
                    surfaces.push(spec);
                }
                surfaces.extend(widget_surface_specs(
                    &monitor.widgets,
                    self.widget_layout(Some(id)),
                    monitor.viewport,
                    Some(monitor.instance.clone()),
                    Some(monitor.target.clone()),
                ));
            }
            return surfaces;
        }
        surfaces.extend(widget_surface_specs(
            &self.widgets,
            &self.settings.layout,
            self.viewport,
            None,
            None,
        ));
        surfaces
    }
    fn init(&mut self, effects: &mut Effects<Message>) {
        let store = self.adapters.settings.clone();
        let wallpaper = self.adapters.wallpaper.clone();
        effects.task_on(crate::work::ASSETS, move || {
            Message::Loaded(
                Box::new(store.load()),
                wallpaper.list(),
                wallpaper.current(),
            )
        });
    }
    fn subscriptions(&self) -> Vec<Subscription<Message>> {
        let apps = self.adapters.apps.clone();
        let clock = self.adapters.clock.clone();
        let system = self.adapters.system.clone();
        let media = self.adapters.media.clone();
        let weather = self.adapters.weather.clone();
        let notifications = self.adapters.notifications.clone();
        let audio = self.adapters.audio.clone();
        let network = self.adapters.network.clone();
        let bluetooth = self.adapters.bluetooth.clone();
        let power = self.adapters.power.clone();
        let tray = self.adapters.tray.clone();
        let mut s = vec![
            Subscription::stream("applications", move |out, cancel| {
                apps.watch(
                    &mut |snapshot| {
                        out.send(Message::Catalog(crate::catalog::Message::Observed(
                            snapshot,
                        )))
                    },
                    &WatchStop(cancel),
                )
            }),
            Subscription::stream("tray", move |out, cancel| {
                tray.watch(
                    &mut |value| out.send(Message::Tray(crate::tray::Message::Observed(value))),
                    &WatchStop(cancel),
                )
            }),
            Subscription::stream("power", move |out, cancel| {
                power.watch(
                    &mut |value| out.send(Message::Power(crate::power::Message::Observed(value))),
                    &WatchStop(cancel),
                );
            }),
            Subscription::stream("bluetooth", move |out, cancel| {
                bluetooth.watch(
                    &mut |value| {
                        out.send(Message::Bluetooth(crate::bluetooth::Message::Observed(
                            value,
                        )))
                    },
                    &WatchStop(cancel),
                );
            }),
            Subscription::stream("network", move |out, cancel| {
                network.watch(
                    &mut |value| {
                        out.send(Message::Network(crate::network::Message::Observed(value)))
                    },
                    &WatchStop(cancel),
                );
            }),
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
        for subscription in self.network.subscriptions() {
            s.push(Subscription::stream(subscription.id, move |out, cancel| {
                (subscription.run)(
                    Emitter::new(move |message| out.send(Message::Network(message))),
                    cancel,
                );
            }));
        }
        for subscription in self.bluetooth.subscriptions() {
            s.push(Subscription::stream(subscription.id, move |out, cancel| {
                (subscription.run)(
                    Emitter::new(move |message| out.send(Message::Bluetooth(message))),
                    cancel,
                );
            }));
        }
        for subscription in self.dock.subscriptions() {
            s.push(Subscription::stream(subscription.id, move |out, cancel| {
                (subscription.run)(
                    Emitter::new(move |message| out.send(Message::Dock(message))),
                    cancel,
                )
            }));
        }
        for subscription in self.tray.subscriptions() {
            s.push(Subscription::stream(subscription.id, move |out, cancel| {
                (subscription.run)(
                    Emitter::new(move |message| out.send(Message::Tray(message))),
                    cancel,
                )
            }));
        }
        for subscription in self.power.subscriptions() {
            s.push(Subscription::stream(subscription.id, move |out, cancel| {
                (subscription.run)(
                    Emitter::new(move |message| out.send(Message::Power(message))),
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
        s.extend(
            self.live_wallpaper
                .subscriptions()
                .into_iter()
                .map(|subscription| subscription.map(Message::LiveWallpaper)),
        );
        s
    }
    fn event(&self, event: Event) -> Option<Message> {
        match event {
            Event::FrameSubmitted {
                surface: "live-wallpaper",
                now,
                ..
            } => Some(Message::LiveWallpaper(
                crate::live_wallpaper::Message::Frame(now),
            )),
            Event::Key {
                surface: "live-wallpaper",
                key: Key::Escape,
            } => Some(Message::LiveWallpaper(
                crate::live_wallpaper::Message::Close,
            )),
            Event::OutputsChanged(catalog) => Some(Message::Outputs(catalog)),
            Event::Scoped { instance, event } => {
                let route = self.monitors.route(&instance)?;
                let monitor = self.monitors.connected.get(&route.id)?;
                let sources = || {
                    monitor
                        .widgets
                        .surfaces(self.widget_layout(Some(&route.id)), monitor.viewport)
                };
                let message = match *event {
                    Event::PointerInside {
                        surface: "bar",
                        inside: true,
                    } => Some(Message::SelectMonitor(route.id.clone())),
                    Event::PointerButton {
                        surface,
                        button: MouseButton::Primary,
                        pressed: true,
                        ..
                    } if sources().iter().any(|source| source.id == surface) => {
                        Some(Message::Widgets(widgets::Message::Press))
                    }
                    Event::Key {
                        surface,
                        key: Key::Escape,
                    } if monitor.widgets.editing()
                        && sources().iter().any(|source| source.id == surface) =>
                    {
                        Some(Message::Widgets(widgets::Message::Edit(false)))
                    }
                    event => self.event(event),
                }?;
                Some(Message::OnMonitor(route, Box::new(message)))
            }
            Event::FrameSubmitted {
                surface: "notifications",
                now,
                ..
            } if self.notifications.settled(now) => Some(Message::Notifications(
                crate::notifications::Message::Settled,
            )),
            Event::Key {
                surface: "notifications",
                key: Key::Escape,
            } if self.notifications.history_open() => {
                Some(Message::Notifications(crate::notifications::Message::Hide))
            }
            Event::Key {
                surface: "dock",
                key:
                    Key::Shortcut {
                        character: 'p' | 'P',
                        modifiers,
                    },
            } if modifiers.control
                && !modifiers.alt
                && !modifiers.logo
                && self.launcher.open()
                && self.launcher.mode() == Mode::Apps =>
            {
                self.launcher
                    .selected_application(&self.apps)
                    .cloned()
                    .map(|id| Message::Dock(crate::dock::Message::Pin(id)))
            }
            Event::PointerInside {
                surface: "dock",
                inside,
            } => Some(Message::Dock(crate::dock::Message::Pointer(inside))),
            Event::PointerButton {
                surface: "dock",
                button: MouseButton::Primary,
                pressed: true,
                ..
            } => Some(Message::Dock(crate::dock::Message::Press)),
            Event::PointerButton {
                surface,
                button: MouseButton::Primary,
                pressed: true,
                ..
            } if self
                .widgets
                .surfaces(&self.settings.layout, self.viewport)
                .iter()
                .any(|source| source.id == surface) =>
            {
                Some(Message::Widgets(widgets::Message::Press))
            }
            Event::Key {
                surface,
                key: Key::Escape,
            } if self.widgets.editing()
                && self
                    .widgets
                    .surfaces(&self.settings.layout, self.viewport)
                    .iter()
                    .any(|source| source.id == surface) =>
            {
                Some(Message::Widgets(widgets::Message::Edit(false)))
            }
            Event::Key {
                surface: "tray",
                key: Key::Escape,
            } => Some(Message::Tray(self.tray.escape())),
            Event::Key {
                surface: "tray", ..
            } => None,
            Event::Key {
                surface: "power",
                key: Key::Escape,
            } => Some(Message::Power(self.power.escape())),
            Event::Key {
                surface: "power", ..
            } => None,
            Event::Key {
                surface: "bluetooth",
                key: Key::Escape,
            } => Some(Message::Bluetooth(self.bluetooth.escape())),
            Event::Key {
                surface: "bluetooth",
                ..
            } => None,
            Event::Key {
                surface: "network",
                key: Key::Escape,
            } => Some(Message::Network(self.network.escape())),
            Event::Key {
                surface: "network", ..
            } => None,
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
                density,
                ..
            } => Some(Message::Resize(surface, width, height, density)),
            event => self
                .launcher
                .event(event, self.browser.source.is_some())
                .map(Message::Launcher),
        }
    }
    fn command(&self, command: &str) -> std::result::Result<Option<Message>, String> {
        if command == "wallpaper live open" {
            return Ok(Some(Message::LiveWallpaper(
                crate::live_wallpaper::Message::Open,
            )));
        }
        if command == "wallpaper live pause" || command == "wallpaper live resume" {
            return Ok(Some(Message::LiveWallpaper(
                crate::live_wallpaper::Message::Pause(command.ends_with("pause")),
            )));
        }
        if command == "wallpaper live off" {
            return Ok(Some(Message::LiveWallpaper(
                crate::live_wallpaper::Message::Off,
            )));
        }
        if let Some(path) = command.strip_prefix("wallpaper live play ") {
            if !self.settings_writable {
                return Err("Settings are not ready for saving".into());
            }
            return Ok(Some(Message::LiveWallpaper(
                crate::live_wallpaper::Message::ImportPath(None, path.into()),
            )));
        }
        if let Some(value) = command.strip_prefix("workspace select ") {
            let id = if let Some(name) = value.strip_prefix("name:") {
                WorkspaceRef::named(name)
            } else {
                value
                    .parse::<i32>()
                    .map_err(|_| "Use workspace select NUMBER|name:NAME".to_string())
                    .and_then(|number| {
                        WorkspaceRef::numbered(number).map_err(|error| error.to_string())
                    })
                    .map_err(DomainError::Invalid)
            }
            .map_err(|error| error.to_string())?;
            return Ok(Some(Message::Workspace(WorkspaceCommand::Select(id))));
        }
        if let Some(name) = command.strip_prefix("workspace toggle-special ") {
            let name = if name == "default" { "" } else { name };
            let id = WorkspaceRef::special(name).map_err(|error| error.to_string())?;
            return Ok(Some(Message::Workspace(WorkspaceCommand::ToggleSpecial(
                id,
            ))));
        }
        if let Some(name) = command.strip_prefix("monitor select ") {
            let id = MonitorId::new(name).map_err(|error| error.to_string())?;
            if !self.monitors.connected.contains_key(&id) {
                return Err("Monitor is not uniquely available".into());
            }
            return Ok(Some(Message::SelectMonitor(id)));
        }
        if let Some(value) = command.strip_prefix("dock auto-hide ") {
            return match value {
                "on" => Ok(Some(Message::Dock(crate::dock::Message::AutoHide(true)))),
                "off" => Ok(Some(Message::Dock(crate::dock::Message::AutoHide(false)))),
                _ => Err("Use dock auto-hide on|off".into()),
            };
        }
        for (prefix, pinned) in [("dock pin ", true), ("dock unpin ", false)] {
            if let Some(id) = command.strip_prefix(prefix) {
                return AppId::new(id)
                    .map(|id| Some(Message::Dock(crate::dock::Message::SetPin(id, pinned))))
                    .map_err(|e| e.to_string());
            }
        }
        if let Some(value) = command.strip_prefix("dock move ") {
            let (id, before) = value
                .split_once(" before ")
                .map_or((value, None), |(id, before)| (id, Some(before)));
            let id = AppId::new(id).map_err(|e| e.to_string())?;
            let before = before
                .map(AppId::new)
                .transpose()
                .map_err(|e| e.to_string())?;
            return Ok(Some(Message::Dock(crate::dock::Message::Move(id, before))));
        }

        if command == "wallpaper retry" {
            return Ok(Some(Message::WallpaperSelection(
                crate::wallpaper_selection::Message::RetryPresentation,
            )));
        }
        if let Some(arguments) = command.strip_prefix("widgets size ") {
            let parts = arguments.split_whitespace().collect::<Vec<_>>();
            let usage =
                "Use: widgets size ID WIDTH HEIGHT (positive logical dimensions)".to_string();
            let [id, width, height] = parts.as_slice() else {
                return Err(usage);
            };
            if widgets::catalog::find(id).is_none() {
                return Err("Unknown widget".into());
            }
            if !self.settings_writable {
                return Err("Settings are not ready for saving".into());
            }
            let size = WidgetSize::new(
                width.parse().map_err(|_| usage.clone())?,
                height.parse().map_err(|_| usage.clone())?,
            )
            .map_err(|e| e.to_string())?;
            return Ok(Some(Message::Widgets(widgets::Message::Size(
                (*id).into(),
                size,
            ))));
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
                        crate::notifications::Message::DismissId(id),
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
            "tray open"=>Message::Tray(crate::tray::Message::Open),
            "tray toggle"=>Message::Tray(crate::tray::Message::Toggle),
            "tray close"=>Message::Tray(crate::tray::Message::Close),
            "power open"=>Message::Power(crate::power::Message::Open),
            "power toggle"=>Message::Power(crate::power::Message::Toggle),
            "power close"=>Message::Power(crate::power::Message::Close),
            "bluetooth open"=>Message::Bluetooth(crate::bluetooth::Message::Open),
            "bluetooth toggle"=>Message::Bluetooth(crate::bluetooth::Message::Toggle),
            "bluetooth close"=>Message::Bluetooth(crate::bluetooth::Message::Close),
            "audio open" => Message::Audio(crate::audio::Message::Open),
            "audio toggle" => Message::Audio(crate::audio::Message::Toggle),
            "audio close" => Message::Audio(crate::audio::Message::Close),
            "network open" => Message::Network(crate::network::Message::Open),
            "network toggle" => Message::Network(crate::network::Message::Toggle),
            "network close" => Message::Network(crate::network::Message::Close),
            "bar show" => Message::Bar(true),
            "bar hide" => Message::Bar(false),
            "osd close" => Message::OsdClose,
            "notifications dismiss-last" => Message::Notifications(crate::notifications::Message::DismissLast),
            "notifications dismiss-all" => Message::Notifications(crate::notifications::Message::DismissAll),
            "notifications invoke-last" => Message::Notifications(crate::notifications::Message::InvokeLast),
            "notifications retry-images" => Message::Notifications(crate::notifications::Message::RetryArtwork),
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
            "widgets edit on" => Message::Widgets(widgets::Message::Edit(true)),
            "widgets edit off" => Message::Widgets(widgets::Message::Edit(false)),
            "themes open" => Message::Launcher(launcher::Message::Mode(Mode::Themes)),
            "quit" => Message::Quit,
            _ => return Err(
                "Use launcher toggle|open|close|size, wallpapers open, widgets open, themes open, inspect, or quit"
                    .into(),
            ),
        }))
    }
    fn inspect(&self) -> String {
        let mut value = serde_json::json!({"desktop_viewport":self.viewport,"dock_surface_expanded":self.launcher.expanded(),"launcher_opening":self.launcher.opening(),"audio":self.audio.inspect(),"wallpaper_presentation":self.wallpaper.inspect_presentation(),"wallpaper_index":self.wallpaper_index, "wallpaper_count":self.wallpapers.len(), "prepared_assets": {
            "icons": crate::previews::usage(self.images.values()),
            "local_previews": self.previews.inspect(),
            "online_previews": crate::previews::usage(self.browser.previews.values()),
            "background": crate::previews::usage(self.wallpaper.background.iter()),
            "total": crate::previews::usage(self.images.values().chain(self.previews.images()).chain(self.browser.previews.values()).chain(self.wallpaper.background.iter())),
        }, "palette":lucent_design::selected_palette(&self.settings),"wallpaper_browser":{"source":self.browser.source,"query":self.browser.query,"page":self.browser.page,"count":self.browser.items.len(),"selected":self.browser.selected,"busy":self.browser.busy(),"status":self.browser.status},"theme": ThemeMode::from_light(self.settings.light).name(), "theme_error": self.theme_error, "bar_visible": self.bar_visible, "background_ready": self.wallpaper.background.is_some(), "wallpaper": self.wallpaper.current, "wallpaper_busy": self.wallpaper.busy(), "notifications_ready":self.notifications.ready,"notification_history_open":self.notifications.history_open(),"notification_count":self.notifications.snapshot.active.len(),"launcher":self.launcher.open(),"launcher_size":self.settings.launcher,"launcher_panel_size":[self.launcher.panel_width().target_value(),self.launcher.panel_height().target_value()],"mode":self.launcher.mode().name(),"query":self.launcher.query(),"selected":self.launcher.selected(),"result_count":self.launcher.result_count(),"applications":self.apps.len(),"mapped_icons":self.apps.iter().filter(|a|lucent_design::app_icons::lookup(a.id.as_str()).is_some()).count(),"workspaces":self.compositor.workspaces.iter().map(|w|serde_json::json!({"id":w.id.number(),"name":w.id.name(),"special":w.id.is_special(),"active":w.active})).collect::<Vec<_>>(),"widgets":self.active_widget_layout().visible_widgets,"positions":self.active_widget_layout().positions,"widget_grid":self.active_widgets().grid_visible(),"widget_surfaces":self.active_widgets().surfaces(self.active_widget_layout(),self.viewport).iter().map(|s|serde_json::json!({"id":s.id,"members":s.members,"x":s.rect.x,"y":s.rect.y,"width":s.rect.w,"height":s.rect.h})).collect::<Vec<_>>(),"notes":self.settings.notes,"timer_seconds":self.active_widgets().timer_remaining(),"error":self.error_message(),"settings_error":self.settings_error,"settings_save_status":self.persistence.status(self.settings_writable).name(),"settings_save_error":self.persistence.error(),"wallpaper_error":self.wallpaper.error,"settings_writable":self.settings_writable});
        value["live_wallpaper"] = self.live_wallpaper.inspect();
        value["notification_artwork"] = self.notifications.artwork_inspection();
        value["network"] = self.network.inspect();
        value["bluetooth"] = self.bluetooth.inspect();
        value["power"] = self.power.inspect();
        value["tray"] = self.tray.inspect();
        value["catalog"] = self.catalog.inspect();
        value["dock"] = self.dock.inspect();
        value["widget_sizes"] = serde_json::json!(self.active_widget_layout().widget_sizes);
        value["widgets_editing"] = serde_json::json!(self.active_widgets().editing());
        value["named_monitor_hosts"] = serde_json::json!(!self.monitors.legacy());
        value["compositor_available"] = serde_json::json!(self.compositor_available);
        value["compositor_error"] = serde_json::json!(self.compositor_error);
        value["compositor_focused_monitor"] =
            serde_json::json!(self.compositor.focused_monitor().map(MonitorId::as_str));
        value["pending_monitor_focus"] =
            serde_json::json!(self.monitors.pending_focus.as_ref().map(MonitorId::as_str));
        value["active_monitor"] =
            serde_json::json!(self.monitors.active.as_ref().map(MonitorId::as_str));
        value["monitor_layouts"] = serde_json::json!(self.settings.monitor_layouts);
        value["monitor_hosts"] = serde_json::json!(self.monitors.connected.iter().map(|(id,monitor)| serde_json::json!({"id":id.as_str(),"generation":monitor.generation,"viewport":monitor.viewport,"scale":monitor.scale,"density":monitor.density.factor(),"instance":monitor.instance.as_str(),"grid_visible":monitor.widgets.grid_visible(),"editing":monitor.widgets.editing(),"widget_surfaces":monitor.widgets.surfaces(self.widget_layout(Some(id)),monitor.viewport).iter().map(|source|serde_json::json!({"id":source.id,"members":source.members})).collect::<Vec<_>>()})).collect::<Vec<_>>());
        value.to_string()
    }
    fn animating(&self, surface: &str, now: f64) -> bool {
        (surface == "live-wallpaper" && self.live_wallpaper.motion.active(now))
            || (surface == "notifications" && self.notifications.animating(now))
            || (surface == "audio" && self.audio.animating(now))
            || (surface == "power" && self.power.animating(now))
            || (surface == "tray" && self.tray.animating(now))
            || (surface == "bluetooth" && self.bluetooth.animating(now))
            || (surface == "network" && self.network.animating(now))
            || (surface == "osd" && self.osd.animating(now))
            || (surface == "dock"
                && (self.launcher.animating(now)
                    || self.carousel.active(now)
                    || self.dock.animating(now)))
    }
}
