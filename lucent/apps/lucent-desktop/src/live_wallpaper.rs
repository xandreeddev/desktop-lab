//! Playback coordinator and bounded presentation handoff. The root owns saved
//! preferences; this child owns pending import, active producers and panel state.
use crate::ports::AssetPort;
use lucent_api::{
    Cancellation, Effects, ImageData, ImageStream, Subscription, TextEdit, TextFieldState,
};
use lucent_domain::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
mod view;
struct PlaybackStopAdapter(Cancellation);
impl StopSignal for PlaybackStopAdapter {
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
    fn wait(&self, duration: std::time::Duration) {
        self.0.sleep(duration);
    }
}
#[derive(Clone)]
pub enum Message {
    Open,
    Close,
    Ignore,
    RefreshLibrary,
    LibraryLoaded(u64, Result<Vec<Wallpaper>>),
    EditPath(TextEdit),
    Target(LiveWallpaperTarget),
    Import(LiveWallpaperTarget),
    ImportPath(LiveWallpaperTarget, String),
    Prepared(u64, Result<(LiveWallpaperAsset, Arc<ImageData>)>),
    Wake(String, u64),
    Pause(LiveWallpaperTarget, bool),
    PauseAll(bool),
    Policy(WallpaperPauseCondition, bool),
    Off(LiveWallpaperTarget),
    Rate(LiveWallpaperTarget, WallpaperFrameRate),
    Inherit(MonitorId),
    Retry(LiveWallpaperTarget),
    RetrySave,
    WorkerFailed(u64, String),
    Scroll(lucent_api::ScrollEvent),
    Frame(f64),
}
pub enum Intent {
    ImportStarted(LiveWallpaperTarget, u64),
    Apply(LiveWallpaperTarget, LiveWallpaperSettings),
    Pause(LiveWallpaperTarget, bool),
    PauseAll(bool),
    Policy(WallpaperPauseCondition, bool),
    Off(LiveWallpaperTarget),
    Rate(LiveWallpaperTarget, WallpaperFrameRate),
    Inherit(MonitorId),
    RetrySave,
    FrameChanged(Vec<LiveWallpaperTarget>),
}
#[derive(Clone, Copy)]
pub struct SaveState<'a> {
    pub status: crate::persistence::Status,
    pub error: &'a str,
    pub writable: bool,
}
enum Delivery {
    Frame(Arc<ImageData>),
    Error(String),
}
#[derive(Default)]
struct Mailbox {
    latest: Option<Delivery>,
    notified: bool,
}
impl Mailbox {
    fn publish(&mut self, item: Delivery) -> bool {
        self.latest = Some(item);
        if self.notified {
            false
        } else {
            self.notified = true;
            true
        }
    }
    fn take(&mut self) -> Option<Delivery> {
        self.notified = false;
        self.latest.take()
    }
}
struct Player {
    revision: u64,
    asset: LiveWallpaperAsset,
    fps: WallpaperFrameRate,
    target: Option<WallpaperPixelTarget>,
    mailbox: Arc<Mutex<Mailbox>>,
    frame: Option<Arc<ImageData>>,
    running: bool,
    error: String,
}
struct Route {
    key: String,
    pause: Option<WallpaperPauseReason>,
    /// A static GPU identity is essential when another output still updates
    /// the shared stream texture. Pixels remain shared immutable storage.
    held: Option<Arc<ImageData>>,
}
fn frozen_frame(frame: &Arc<ImageData>) -> Arc<ImageData> {
    if frame.stream_revision().is_some() {
        Arc::new(frame.snapshot())
    } else {
        frame.clone()
    }
}
struct Import {
    revision: u64,
    target: LiveWallpaperTarget,
}
pub struct LiveWallpaper {
    pub open: bool,
    pub path: String,
    pub field: TextFieldState,
    pub target: LiveWallpaperTarget,
    pub error: String,
    pub scroll: f32,
    pub mounted: bool,
    pub motion: lucent_api::Motion,
    awaiting_first_frame: Option<f64>,
    first_frame_submitted: bool,
    importing: Option<Import>,
    revision: u64,
    players: BTreeMap<String, Player>,
    routes: BTreeMap<LiveWallpaperTarget, Route>,
    default_key: Option<String>,
    adapter: Arc<dyn WallpaperPlaybackPort>,
    assets: Arc<dyn AssetPort>,
    import_cancel: Cancellation,
    library: Vec<Wallpaper>,
    library_loading: bool,
    library_error: String,
    library_revision: u64,
    library_cancel: Cancellation,
    prepared: BTreeMap<String, Arc<ImageData>>,
}
impl LiveWallpaper {
    pub fn new(adapter: Arc<dyn WallpaperPlaybackPort>, assets: Arc<dyn AssetPort>) -> Self {
        Self {
            open: false,
            path: String::new(),
            field: TextFieldState::default(),
            target: LiveWallpaperTarget::Default,
            error: String::new(),
            scroll: 0.,
            mounted: false,
            motion: lucent_api::Motion::fixed(0.),
            awaiting_first_frame: None,
            first_frame_submitted: false,
            importing: None,
            revision: 0,
            players: BTreeMap::new(),
            routes: BTreeMap::new(),
            default_key: None,
            adapter,
            assets,
            import_cancel: Cancellation::default(),
            library: vec![],
            library_loading: false,
            library_error: String::new(),
            library_revision: 0,
            library_cancel: Cancellation::default(),
            prepared: BTreeMap::new(),
        }
    }
    #[cfg(test)]
    pub fn frame(&self, monitor: Option<&MonitorId>) -> Option<&Arc<ImageData>> {
        self.frame_source(monitor).map(|(_, frame)| frame)
    }
    /// Selected asset identity paired with the actual prepared frame.
    pub fn frame_source(&self, monitor: Option<&MonitorId>) -> Option<(&str, &Arc<ImageData>)> {
        let target = monitor
            .cloned()
            .map_or(LiveWallpaperTarget::Default, LiveWallpaperTarget::Monitor);
        let route = self.routes.get(&target)?;
        let player = self.players.get(&route.key)?;
        Some((
            player.asset.path.as_str(),
            route.held.as_ref().or(player.frame.as_ref())?,
        ))
    }
    pub fn reconcile(
        &mut self,
        settings: &LiveWallpapers,
        monitors: &[MonitorId],
        outputs_known: bool,
        context: &WallpaperPlaybackContext,
        pixel_targets: &BTreeMap<LiveWallpaperTarget, WallpaperPixelTarget>,
    ) {
        let targets: Vec<_> = if monitors.is_empty() && !outputs_known {
            vec![LiveWallpaperTarget::Default]
        } else {
            monitors
                .iter()
                .cloned()
                .map(LiveWallpaperTarget::Monitor)
                .collect()
        };
        let mut wanted: BTreeMap<
            String,
            (LiveWallpaperSettings, bool, Option<WallpaperPixelTarget>),
        > = BTreeMap::new();
        let mut routes = BTreeMap::new();
        let mut seeds = BTreeMap::new();
        for target in targets {
            let selected = settings.selected(&target);
            if let Some(setting) = selected.filter(|s| s.enabled) {
                let pixels = pixel_targets.get(&target).copied();
                let key = format!("{}:{}:{}", setting.asset.path, setting.fps, setting.paused);
                let pause = lucent_usecases::live_wallpaper::wallpaper_pause_reason(
                    settings, setting, &target, context,
                );
                let previous = self.routes.get(&target).and_then(|route| {
                    let player = self.players.get(&route.key)?;
                    (player.asset.path == setting.asset.path)
                        .then(|| route.held.as_ref().or(player.frame.as_ref()))
                        .flatten()
                });
                if let Some(frame) = previous {
                    seeds.entry(key.clone()).or_insert_with(|| frame.clone());
                }
                let held = pause.and_then(|_| previous.map(frozen_frame));
                routes.insert(
                    target,
                    Route {
                        key: key.clone(),
                        pause,
                        held,
                    },
                );
                // One visible output is enough to keep a shared decoder alive.
                // Covered outputs retain their own frozen presentation frame.
                wanted
                    .entry(key)
                    .and_modify(|(_, running, target)| {
                        *running |= pause.is_none();
                        // Each connected consumer, including a paused output,
                        // must retain cover density. Unknown geometry keeps
                        // source pixels until that output configures.
                        *target = match (*target, pixels) {
                            (Some(left), Some(right)) => {
                                let (lw, lh) = left.size();
                                let (rw, rh) = right.size();
                                WallpaperPixelTarget::new(lw.max(rw), lh.max(rh)).ok()
                            }
                            _ => None,
                        };
                    })
                    .or_insert((setting.clone(), pause.is_none(), pixels));
            }
        }
        // A changed FPS/pause profile retires its producer identity, but keeps
        // the same source's last immutable image while its replacement starts.
        let last_frames: BTreeMap<_, _> = self
            .players
            .values()
            .filter_map(|player| {
                player
                    .frame
                    .as_ref()
                    .map(|frame| (player.asset.path.clone(), frame.clone()))
            })
            .collect();
        self.players.retain(|key, _| wanted.contains_key(key));
        self.routes = routes;
        for (key, (setting, running, target)) in wanted.into_iter().take(4) {
            if let Some(player) = self.players.get_mut(&key) {
                if player.running != running || player.target != target {
                    self.revision = self.revision.wrapping_add(1);
                    player.revision = self.revision;
                    player.running = running;
                    player.target = target;
                    player.error.clear();
                    player.mailbox = Arc::new(Mutex::new(Mailbox::default()));
                }
            } else {
                self.revision = self.revision.wrapping_add(1);
                let frame = seeds
                    .get(&key)
                    .cloned()
                    .or_else(|| last_frames.get(&setting.asset.path).cloned())
                    .or_else(|| self.prepared.get(&setting.asset.path).cloned());
                self.players.insert(
                    key,
                    Player {
                        revision: self.revision,
                        asset: setting.asset,
                        fps: setting.fps,
                        target,
                        mailbox: Arc::new(Mutex::new(Mailbox::default())),
                        frame,
                        running,
                        error: String::new(),
                    },
                );
            }
        }
        self.default_key = settings
            .default
            .as_ref()
            .filter(|profile| profile.enabled)
            .map(|profile| format!("{}:{}:{}", profile.asset.path, profile.fps, profile.paused));
        self.prepared.clear();
    }
    pub fn has_outputs(&self) -> bool {
        !self.routes.is_empty()
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        self.players
            .iter()
            .filter(|(_, p)| p.error.is_empty() && (p.running || p.frame.is_none()))
            .map(|(key, player)| {
                let key = key.clone();
                let revision = player.revision;
                let mailbox = player.mailbox.clone();
                let adapter = self.adapter.clone();
                let asset = player.asset.clone();
                let assets = self.assets.clone();
                let fps = player.fps;
                let target = player.target;
                let running = player.running;
                Subscription::stream(format!("live-wallpaper:{revision}"), move |out, cancel| {
                    let stream = ImageStream::new();
                    let stop = Arc::new(PlaybackStopAdapter(cancel));
                    let publish = |delivery| {
                        if stop.cancelled() {
                            return;
                        }
                        let notify = mailbox
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .publish(delivery);
                        if notify {
                            out.send(Message::Wake(key.clone(), revision));
                        }
                    };
                    if !running {
                        // A restored paused profile still needs its own poster.
                        // Delivery remains bounded and never carries pixels in
                        // a runtime message. This finite worker retires on Wake.
                        publish(match assets.background(&asset.poster.path, None) {
                            Ok(image) => Delivery::Frame(image),
                            Err(error) => Delivery::Error(error.to_string()),
                        });
                        return;
                    }
                    adapter.play(
                        &asset,
                        PlaybackOptions {
                            fps,
                            looping: true,
                            target,
                        },
                        &mut |result| {
                            if stop.cancelled() {
                                return false;
                            }
                            let delivery = match result.and_then(|frame| {
                                let (w, h, bytes, aspect) = frame.into_rgba_with_aspect();
                                stream
                                    .frame_rgba_with_aspect(w, h, bytes, aspect)
                                    .map(Arc::new)
                                    .map_err(|e| DomainError::Failed(e.to_string()))
                            }) {
                                Ok(image) => Delivery::Frame(image),
                                Err(error) => Delivery::Error(error.to_string()),
                            };
                            publish(delivery);
                            !stop.cancelled()
                        },
                        stop.clone(),
                    );
                })
            })
            .collect()
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) -> Option<Intent> {
        match message {
            Message::Ignore => return None,
            Message::Open => {
                if self.open {
                    return None;
                }
                self.open = true;
                if self.mounted && self.first_frame_submitted {
                    // A closing surface is already submitted. Reverse its
                    // current visible progress without resetting to zero.
                    self.motion.target(
                        1.,
                        effects.now,
                        lucent_design::motion::WALLPAPER_REVEAL,
                        lucent_design::motion::CUT,
                    );
                } else {
                    // Rendering or waiting for the first compositor callback
                    // must not consume the selector's reveal duration.
                    self.motion = lucent_api::Motion::fixed(0.);
                    self.awaiting_first_frame = Some(effects.now);
                    self.first_frame_submitted = false;
                }
                self.mounted = true;
                self.scroll = 0.;
                self.refresh_library(effects);
            }
            Message::RefreshLibrary => {
                if self.open {
                    self.refresh_library(effects);
                }
            }
            Message::LibraryLoaded(revision, result) => {
                if !self.open || !self.library_loading || revision != self.library_revision {
                    return None;
                }
                self.library_loading = false;
                match result {
                    Ok(mut library) => {
                        library.truncate(32);
                        self.library = library;
                        self.library_error.clear();
                    }
                    Err(error) => self.library_error = error.to_string(),
                }
            }
            Message::Frame(now) => {
                if let Some(opened_at) = self.awaiting_first_frame {
                    if now < opened_at {
                        return None;
                    }
                    self.awaiting_first_frame = None;
                    self.first_frame_submitted = true;
                    self.motion.target(
                        1.,
                        effects.now.max(now),
                        lucent_design::motion::WALLPAPER_REVEAL,
                        lucent_design::motion::CUT,
                    );
                    effects.redraw("live-wallpaper");
                    return None;
                }
                if !self.open && self.mounted && !self.motion.active(now) {
                    self.mounted = false;
                    effects.redraw("live-wallpaper");
                }
                return None;
            }
            Message::Scroll(event) => {
                if event.offset.is_finite() && event.offset >= 0. {
                    self.scroll = event.offset;
                }
            }
            Message::Close => {
                self.open = false;
                self.awaiting_first_frame = None;
                self.motion.target(
                    0.,
                    effects.now,
                    lucent_design::motion::WALLPAPER_REVEAL,
                    lucent_design::motion::CUT,
                );
                self.field.cancel();
                self.retire_import();
                self.library_cancel.cancel();
                self.library_loading = false;
            }
            Message::EditPath(edit) => {
                if let Err(error) = self.field.update(
                    &mut self.path,
                    edit,
                    4096,
                    lucent_api::ClipboardTarget::new("live-wallpaper", "live-path"),
                    effects,
                    Message::EditPath,
                ) {
                    self.error = error.to_string();
                }
            }
            Message::Target(target) => {
                self.target = target;
                self.error.clear();
            }
            Message::Import(target) => {
                return self.update(Message::ImportPath(target, self.path.clone()), effects);
            }
            Message::ImportPath(target, path) => {
                if self.importing.is_some() {
                    return None;
                } else {
                    self.revision = self.revision.wrapping_add(1);
                    let revision = self.revision;
                    self.importing = Some(Import {
                        revision,
                        target: target.clone(),
                    });
                    self.error.clear();
                    self.import_cancel = Cancellation::default();
                    let adapter = self.adapter.clone();
                    let assets = self.assets.clone();
                    let name = self
                        .library
                        .iter()
                        .find(|source| source.path == path)
                        .map_or_else(|| "Live wallpaper".into(), |source| source.name.clone());
                    effects.task_cancellable_on_with_failure(
                        crate::work::APPEARANCE,
                        self.import_cancel.clone(),
                        move || {
                            Message::Prepared(
                                revision,
                                (|| {
                                    let source = Wallpaper { path, name };
                                    let asset = adapter.prepare(&source)?;
                                    let preview = assets.background(&asset.poster.path, None)?;
                                    Ok((asset, preview))
                                })(),
                            )
                        },
                        move |failure| {
                            Message::Prepared(
                                revision,
                                Err(DomainError::Failed(failure.to_string())),
                            )
                        },
                    );
                    effects.redraw("live-wallpaper");
                    return Some(Intent::ImportStarted(target, revision));
                }
            }
            Message::Prepared(revision, result) => {
                if self
                    .importing
                    .as_ref()
                    .is_none_or(|job| job.revision != revision)
                {
                    return None;
                }
                let target = self.importing.take().unwrap().target;
                match result {
                    Ok((asset, poster)) => {
                        self.prepared.insert(asset.path.clone(), poster);
                        self.error.clear();
                        effects.redraw("background");
                        return Some(Intent::Apply(
                            target,
                            LiveWallpaperSettings {
                                asset,
                                fps: WallpaperFrameRate::default(),
                                paused: false,
                                enabled: true,
                            },
                        ));
                    }
                    Err(error) => self.error = error.to_string(),
                }
            }
            Message::Wake(key, revision) => {
                if let Some(player) = self.players.get_mut(&key)
                    && player.revision == revision
                {
                    let item = player
                        .mailbox
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .take();
                    match item {
                        Some(Delivery::Frame(image)) => {
                            player.frame = Some(image.clone());
                            return Some(Intent::FrameChanged(
                                self.routes
                                    .iter_mut()
                                    .filter(|(_, route)| route.key == key)
                                    .filter_map(|(target, route)| {
                                        if route.pause.is_none() {
                                            Some(target.clone())
                                        } else if route.held.is_none() {
                                            route.held = Some(frozen_frame(&image));
                                            Some(target.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect(),
                            ));
                        }
                        Some(Delivery::Error(error)) => player.error = error,
                        None => {}
                    }
                }
            }
            Message::Pause(target, value) => {
                self.error.clear();
                return Some(Intent::Pause(target, value));
            }
            Message::PauseAll(value) => {
                self.error.clear();
                return Some(Intent::PauseAll(value));
            }
            Message::Policy(condition, value) => {
                self.error.clear();
                return Some(Intent::Policy(condition, value));
            }
            Message::Off(target) => {
                self.cancel_import(&target);
                self.error.clear();
                return Some(Intent::Off(target));
            }
            Message::Rate(target, rate) => {
                self.error.clear();
                return Some(Intent::Rate(target, rate));
            }
            Message::Inherit(id) => {
                self.cancel_import(&LiveWallpaperTarget::Monitor(id.clone()));
                self.error.clear();
                return Some(Intent::Inherit(id));
            }
            Message::Retry(target) => {
                if let Some(key) = self.route_key(&target)
                    && let Some(player) = self.players.get_mut(&key)
                    && !player.error.is_empty()
                {
                    self.revision = self.revision.wrapping_add(1);
                    player.revision = self.revision;
                    player.error.clear();
                    player.mailbox = Arc::new(Mutex::new(Mailbox::default()));
                }
                self.error.clear();
            }
            Message::RetrySave => return Some(Intent::RetrySave),
            Message::WorkerFailed(revision, reason) => {
                if let Some(player) = self
                    .players
                    .values_mut()
                    .find(|player| player.revision == revision)
                {
                    player.error = reason;
                }
            }
        }
        effects.redraw("live-wallpaper");
        None
    }
    fn refresh_library(&mut self, effects: &mut Effects<Message>) {
        self.library_cancel.cancel();
        let Some(revision) = self.library_revision.checked_add(1) else {
            self.library_loading = false;
            self.library_error = "Live wallpaper library request limit reached".into();
            return;
        };
        self.library_revision = revision;
        self.library_loading = true;
        self.library_error.clear();
        self.library_cancel = Cancellation::default();
        let adapter = self.adapter.clone();
        effects.task_cancellable_on_with_failure(
            crate::work::APPEARANCE,
            self.library_cancel.clone(),
            move || Message::LibraryLoaded(revision, adapter.library()),
            move |failure| {
                Message::LibraryLoaded(revision, Err(DomainError::Failed(failure.to_string())))
            },
        );
    }
    fn retire_import(&mut self) {
        self.import_cancel.cancel();
        self.importing = None;
    }
    pub fn cancel_import(&mut self, target: &LiveWallpaperTarget) {
        if self
            .importing
            .as_ref()
            .is_some_and(|job| &job.target == target)
        {
            self.retire_import();
        }
    }
    pub fn importing_target(&self) -> Option<&LiveWallpaperTarget> {
        self.importing.as_ref().map(|job| &job.target)
    }
    fn route_key(&self, target: &LiveWallpaperTarget) -> Option<String> {
        match target {
            LiveWallpaperTarget::Default => self.default_key.clone(),
            LiveWallpaperTarget::Monitor(_) => {
                self.routes.get(target).map(|route| route.key.clone())
            }
        }
    }
    fn selected_player(
        &self,
        settings: &LiveWallpapers,
        target: &LiveWallpaperTarget,
    ) -> Option<&Player> {
        if matches!(target, LiveWallpaperTarget::Monitor(_)) && !self.routes.contains_key(target) {
            return None;
        }
        let selected = settings.selected(target)?;
        let key = format!(
            "{}:{}:{}",
            selected.asset.path, selected.fps, selected.paused
        );
        self.players.get(&key)
    }
    pub fn retry_available(&self, settings: &LiveWallpapers, target: &LiveWallpaperTarget) -> bool {
        self.selected_player(settings, target)
            .is_some_and(|player| !player.error.is_empty())
    }
    pub fn selected_status<'a>(
        &'a self,
        settings: &LiveWallpapers,
        target: &LiveWallpaperTarget,
    ) -> &'a str {
        if self
            .importing
            .as_ref()
            .is_some_and(|job| &job.target == target)
        {
            return "Importing and decoding…";
        }
        if &self.target == target && !self.error.is_empty() {
            return &self.error;
        }
        let Some(selected) = settings.selected(target) else {
            return "Choose a local MP4, WebM or GIF";
        };
        if !selected.enabled {
            return "Using the static wallpaper";
        }
        if let Some(player) = self.selected_player(settings, target) {
            if !player.error.is_empty() {
                return &player.error;
            }
            if settings.paused {
                return "All live wallpapers are paused";
            }
            if selected.paused {
                return "Paused";
            }
            let pause = self
                .routes
                .get(target)
                .and_then(|route| route.pause)
                .or_else(|| {
                    if matches!(target, LiveWallpaperTarget::Default) && !player.running {
                        self.routes
                            .values()
                            .find(|route| self.default_key.as_ref() == Some(&route.key))
                            .and_then(|route| route.pause)
                    } else {
                        None
                    }
                });
            if let Some(reason) = pause {
                return match reason {
                    WallpaperPauseReason::All => "All live wallpapers are paused",
                    WallpaperPauseReason::Profile => "Paused",
                    WallpaperPauseReason::Sleep => "Paused for system sleep",
                    WallpaperPauseReason::SessionLock => "Paused while the session is locked",
                    WallpaperPauseReason::Battery => "Paused on battery power",
                    WallpaperPauseReason::Fullscreen => "Paused behind a fullscreen app",
                };
            }
            return if player.running {
                "Playing"
            } else {
                "Suspended"
            };
        }
        match target {
            LiveWallpaperTarget::Default => "No connected output uses this profile",
            LiveWallpaperTarget::Monitor(_) => "Monitor disconnected",
        }
    }
    pub fn status(&self) -> &str {
        if self.importing.is_some() {
            "Importing and decoding…"
        } else if !self.error.is_empty() {
            &self.error
        } else if let Some(player) = self.players.values().find(|p| !p.error.is_empty()) {
            &player.error
        } else if self.players.values().any(|p| p.running) {
            "Playing · bounded latest-frame delivery"
        } else if !self.players.is_empty() {
            "Paused"
        } else {
            "Choose a local MP4, WebM or GIF"
        }
    }
    pub fn inspect(&self) -> serde_json::Value {
        let retained = crate::previews::usage(
            self.players
                .values()
                .filter_map(|player| player.frame.as_ref())
                .chain(self.routes.values().filter_map(|route| route.held.as_ref())),
        );
        serde_json::json!({"open":self.open,"awaiting_first_frame":self.awaiting_first_frame.is_some(),"importing":self.importing.is_some(),"library_loading":self.library_loading,"library_count":self.library.len(),"library_error":self.library_error,"players":self.players.len(),"running":self.players.values().filter(|p|p.running&&p.error.is_empty()).count(),"retained_frame_bytes":retained.bytes,"producers":self.players.values().map(|player|serde_json::json!({"revision":player.revision,"running":player.running,"target":player.target.map(WallpaperPixelTarget::size),"frame_size":player.frame.as_ref().map(|frame|(frame.width(),frame.height())),"source_aspect":player.frame.as_ref().map(|frame|frame.aspect_size())})).collect::<Vec<_>>(),"error":self.status(),"outputs":self.routes.iter().map(|(target,route)| serde_json::json!({"monitor":target.monitor().map(MonitorId::as_str),"pause_reason":route.pause})).collect::<Vec<_>>()})
    }
}
#[cfg(test)]
mod tests;
