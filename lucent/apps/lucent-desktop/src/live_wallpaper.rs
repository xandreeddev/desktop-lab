//! Playback coordinator and bounded presentation handoff. The root owns saved
//! preferences; this child owns pending import, active producers and panel state.
use crate::ports::AssetPort;
use crate::presentation_frame::PresentationFrame;
use lucent_api::{
    Cancellation, Effects, ImageData, ImageStream, Subscription, TextEdit, TextFieldState,
};
use lucent_domain::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
mod frame_trace;
pub(crate) mod preview;
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
    SelectSource(usize),
    Browse(i32),
    ApplySelection,
    RetryPreview,
    Preview(preview::Message),
    PreviewViewport(f32, f32, lucent_api::Density),
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
    Frame(PresentationFrame),
    Error(String),
}
struct PreparedPoster {
    asset: String,
    revision: u64,
    image: Arc<ImageData>,
}
#[derive(Default)]
struct Mailbox {
    latest: Option<Delivery>,
    /// One worker-prepared same-asset RGBA poster, independent of replaceable
    /// video frames so latest-frame coalescing cannot discard the safe handoff.
    poster: Option<PreparedPoster>,
    notified: bool,
    source_frames: Option<frame_trace::SourceFrames>,
}
impl Mailbox {
    fn traced(enabled: bool) -> Self {
        Self {
            source_frames: enabled.then(frame_trace::SourceFrames::default),
            ..Default::default()
        }
    }
    fn publish(&mut self, item: Delivery) -> bool {
        self.latest = Some(item);
        self.notify()
    }
    fn publish_poster(&mut self, asset: &str, revision: u64, image: Arc<ImageData>) -> bool {
        self.poster = Some(PreparedPoster {
            asset: asset.into(),
            revision,
            image,
        });
        self.notify()
    }
    fn notify(&mut self) -> bool {
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
    frame: Option<PresentationFrame>,
    /// Only Image variants are stored here, prepared through AssetPort. Keep
    /// this same-asset static/mipped image while native frames may be retained.
    rgba_poster: Option<PresentationFrame>,
    running: bool,
    awaiting_frame: bool,
    native_planes: bool,
    error: String,
}
impl Player {
    fn presentation<'a>(&'a self, frame: &'a PresentationFrame) -> Option<&'a PresentationFrame> {
        if matches!(frame, PresentationFrame::Video { .. })
            && (!self.native_planes
                || self.target.is_none_or(|target| {
                    !target
                        .cover_size(frame.width(), frame.height())
                        .is_ok_and(|size| size == (frame.width(), frame.height()))
                }))
        {
            self.rgba_poster.as_ref()
        } else {
            Some(frame)
        }
    }
}
struct Route {
    key: String,
    pause: Option<WallpaperPauseReason>,
    /// A static GPU identity is essential when another output still updates
    /// the shared stream texture. Pixels remain shared immutable storage.
    held: Option<PresentationFrame>,
}
fn frozen_frame(frame: &PresentationFrame) -> PresentationFrame {
    frame.snapshot()
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
    pub selection: usize,
    pub carousel: lucent_api::Motion,
    pub preview: preview::Preview,
    preview_options: WallpaperPreviewOptions,
    awaiting_first_frame: Option<f64>,
    first_frame_submitted: bool,
    source_frame_profiling: bool,
    native_planes: bool,
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
            selection: 0,
            carousel: lucent_api::Motion::fixed(0.),
            preview: preview::Preview::new(adapter.clone()),
            preview_options: WallpaperPreviewOptions::new(
                WallpaperPreviewOptions::MAX_EDGE,
                WallpaperPreviewOptions::MAX_EDGE,
            )
            .expect("preview resource envelope is valid"),
            awaiting_first_frame: None,
            first_frame_submitted: false,
            source_frame_profiling: false,
            native_planes: false,
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
    /// Composition-root diagnostic opt-in; no preferences or playback change.
    pub fn enable_frame_tracing(&mut self, enabled: bool) {
        self.source_frame_profiling = enabled;
        for player in self.players.values() {
            player
                .mailbox
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .source_frames = enabled.then(frame_trace::SourceFrames::default);
        }
    }
    /// Default/legacy RGBA observation. Native presentation uses the typed
    /// presentation_source below, never an empty or fabricated RGBA buffer.
    #[cfg(test)]
    pub fn frame(&self, monitor: Option<&MonitorId>) -> Option<&Arc<ImageData>> {
        self.frame_source(monitor).map(|(_, frame)| frame)
    }
    #[cfg(test)]
    pub fn frame_source(&self, monitor: Option<&MonitorId>) -> Option<(&str, &Arc<ImageData>)> {
        self.presentation_source(monitor)
            .and_then(|(source, frame)| frame.as_image().map(|image| (source, image)))
    }
    pub fn presentation_source(
        &self,
        monitor: Option<&MonitorId>,
    ) -> Option<(&str, &PresentationFrame)> {
        let target = monitor
            .cloned()
            .map_or(LiveWallpaperTarget::Default, LiveWallpaperTarget::Monitor);
        let route = self.routes.get(&target)?;
        let player = self.players.get(&route.key)?;
        Some((
            player.asset.path.as_str(),
            player.presentation(
                route
                    .held
                    .as_ref()
                    .or(player.frame.as_ref())
                    .or(player.rgba_poster.as_ref())?,
            )?,
        ))
    }
    /// Current route admission for independently retained background layers.
    /// A missing/retired producer is unknown and must not authorize native paint.
    pub fn native_presentation_target(
        &self,
        monitor: Option<&MonitorId>,
    ) -> Option<WallpaperPixelTarget> {
        let target = monitor
            .cloned()
            .map_or(LiveWallpaperTarget::Default, LiveWallpaperTarget::Monitor);
        let route = self.routes.get(&target)?;
        let player = self.players.get(&route.key)?;
        player.native_planes.then_some(player.target).flatten()
    }
    /// Composition-root experimental policy. Mixed/shared/unknown outputs keep
    /// fitted RGBA; enable before the normal host reconciliation starts.
    pub fn enable_native_planes(&mut self, enabled: bool) {
        self.native_planes = enabled;
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
            (
                LiveWallpaperSettings,
                bool,
                Option<WallpaperPixelTarget>,
                usize,
            ),
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
                    .and_modify(|(_, running, target, count)| {
                        *count += 1;
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
                    .or_insert((setting.clone(), pause.is_none(), pixels, 1));
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
        let last_posters: BTreeMap<_, _> = self
            .players
            .values()
            .filter_map(|player| {
                player
                    .rgba_poster
                    .as_ref()
                    .map(|poster| (player.asset.path.clone(), poster.clone()))
            })
            .collect();
        self.players.retain(|key, _| wanted.contains_key(key));
        self.routes = routes;
        for (key, (setting, running, target, count)) in wanted.into_iter().take(4) {
            let native_planes =
                self.native_planes && outputs_known && count == 1 && target.is_some();
            if let Some(player) = self.players.get_mut(&key) {
                if player.running != running
                    || player.target != target
                    || player.native_planes != native_planes
                {
                    self.revision = self.revision.wrapping_add(1);
                    player.revision = self.revision;
                    player.running = running;
                    player.awaiting_frame = true;
                    player.target = target;
                    player.native_planes = native_planes;
                    player.error.clear();
                    player.mailbox =
                        Arc::new(Mutex::new(Mailbox::traced(self.source_frame_profiling)));
                }
            } else {
                self.revision = self.revision.wrapping_add(1);
                let frame = seeds
                    .get(&key)
                    .cloned()
                    .or_else(|| last_frames.get(&setting.asset.path).cloned())
                    .or_else(|| {
                        self.prepared
                            .get(&setting.asset.path)
                            .cloned()
                            .map(PresentationFrame::from)
                    });
                let rgba_poster = last_posters.get(&setting.asset.path).cloned().or_else(|| {
                    native_planes
                        .then(|| self.prepared.get(&setting.asset.path).cloned())
                        .flatten()
                        .map(PresentationFrame::from)
                });
                self.players.insert(
                    key,
                    Player {
                        revision: self.revision,
                        asset: setting.asset,
                        fps: setting.fps,
                        target,
                        mailbox: Arc::new(Mutex::new(Mailbox::traced(self.source_frame_profiling))),
                        frame,
                        rgba_poster,
                        running,
                        awaiting_frame: true,
                        native_planes,
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
        let mut subscriptions: Vec<_> = self
            .players
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
                let native_planes = player.native_planes;
                let prepared_poster = player
                    .rgba_poster
                    .as_ref()
                    .and_then(PresentationFrame::as_image)
                    .cloned();
                let source_frame_profiling = self.source_frame_profiling;
                Subscription::stream(format!("live-wallpaper:{revision}"), move |out, cancel| {
                    let stream = ImageStream::new();
                    let stop = Arc::new(PlaybackStopAdapter(cancel));
                    if stop.cancelled() {
                        return;
                    }
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
                            Ok(image) => Delivery::Frame(image.into()),
                            Err(error) => Delivery::Error(error.to_string()),
                        });
                        return;
                    }
                    // Native pixels are admitted only after this worker has a
                    // same-asset static RGBA fallback. If poster preparation
                    // fails, use the existing fitted RGBA decoder instead.
                    let native_poster = if native_planes {
                        match prepared_poster
                            .map(Ok)
                            .unwrap_or_else(|| assets.background(&asset.poster.path, None))
                        {
                            Ok(image) => {
                                if stop.cancelled() {
                                    return;
                                }
                                let notify = mailbox
                                    .lock()
                                    .unwrap_or_else(|p| p.into_inner())
                                    .publish_poster(&asset.path, revision, image.clone());
                                if notify {
                                    out.send(Message::Wake(key.clone(), revision));
                                }
                                Some(image)
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    };
                    let mut deliver = |result: Result<NativeMediaFrame>| {
                        if stop.cancelled() {
                            return false;
                        }
                        let delivery = match result.and_then(|frame| {
                            let timestamp = frame.timestamp();
                            let image: PresentationFrame = match frame {
                                NativeMediaFrame::Rgba(frame) => {
                                    let (w, h, bytes, aspect) =
                                        frame.into_shared_rgba_with_aspect();
                                    Arc::new(
                                        stream
                                            .frame_rgba_shared_with_aspect(w, h, bytes, aspect)
                                            .map_err(|e| DomainError::Failed(e.to_string()))?,
                                    )
                                    .into()
                                }
                                NativeMediaFrame::Yuv420 { frame, .. } => {
                                    PresentationFrame::native(
                                        Arc::new(
                                            stream
                                                .frame_yuv420(frame)
                                                .map_err(|e| DomainError::Failed(e.to_string()))?,
                                        ),
                                        native_poster.clone().ok_or_else(|| {
                                            DomainError::Failed(
                                                "Native delivery has no prepared RGBA handoff"
                                                    .into(),
                                            )
                                        })?,
                                    )
                                }
                            };
                            if source_frame_profiling
                                && let Some(trace) = mailbox
                                    .lock()
                                    .unwrap_or_else(|p| p.into_inner())
                                    .source_frames
                                    .as_mut()
                                && let Some(identity) = image.stream_revision()
                            {
                                trace.record(revision, identity, timestamp);
                            }
                            Ok(image)
                        }) {
                            Ok(frame) => Delivery::Frame(frame),
                            Err(error) => Delivery::Error(error.to_string()),
                        };
                        publish(delivery);
                        !stop.cancelled()
                    };
                    if native_poster.is_some() {
                        adapter.play_native(
                            &asset,
                            NativePlaybackOptions {
                                fps,
                                looping: true,
                                target,
                            },
                            &mut deliver,
                            stop.clone(),
                        );
                    } else {
                        adapter.play(
                            &asset,
                            PlaybackOptions {
                                fps,
                                looping: true,
                                target,
                            },
                            &mut |value| deliver(value.map(NativeMediaFrame::Rgba)),
                            stop.clone(),
                        );
                    }
                })
            })
            .collect();
        subscriptions.extend(
            self.preview
                .subscriptions()
                .into_iter()
                .map(|subscription| subscription.map(Message::Preview)),
        );
        subscriptions
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
                self.selection = self.selection.min(self.library.len().saturating_sub(1));
                self.carousel = lucent_api::Motion::fixed(self.selection as f32);
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
                        let selected_path =
                            self.selected_source().map(|source| source.path.clone());
                        library.truncate(32);
                        self.library = library;
                        self.selection = selected_path
                            .as_ref()
                            .and_then(|path| {
                                self.library.iter().position(|source| &source.path == path)
                            })
                            .or_else(|| {
                                self.library
                                    .iter()
                                    .position(|source| source.path == self.path)
                            })
                            .unwrap_or(0);
                        self.carousel = lucent_api::Motion::fixed(self.selection as f32);
                        self.sync_preview();
                        self.library_error.clear();
                    }
                    Err(error) => self.library_error = error.to_string(),
                }
            }
            Message::SelectSource(index) => {
                if !self.open || index >= self.library.len() {
                    return None;
                }
                self.selection = index;
                self.carousel.target(
                    index as f32,
                    effects.now,
                    lucent_design::motion::PANEL,
                    lucent_design::motion::SPATIAL,
                );
                self.sync_preview();
            }
            Message::Browse(delta) => {
                if !self.open || self.library.is_empty() {
                    return None;
                }
                let index = self
                    .selection
                    .saturating_add_signed(delta as isize)
                    .min(self.library.len() - 1);
                return self.update(Message::SelectSource(index), effects);
            }
            Message::ApplySelection => {
                if !self.open {
                    return None;
                }
                let source = self.selected_source()?;
                return self.update(
                    Message::ImportPath(self.target.clone(), source.path.clone()),
                    effects,
                );
            }
            Message::RetryPreview => {
                if !self.open || !self.preview.retry() {
                    return None;
                }
            }
            Message::Preview(message) => {
                if self.preview.update(message) {
                    effects.redraw("live-wallpaper");
                }
                return None;
            }
            Message::PreviewViewport(width, height, density) => {
                if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
                    return None;
                }
                let (width, height) = view::preview_size((width, height));
                let factor = f64::from(density.numerator()) / f64::from(density.denominator());
                let width = f64::from(width) * factor;
                let height = f64::from(height) * factor;
                if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
                    return None;
                }
                let limit = f64::from(WallpaperPreviewOptions::MAX_EDGE);
                let fit = (limit / width.max(height)).min(1.);
                let options = WallpaperPreviewOptions::new(
                    (width * fit).ceil().clamp(1., limit) as u32,
                    (height * fit).ceil().clamp(1., limit) as u32,
                )
                .ok()?;
                if self.preview_options == options {
                    return None;
                }
                self.preview_options = options;
                self.sync_preview();
                if !self.open {
                    return None;
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
                self.preview.clear();
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
                    let (poster, item) = {
                        let mut mailbox = player.mailbox.lock().unwrap_or_else(|p| p.into_inner());
                        (mailbox.poster.take(), mailbox.take())
                    };
                    let poster = poster.filter(|poster| {
                        poster.asset == player.asset.path && poster.revision == revision
                    });
                    let poster_changed = poster.is_some();
                    if let Some(poster) = poster {
                        player.rgba_poster = Some(poster.image.into());
                    }
                    match item {
                        Some(Delivery::Frame(image)) => {
                            if matches!(&image, PresentationFrame::Video { .. })
                                && player.rgba_poster.is_none()
                            {
                                player.awaiting_frame = false;
                                player.error = "Native frame has no prepared RGBA handoff".into();
                                return None;
                            }
                            if self.native_planes && player.awaiting_frame {
                                effects.redraw("live-wallpaper");
                            }
                            player.awaiting_frame = false;
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
                        Some(Delivery::Error(error)) => {
                            if self.native_planes {
                                effects.redraw("live-wallpaper");
                            }
                            player.awaiting_frame = false;
                            player.error = error;
                        }
                        None => {}
                    }
                    if poster_changed {
                        return Some(Intent::FrameChanged(
                            self.routes
                                .iter()
                                .filter(|(_, route)| route.key == key)
                                .map(|(target, _)| target.clone())
                                .collect(),
                        ));
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
                    player.awaiting_frame = true;
                    player.error.clear();
                    player.mailbox =
                        Arc::new(Mutex::new(Mailbox::traced(self.source_frame_profiling)));
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
                    player.awaiting_frame = false;
                    player.error = reason;
                }
            }
        }
        effects.redraw("live-wallpaper");
        None
    }
    pub fn selected_source(&self) -> Option<&Wallpaper> {
        self.library.get(self.selection)
    }
    fn sync_preview(&mut self) {
        if !self.open || self.library_loading {
            self.preview.clear();
            return;
        }
        let Some(source) = self.selected_source().cloned() else {
            self.preview.clear();
            return;
        };
        let neighbors: Vec<_> = self
            .selection
            .checked_sub(1)
            .into_iter()
            .chain(
                self.selection
                    .checked_add(1)
                    .filter(|index| *index < self.library.len()),
            )
            .filter_map(|index| self.library.get(index).cloned())
            .collect();
        self.preview
            .select(source, self.preview_options, &neighbors);
    }
    pub fn animating(&self, now: f64) -> bool {
        self.mounted && (self.motion.active(now) || self.carousel.active(now))
    }
    fn refresh_library(&mut self, effects: &mut Effects<Message>) {
        // Discovery is also explicit invalidation: a local file may have been
        // replaced without changing its path. Drop cached pixels/failures and
        // demand, but retain a retiring decoder until its explicit Ended ACK.
        self.preview.clear();
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
            return if self.native_planes && player.awaiting_frame && player.running {
                "Preparing live wallpaper…"
            } else if player.running {
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
        } else if self.native_planes && self.players.values().any(|p| p.running && p.awaiting_frame)
        {
            "Preparing live wallpaper…"
        } else if self.players.values().any(|p| p.running) {
            "Playing · bounded latest-frame delivery"
        } else if !self.players.is_empty() {
            "Paused"
        } else {
            "Choose a local MP4, WebM or GIF"
        }
    }
    pub fn inspect(&self) -> serde_json::Value {
        let frames: Vec<_> = self
            .players
            .values()
            .flat_map(|player| player.frame.iter().chain(player.rgba_poster.iter()))
            .chain(self.routes.values().filter_map(|route| route.held.as_ref()))
            .collect();
        let rgba = crate::previews::usage(frames.iter().filter_map(|frame| frame.as_image()));
        let mut plane_views = BTreeMap::new();
        for frame in &frames {
            if let PresentationFrame::Video { data, .. } = frame {
                for plane in data.frame().planes() {
                    plane_views.insert(
                        (plane.bytes().as_ptr() as usize, plane.bytes().len()),
                        plane.bytes().len(),
                    );
                }
            }
        }
        // Logical retained published views, not underlying AVBuffer capacity/RSS.
        let retained_bytes = rgba.bytes + plane_views.values().sum::<usize>();
        serde_json::json!({"open":self.open,"awaiting_first_frame":self.awaiting_first_frame.is_some(),"importing":self.importing.is_some(),"library_loading":self.library_loading,"library_count":self.library.len(),"library_error":self.library_error,"selection":self.selection,"preview":self.preview.inspect(),"players":self.players.len(),"running":self.players.values().filter(|p|p.running&&p.error.is_empty()).count(),"retained_frame_bytes":retained_bytes,"native_planes_opt_in":self.native_planes,"producers":self.players.values().map(|player| {
            let source_frames=player.mailbox.lock().unwrap_or_else(|p|p.into_inner()).source_frames.as_ref().map(frame_trace::SourceFrames::json);
            serde_json::json!({"revision":player.revision,"running":player.running,"native_requested":player.native_planes,"awaiting_frame":player.awaiting_frame,"representation":player.frame.as_ref().map(PresentationFrame::representation),"target":player.target.map(WallpaperPixelTarget::size),"frame_size":player.frame.as_ref().map(|frame|(frame.width(),frame.height())),"source_aspect":player.frame.as_ref().map(|frame|frame.aspect_size()),"source_frames":source_frames})
        }).collect::<Vec<_>>(),"error":self.status(),"outputs":self.routes.iter().map(|(target,route)| serde_json::json!({"monitor":target.monitor().map(MonitorId::as_str),"pause_reason":route.pause})).collect::<Vec<_>>()})
    }
}
#[cfg(test)]
mod tests;
