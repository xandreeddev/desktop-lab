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
    EditPath(TextEdit),
    Target(Option<MonitorId>),
    Import,
    ImportPath(Option<MonitorId>, String),
    Prepared(u64, Result<(LiveWallpaperAsset, Arc<ImageData>)>),
    Wake(String, u64),
    Pause(bool),
    Off,
    Rate(WallpaperFps),
    Scroll(lucent_api::ScrollEvent),
    Frame(f64),
}
pub enum Intent {
    Apply(Option<MonitorId>, LiveWallpaperSettings),
    Pause(bool),
    Off(Option<MonitorId>),
    Rate(Option<MonitorId>, WallpaperFps),
    FrameChanged(Vec<Option<MonitorId>>),
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
    fps: WallpaperFps,
    mailbox: Arc<Mutex<Mailbox>>,
    frame: Option<Arc<ImageData>>,
    running: bool,
    error: String,
}
struct Import {
    revision: u64,
    target: Option<MonitorId>,
}
pub struct LiveWallpaper {
    pub open: bool,
    pub path: String,
    pub field: TextFieldState,
    pub target: Option<MonitorId>,
    pub error: String,
    pub scroll: f32,
    pub mounted: bool,
    pub motion: lucent_api::Motion,
    importing: Option<Import>,
    revision: u64,
    players: BTreeMap<String, Player>,
    routes: BTreeMap<Option<MonitorId>, String>,
    adapter: Arc<dyn WallpaperPlaybackPort>,
    assets: Arc<dyn AssetPort>,
    import_cancel: Cancellation,
    prepared: BTreeMap<String, Arc<ImageData>>,
}
impl LiveWallpaper {
    pub fn new(adapter: Arc<dyn WallpaperPlaybackPort>, assets: Arc<dyn AssetPort>) -> Self {
        Self {
            open: false,
            path: String::new(),
            field: TextFieldState::default(),
            target: None,
            error: String::new(),
            scroll: 0.,
            mounted: false,
            motion: lucent_api::Motion::fixed(0.),
            importing: None,
            revision: 0,
            players: BTreeMap::new(),
            routes: BTreeMap::new(),
            adapter,
            assets,
            import_cancel: Cancellation::default(),
            prepared: BTreeMap::new(),
        }
    }
    pub fn frame(&self, monitor: Option<&MonitorId>) -> Option<&Arc<ImageData>> {
        self.routes
            .get(&monitor.cloned())
            .and_then(|key| self.players.get(key))
            .and_then(|p| p.frame.as_ref())
    }
    pub fn reconcile(
        &mut self,
        settings: &LiveWallpapers,
        monitors: &[MonitorId],
        outputs_known: bool,
        suspended: bool,
    ) {
        let targets: Vec<_> = if monitors.is_empty() && !outputs_known {
            vec![None]
        } else {
            monitors.iter().cloned().map(Some).collect()
        };
        let mut wanted = BTreeMap::new();
        let mut routes = BTreeMap::new();
        for target in targets {
            let selected = target
                .as_ref()
                .and_then(|id| settings.monitors.get(id))
                .or(settings.default.as_ref());
            if let Some(setting) = selected.filter(|s| s.enabled) {
                let key = format!(
                    "{}:{}:{}",
                    setting.asset.path,
                    setting.fps.value(),
                    setting.paused
                );
                routes.insert(target, key.clone());
                wanted.entry(key).or_insert((
                    setting.clone(),
                    !settings.paused && !setting.paused && !suspended,
                ));
            }
        }
        self.players.retain(|key, _| wanted.contains_key(key));
        self.routes = routes;
        for (key, (setting, running)) in wanted.into_iter().take(4) {
            if let Some(player) = self.players.get_mut(&key) {
                if player.running != running {
                    self.revision = self.revision.wrapping_add(1);
                    player.revision = self.revision;
                    player.running = running;
                    player.error.clear();
                    player.mailbox = Arc::new(Mutex::new(Mailbox::default()));
                }
            } else {
                self.revision = self.revision.wrapping_add(1);
                let frame = self.prepared.remove(&setting.asset.path);
                self.players.insert(
                    key,
                    Player {
                        revision: self.revision,
                        asset: setting.asset,
                        fps: setting.fps,
                        mailbox: Arc::new(Mutex::new(Mailbox::default())),
                        frame,
                        running,
                        error: String::new(),
                    },
                );
            }
        }
        self.prepared.clear();
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        self.players
            .iter()
            .filter(|(_, p)| p.running && p.error.is_empty())
            .map(|(key, player)| {
                let key = key.clone();
                let revision = player.revision;
                let mailbox = player.mailbox.clone();
                let adapter = self.adapter.clone();
                let asset = player.asset.clone();
                let fps = player.fps;
                Subscription::stream(format!("live-wallpaper:{revision}"), move |out, cancel| {
                    let stream = ImageStream::new();
                    let stop = Arc::new(PlaybackStopAdapter(cancel));
                    adapter.play(
                        &asset,
                        PlaybackOptions { fps, looping: true },
                        &mut |result| {
                            if stop.cancelled() {
                                return false;
                            }
                            let delivery = match result.and_then(|frame| {
                                let (w, h, bytes) = frame.into_rgba();
                                stream
                                    .frame_rgba(w, h, bytes)
                                    .map(Arc::new)
                                    .map_err(|e| DomainError::Failed(e.to_string()))
                            }) {
                                Ok(image) => Delivery::Frame(image),
                                Err(error) => Delivery::Error(error.to_string()),
                            };
                            let notify = mailbox
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .publish(delivery);
                            if notify {
                                out.send(Message::Wake(key.clone(), revision));
                            }
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
            Message::Open => {
                self.open = true;
                self.mounted = true;
                self.motion.target(
                    1.,
                    effects.now,
                    lucent_design::motion::PANEL,
                    lucent_design::motion::SPATIAL,
                );
                self.scroll = 0.;
            }
            Message::Frame(now) => {
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
                self.motion.target(
                    0.,
                    effects.now,
                    lucent_design::motion::PANEL,
                    lucent_design::motion::SPATIAL,
                );
                self.field.cancel();
                self.import_cancel.cancel();
                self.importing = None;
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
            Message::Target(target) => self.target = target,
            Message::Import => {
                return self.update(
                    Message::ImportPath(self.target.clone(), self.path.clone()),
                    effects,
                );
            }
            Message::ImportPath(target, path) => {
                if self.importing.is_some() {
                    self.error = "A media import is already running".into();
                } else {
                    self.revision = self.revision.wrapping_add(1);
                    let revision = self.revision;
                    self.importing = Some(Import { revision, target });
                    self.error.clear();
                    self.import_cancel = Cancellation::default();
                    let adapter = self.adapter.clone();
                    let assets = self.assets.clone();
                    effects.task_cancellable_on_with_failure(
                        crate::work::APPEARANCE,
                        self.import_cancel.clone(),
                        move || {
                            Message::Prepared(
                                revision,
                                (|| {
                                    let source = Wallpaper {
                                        path,
                                        name: "Live wallpaper".into(),
                                    };
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
                                fps: WallpaperFps::default(),
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
                    && player.running
                {
                    let item = player
                        .mailbox
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .take();
                    match item {
                        Some(Delivery::Frame(image)) => {
                            player.frame = Some(image);
                            return Some(Intent::FrameChanged(
                                self.routes
                                    .iter()
                                    .filter(|(_, route)| *route == &key)
                                    .map(|(target, _)| target.clone())
                                    .collect(),
                            ));
                        }
                        Some(Delivery::Error(error)) => player.error = error,
                        None => {}
                    }
                }
            }
            Message::Pause(value) => return Some(Intent::Pause(value)),
            Message::Off => return Some(Intent::Off(self.target.clone())),
            Message::Rate(rate) => return Some(Intent::Rate(self.target.clone(), rate)),
        }
        effects.redraw("live-wallpaper");
        None
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
        serde_json::json!({"open":self.open,"importing":self.importing.is_some(),"players":self.players.len(),"running":self.players.values().filter(|p|p.running&&p.error.is_empty()).count(),"retained_frame_bytes":self.players.values().filter_map(|p|p.frame.as_ref()).map(|f|f.bytes()).sum::<usize>(),"error":self.status()})
    }
}
#[cfg(test)]
mod tests;
