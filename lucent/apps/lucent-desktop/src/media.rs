//! Selected-player presentation. Native observation and commands stay behind MediaPort.
use crate::ports::AssetPort;
use lucent_api::{Cancellation, Effects, ImageData, Motion, SliderEvent, Subscription};
use lucent_design::motion;
use lucent_domain::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
#[cfg(test)]
mod tests;
mod view;

struct WatchStop(Cancellation);
impl StopSignal for WatchStop {
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
    fn wait(&self, duration: Duration) {
        self.0.sleep(duration);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeekTarget {
    player: MediaPlayerId,
    track: MediaTrackId,
    epoch: u64,
}
#[cfg(test)]
pub fn fixture() -> MediaSnapshot {
    MediaSnapshot {
        operation: None,
        players: vec![MediaPlayer {
            id: MediaPlayerId::new(1).unwrap(),
            name: "Fixture player".into(),
            desktop_entry: None,
            state: MediaPlayerState::Ready(MediaPlayback {
                status: PlaybackStatus::Playing,
                capabilities: MediaCapabilities {
                    can_control: true,
                    can_play: true,
                    can_pause: true,
                    can_next: true,
                    can_previous: true,
                    can_seek: true,
                    can_raise: true,
                },
                track: Some(MediaTrack {
                    id: MediaTrackId::new(1).unwrap(),
                    title: "A song to keep nearby".into(),
                    artists: vec!["Fixture artist".into()],
                    album: "Fixture album".into(),
                    length: Some(MediaTime::new(180_000_000).unwrap()),
                    artwork: None,
                }),
                position: Some(MediaPositionSample {
                    position: MediaTime::new(30_000_000).unwrap(),
                    sampled_at: Instant::now(),
                    rate: MediaRate::new(1.).unwrap(),
                }),
            }),
        }],
    }
}
#[derive(Clone)]
pub enum Message {
    Observed(Result<MediaSnapshot>),
    Open,
    Toggle,
    Close,
    Closed(u64),
    Select(MediaPlayerId),
    Request(MediaCommand),
    Admitted(u64, Result<MediaOperationId>),
    Seek(SeekTarget, SliderEvent),
    CancelSeek,
    Scroll(lucent_api::ScrollEvent),
    Tick(Instant),
    Artwork(u64, Result<Arc<ImageData>>),
    RetryArtwork,
    Ignore,
}
struct ArtworkDemand {
    revision: u64,
    source: MediaArtworkSource,
}
struct PendingCommand {
    revision: u64,
    command: MediaCommand,
    operation: Option<MediaOperationId>,
}
pub struct Media {
    snapshot: MediaSnapshot,
    ready: bool,
    observed: bool,
    observation_error: String,
    selected: Option<MediaPlayerId>,
    explicit_selection: bool,
    pub(super) open: bool,
    mounted: bool,
    pub(super) motion: Motion,
    close_generation: u64,
    seek_epoch: u64,
    seek_preview: Option<(SeekTarget, f32)>,
    now: Instant,
    progress_visible: bool,
    scroll: f32,
    command_revision: u64,
    pending: Option<PendingCommand>,
    command_cancel: Cancellation,
    error: String,
    adapter: Arc<dyn MediaPort>,
    assets: Arc<dyn AssetPort>,
    artwork_source: Option<MediaArtworkSource>,
    artwork_target: Option<(MediaPlayerId, MediaTrackId)>,
    artwork: Option<Arc<ImageData>>,
    artwork_error: String,
    artwork_revision: u64,
    artwork_active: Option<u64>,
    artwork_pending: Option<ArtworkDemand>,
}
impl Media {
    pub fn new(adapter: Arc<dyn MediaPort>, assets: Arc<dyn AssetPort>) -> Self {
        Self {
            snapshot: MediaSnapshot::default(),
            ready: false,
            observed: false,
            observation_error: String::new(),
            selected: None,
            explicit_selection: false,
            open: false,
            mounted: false,
            motion: Motion::fixed(0.),
            close_generation: 0,
            seek_epoch: 0,
            seek_preview: None,
            now: Instant::now(),
            progress_visible: false,
            scroll: 0.,
            command_revision: 0,
            pending: None,
            command_cancel: Cancellation::default(),
            error: String::new(),
            adapter,
            assets,
            artwork_source: None,
            artwork_target: None,
            artwork: None,
            artwork_error: String::new(),
            artwork_revision: 0,
            artwork_active: None,
            artwork_pending: None,
        }
    }
    pub fn open(&self) -> bool {
        self.open
    }
    pub fn visible(&self) -> bool {
        self.mounted
    }
    pub fn animating(&self, now: f64) -> bool {
        self.mounted && self.motion.active(now)
    }
    pub fn selected(&self) -> Option<&MediaPlayer> {
        self.selected
            .and_then(|id| self.snapshot.players.iter().find(|player| player.id == id))
    }
    pub fn playback(&self) -> Option<&MediaPlayback> {
        match &self.selected()?.state {
            MediaPlayerState::Ready(value) => Some(value),
            _ => None,
        }
    }
    pub fn label(&self) -> &str {
        if !self.observed {
            "Connecting to players…"
        } else if !self.ready {
            "Media unavailable"
        } else if self.snapshot.players.is_empty() {
            "No media players"
        } else if self
            .selected()
            .is_some_and(|player| matches!(player.state, MediaPlayerState::Unavailable(_)))
        {
            "Player unavailable"
        } else if let Some(track) = self.playback().and_then(|value| value.track.as_ref()) {
            if track.title.is_empty() {
                "Untitled track"
            } else {
                &track.title
            }
        } else {
            self.selected()
                .map_or("No media selected", |player| player.name.as_str())
        }
    }
    pub fn artist(&self) -> String {
        self.playback()
            .and_then(|value| value.track.as_ref())
            .map_or_else(String::new, |track| track.artists.join(" · "))
    }
    pub fn toggle_command(&self) -> Option<MediaCommand> {
        let player = self.selected()?;
        let command = if self.playback()?.status == PlaybackStatus::Playing {
            MediaCommand::Pause(player.id)
        } else {
            MediaCommand::Play(player.id)
        };
        self.admits(&command).then_some(command)
    }
    fn admits(&self, command: &MediaCommand) -> bool {
        self.ready
            && lucent_usecases::media::validate_media_command(&self.snapshot, command).is_ok()
    }
    pub fn command_available(&self, command: &MediaCommand) -> bool {
        self.admits(command) && self.pending.is_none()
    }
    pub fn command(&self, input: &str) -> std::result::Result<Message, String> {
        let parts: Vec<_> = input.split_whitespace().collect();
        let invalid = || {
            "Use media open/close/select ID, play/pause/next/previous/stop/raise ID, or seek PLAYER TRACK MICROSECONDS".to_string()
        };
        match parts.as_slice() {
            ["open"] => Ok(Message::Open),
            ["close"] => Ok(Message::Close),
            ["toggle"] => Ok(Message::Toggle),
            [method @ ("play-pause" | "next" | "previous")] => {
                let id = self
                    .selected()
                    .map(|player| player.id)
                    .ok_or_else(|| "Choose a current media player".to_string())?;
                let command = match *method {
                    "play-pause" => self
                        .toggle_command()
                        .ok_or_else(|| "Media playback control is unavailable".to_string())?,
                    "next" => MediaCommand::Next(id),
                    _ => MediaCommand::Previous(id),
                };
                if !self.admits(&command) {
                    return Err("Media control is no longer available".into());
                }
                Ok(Message::Request(command))
            }
            ["select", id] => {
                let id = id
                    .parse()
                    .ok()
                    .and_then(|id| MediaPlayerId::new(id).ok())
                    .ok_or_else(invalid)?;
                if !self.ready || !self.snapshot.players.iter().any(|player| player.id == id) {
                    return Err("Choose a current media player".into());
                }
                Ok(Message::Select(id))
            }
            [method, id] => {
                let id = id
                    .parse()
                    .ok()
                    .and_then(|id| MediaPlayerId::new(id).ok())
                    .ok_or_else(invalid)?;
                let command = match *method {
                    "play" => MediaCommand::Play(id),
                    "pause" => MediaCommand::Pause(id),
                    "next" => MediaCommand::Next(id),
                    "previous" => MediaCommand::Previous(id),
                    "stop" => MediaCommand::Stop(id),
                    "raise" => MediaCommand::Raise(id),
                    _ => return Err(invalid()),
                };
                if !self.admits(&command) {
                    return Err("Media control is no longer available".into());
                }
                Ok(Message::Request(command))
            }
            ["seek", player, track, position] => {
                let player = player
                    .parse()
                    .ok()
                    .and_then(|value| MediaPlayerId::new(value).ok())
                    .ok_or_else(invalid)?;
                let track = track
                    .parse()
                    .ok()
                    .and_then(|value| MediaTrackId::new(value).ok())
                    .ok_or_else(invalid)?;
                let position = position
                    .parse()
                    .ok()
                    .and_then(|value| MediaTime::new(value).ok())
                    .ok_or_else(invalid)?;
                let command = MediaCommand::SetPosition {
                    player,
                    track,
                    position,
                };
                if !self.admits(&command) {
                    return Err("Media seek target is no longer available".into());
                }
                Ok(Message::Request(command))
            }
            _ => Err(invalid()),
        }
    }
    fn seek_target(&self) -> Option<SeekTarget> {
        let playback = self.playback()?;
        let track = playback.track.as_ref()?;
        let target = SeekTarget {
            player: self.selected()?.id,
            track: track.id,
            epoch: self.seek_epoch,
        };
        let command = MediaCommand::SetPosition {
            player: target.player,
            track: target.track,
            position: MediaTime::new(0).ok()?,
        };
        (track.length.is_some_and(|length| length.micros() > 0) && self.admits(&command))
            .then_some(target)
    }
    fn cancel_seek(&mut self) {
        self.seek_epoch = self.seek_epoch.wrapping_add(1);
        self.seek_preview = None;
    }
    pub fn progress(&self) -> Option<(f32, MediaTime, MediaTime)> {
        let playback = self.playback()?;
        let length = playback.track.as_ref()?.length?;
        let position = lucent_usecases::media::project_media_position(playback, self.now)?;
        let fraction = self.seek_preview.map_or_else(
            || (position.micros() as f64 / length.micros().max(1) as f64) as f32,
            |(_, fraction)| fraction,
        );
        Some((fraction.clamp(0., 1.), position, length))
    }
    pub fn artwork(&self) -> Option<&Arc<ImageData>> {
        self.artwork.as_ref()
    }
    pub fn set_progress_visible(&mut self, visible: bool) {
        self.progress_visible = visible;
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Observed(result) => {
                let previous = self.selected().map(|player| {
                    (
                        player.id,
                        self.playback()
                            .and_then(|value| value.track.as_ref())
                            .map(|track| (track.id, track.length)),
                    )
                });
                self.observed = true;
                match result {
                    Ok(snapshot) => {
                        self.now = Instant::now();
                        self.snapshot = snapshot;
                        self.ready = true;
                        self.observation_error.clear();
                        if !self.selected.is_some_and(|id| {
                            self.snapshot.players.iter().any(|player| player.id == id)
                        }) {
                            self.explicit_selection = false;
                        }
                        if !self.explicit_selection {
                            self.selected = self.snapshot.players.iter().find(|player| matches!(&player.state, MediaPlayerState::Ready(playback) if playback.status == PlaybackStatus::Playing))
                                .or_else(|| self.snapshot.players.iter().find(|player| matches!(player.state, MediaPlayerState::Ready(_))))
                                .or_else(|| self.snapshot.players.first()).map(|player| player.id);
                        }
                    }
                    Err(error) => {
                        if self.ready {
                            self.now = Instant::now();
                        }
                        self.ready = false;
                        self.observation_error = error.to_string();
                        if self.pending.take().is_some() {
                            self.command_cancel.cancel();
                            self.error = "Media command outcome is uncertain; observe the player before retrying".into();
                        }
                    }
                }
                let current = self.selected().map(|player| {
                    (
                        player.id,
                        self.playback()
                            .and_then(|value| value.track.as_ref())
                            .map(|track| (track.id, track.length)),
                    )
                });
                if !self.ready || previous != current || self.seek_target().is_none() {
                    self.cancel_seek();
                }
                self.settle_command();
                self.reconcile_artwork(effects);
            }
            Message::Open | Message::Toggle => {
                self.set_open(
                    if matches!(message, Message::Toggle) {
                        !self.open
                    } else {
                        true
                    },
                    effects.now,
                );
            }
            Message::Close => self.set_open(false, effects.now),
            Message::Closed(generation) => {
                if !self.open && generation == self.close_generation {
                    self.mounted = false;
                }
            }
            Message::Select(id) => {
                if self.ready && self.snapshot.players.iter().any(|player| player.id == id) {
                    self.explicit_selection = true;
                    if self.selected != Some(id) {
                        self.selected = Some(id);
                        self.cancel_seek();
                        self.error.clear();
                        self.scroll = 0.;
                        self.reconcile_artwork(effects);
                    }
                }
            }
            Message::Request(command) => {
                if !self.admits(&command) {
                    self.error = "This player or control is no longer available".into();
                } else if self.pending.is_some() {
                    self.error = "A media command is still being delivered".into();
                } else {
                    self.error.clear();
                    self.command_revision = self.command_revision.wrapping_add(1);
                    let revision = self.command_revision;
                    self.pending = Some(PendingCommand {
                        revision,
                        command,
                        operation: None,
                    });
                    self.command_cancel = Cancellation::default();
                    let adapter = self.adapter.clone();
                    effects.task_cancellable_on_with_failure(
                        crate::work::CONTROLS,
                        self.command_cancel.clone(),
                        move || Message::Admitted(revision, adapter.start(command)),
                        move |failure| {
                            Message::Admitted(
                                revision,
                                Err(DomainError::Failed(failure.to_string())),
                            )
                        },
                    );
                }
            }
            Message::Admitted(revision, result) => {
                if self
                    .pending
                    .as_ref()
                    .is_none_or(|pending| pending.revision != revision)
                {
                    return;
                }
                match result {
                    Ok(operation) => {
                        self.pending.as_mut().unwrap().operation = Some(operation);
                        // The observer may deliver completion before this short
                        // admission task's reply reaches the UI thread.
                        self.settle_command();
                    }
                    Err(error) => {
                        self.pending = None;
                        self.error = error.to_string();
                    }
                }
            }
            Message::Seek(target, event) => {
                if self.seek_target() != Some(target)
                    || !event.value.is_finite()
                    || !(0. ..=1.).contains(&event.value)
                {
                    return;
                }
                self.seek_preview = Some((target, event.value));
                if event.finished {
                    let length = self
                        .playback()
                        .and_then(|value| value.track.as_ref())
                        .and_then(|track| track.length)
                        .unwrap();
                    self.seek_preview = None;
                    let micros = ((length.micros() as f64 * f64::from(event.value)).round() as u64)
                        .min(length.micros());
                    if let Ok(position) = MediaTime::new(micros) {
                        self.update(
                            Message::Request(MediaCommand::SetPosition {
                                player: target.player,
                                track: target.track,
                                position,
                            }),
                            effects,
                        );
                    }
                }
            }
            Message::CancelSeek => self.cancel_seek(),
            Message::Tick(now) => {
                if !self.wants_progress() {
                    return;
                }
                let previous = self
                    .progress()
                    .map(|(_, position, _)| position.micros() / 1_000_000);
                self.now = now;
                if previous
                    == self
                        .progress()
                        .map(|(_, position, _)| position.micros() / 1_000_000)
                {
                    return;
                }
                effects.redraw("media-controls");
                effects.redraw("widgets");
                return;
            }
            Message::Artwork(revision, result) => {
                if self.artwork_active != Some(revision) {
                    return;
                }
                self.artwork_active = None;
                if revision == self.artwork_revision {
                    match result {
                        Ok(image) => {
                            self.artwork = Some(image);
                            self.artwork_error.clear();
                        }
                        Err(error) => self.artwork_error = error.to_string(),
                    }
                }
                self.start_artwork(effects);
            }
            Message::RetryArtwork => {
                if self.artwork_source.is_some() && !self.artwork_error.is_empty() {
                    self.artwork_revision = self.artwork_revision.wrapping_add(1);
                    self.artwork_pending =
                        self.artwork_source.clone().map(|source| ArtworkDemand {
                            revision: self.artwork_revision,
                            source,
                        });
                    self.artwork_error.clear();
                    self.start_artwork(effects);
                }
            }
            Message::Scroll(event) => {
                if event.offset.is_finite() && event.offset >= 0. {
                    self.scroll = event.offset;
                }
            }
            Message::Ignore => return,
        }
        effects.redraw("media-controls");
        effects.redraw("bar");
        effects.redraw("widgets");
    }
    fn settle_command(&mut self) {
        let Some(pending) = &self.pending else {
            return;
        };
        let Some(operation) = self.snapshot.operation.as_ref().filter(|operation| {
            Some(operation.id) == pending.operation && operation.command == pending.command
        }) else {
            return;
        };
        let error = match &operation.phase {
            MediaOperationPhase::InProgress => return,
            MediaOperationPhase::Acknowledged => String::new(),
            MediaOperationPhase::Failed(error) => error.to_string(),
        };
        self.pending = None;
        self.error = error;
    }
    fn set_open(&mut self, open: bool, now: f64) {
        if self.open == open {
            return;
        }
        self.open = open;
        self.mounted = true;
        if !open {
            self.close_generation = self.close_generation.wrapping_add(1);
            self.cancel_seek();
        }
        self.motion.target(
            if open { 1. } else { 0. },
            now,
            if open { motion::ENTER } else { motion::EXIT },
            motion::DECELERATE,
        );
    }
    fn reconcile_artwork(&mut self, effects: &mut Effects<Message>) {
        let target = self
            .ready
            .then(|| {
                self.selected()
                    .zip(self.playback().and_then(|value| value.track.as_ref()))
            })
            .flatten()
            .map(|(player, track)| (player.id, track.id));
        let next = target.and_then(|_| {
            self.playback()
                .and_then(|value| value.track.as_ref())
                .and_then(|track| track.artwork.clone())
        });
        if next == self.artwork_source && target == self.artwork_target {
            return;
        }
        self.artwork_target = target;
        self.artwork_revision = self.artwork_revision.wrapping_add(1);
        self.artwork_source = next.clone();
        self.artwork = None;
        self.artwork_error.clear();
        self.artwork_pending = next.map(|source| ArtworkDemand {
            revision: self.artwork_revision,
            source,
        });
        self.start_artwork(effects);
    }
    fn start_artwork(&mut self, effects: &mut Effects<Message>) {
        if self.artwork_active.is_some() {
            return;
        }
        let Some(demand) = self.artwork_pending.take() else {
            return;
        };
        self.artwork_active = Some(demand.revision);
        let assets = self.assets.clone();
        let revision = demand.revision;
        effects.task_on_with_failure(
            crate::work::ASSETS,
            move || Message::Artwork(revision, assets.media_artwork(&demand.source)),
            move |failure| {
                Message::Artwork(revision, Err(DomainError::Failed(failure.to_string())))
            },
        );
    }
    fn wants_progress(&self) -> bool {
        self.ready
            && (self.open || self.progress_visible)
            && self.playback().is_some_and(|playback| {
                playback.status == PlaybackStatus::Playing
                    && playback.position.is_some()
                    && playback
                        .track
                        .as_ref()
                        .and_then(|track| track.length)
                        .is_some_and(|length| length.micros() > 0)
            })
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        let adapter = self.adapter.clone();
        let mut subscriptions = vec![Subscription::stream("media", move |out, cancel| {
            adapter.watch(
                &mut |snapshot| out.send(Message::Observed(snapshot)),
                &WatchStop(cancel),
            );
        })];
        if self.wants_progress() {
            subscriptions.push(Subscription::every(
                "media-progress",
                Duration::from_secs(1),
                || Message::Tick(Instant::now()),
            ));
        }
        if self.mounted && !self.open {
            let generation = self.close_generation;
            subscriptions.push(Subscription::stream(
                format!("media-close-{generation}"),
                move |out, cancel| {
                    cancel.sleep(Duration::from_secs_f64(motion::EXIT));
                    if !cancel.cancelled() {
                        out.send(Message::Closed(generation));
                    }
                },
            ));
        }
        subscriptions
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"status":if !self.observed {"loading"} else if self.ready {"ready"} else {"unavailable"},
            "observation_error":self.observation_error,"control_error":self.error,"busy":self.pending.is_some(),
            "open":self.open,"visible":self.mounted,"selected":self.selected.map(MediaPlayerId::value),
            "pending_operation":self.pending.as_ref().and_then(|pending|pending.operation).map(MediaOperationId::value),
            "command_phase":self.pending.as_ref().map(|pending|if pending.operation.is_some(){"in_progress"}else{"admitting"}),
            "players":self.snapshot.players.iter().map(|player|{
                let mut value=serde_json::json!({"id":player.id.value(),"name":player.name,"available":self.ready && matches!(player.state,MediaPlayerState::Ready(_))});
                if let MediaPlayerState::Ready(playback)=&player.state {
                    value["status"]=serde_json::json!(format!("{:?}",playback.status).to_ascii_lowercase());
                    value["track"]=serde_json::json!(playback.track.as_ref().map(|track|track.id.value()));
                    value["title"]=serde_json::json!(playback.track.as_ref().map(|track|&track.title));
                    value["can_seek"]=serde_json::json!(playback.capabilities.can_control && playback.capabilities.can_seek);
                    value["can_play"]=serde_json::json!(playback.capabilities.can_control && playback.capabilities.can_play);
                    value["can_pause"]=serde_json::json!(playback.capabilities.can_control && playback.capabilities.can_pause);
                }
                value
            }).collect::<Vec<_>>(),
            "title":self.label(),"artist":self.artist(),"artwork_ready":self.artwork.is_some(),"artwork_error":self.artwork_error,
            "artwork_busy":self.artwork_active.is_some(),"seek_active":self.seek_preview.is_some(),"progress_ticking":self.wants_progress(),
            "position_us":self.progress().map(|(_,position,_)|position.micros()),"duration_us":self.progress().map(|(_,_,length)|length.micros())})
    }
}
