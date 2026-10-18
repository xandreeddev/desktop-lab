use super::*;
use crate::ports::{BackgroundTarget, Images};
use std::{collections::BTreeMap, sync::Mutex};
#[derive(Default)]
struct Adapter {
    commands: Mutex<Vec<MediaCommand>>,
    images: Mutex<Vec<String>>,
}
impl MediaPort for Adapter {
    fn watch(&self, emit: &mut dyn FnMut(Result<MediaSnapshot>), _: &dyn StopSignal) {
        emit(Ok(fixture()));
    }
    fn control(&self, command: MediaCommand) -> Result<()> {
        self.commands.lock().unwrap().push(command);
        Ok(())
    }
}
impl AssetPort for Adapter {
    fn initial(&self) -> BTreeMap<String, Arc<ImageData>> {
        BTreeMap::new()
    }
    fn application_icons(&self, _: &[Application]) -> Images {
        vec![]
    }
    fn thumbnail(&self, _: &str) -> Result<Arc<ImageData>> {
        Err(DomainError::Unavailable("fixture".into()))
    }
    fn background(&self, _: &str, _: Option<BackgroundTarget>) -> Result<Arc<ImageData>> {
        Err(DomainError::Unavailable("fixture".into()))
    }
    fn preview(&self, image: &ImageData) -> Arc<ImageData> {
        Arc::new(image.downsample(1))
    }
    fn media_artwork(&self, source: &MediaArtworkSource) -> Result<Arc<ImageData>> {
        self.images.lock().unwrap().push(source.value().into());
        Ok(Arc::new(
            ImageData::from_rgba(source.value(), 1, 1, vec![80, 120, 160, 255]).unwrap(),
        ))
    }
}
fn client() -> (Media, Arc<Adapter>) {
    let adapter = Arc::new(Adapter::default());
    (Media::new(adapter.clone(), adapter.clone()), adapter)
}
fn observe(media: &mut Media, snapshot: MediaSnapshot) -> Effects<Message> {
    let mut effects = Effects::default();
    media.update(Message::Observed(Ok(snapshot)), &mut effects);
    effects
}
fn with_art(track: u64) -> MediaSnapshot {
    let mut snapshot = fixture();
    if let MediaPlayerState::Ready(playback) = &mut snapshot.players[0].state {
        let selected = playback.track.as_mut().unwrap();
        selected.id = MediaTrackId::new(track).unwrap();
        selected.artwork =
            Some(MediaArtworkSource::new("file:///owned/current-cover.png").unwrap());
    }
    snapshot
}
#[test]
fn observation_distinguishes_loading_empty_unavailable_and_last_good_without_authority() {
    let (mut media, adapter) = client();
    assert_eq!(media.inspect()["status"], "loading");
    assert_eq!(media.label(), "Connecting to players…");
    observe(&mut media, MediaSnapshot::default());
    assert_eq!(media.label(), "No media players");
    observe(&mut media, fixture());
    let title = media.label().to_string();
    media.update(
        Message::Observed(Err(DomainError::Unavailable("offline".into()))),
        &mut Effects::default(),
    );
    let frozen = media.progress();
    assert_eq!(media.inspect()["status"], "unavailable");
    assert_eq!(media.label(), "Media unavailable");
    assert_eq!(
        media.playback().unwrap().track.as_ref().unwrap().title,
        title
    );
    media.update(
        Message::Observed(Err(DomainError::Unavailable("still offline".into()))),
        &mut Effects::default(),
    );
    assert_eq!(media.progress(), frozen);
    let mut effects = Effects::default();
    media.update(
        Message::Request(MediaCommand::Play(MediaPlayerId::new(1).unwrap())),
        &mut effects,
    );
    assert!(effects.tasks.is_empty());
    assert!(adapter.commands.lock().unwrap().is_empty());
    observe(&mut media, fixture());
    assert_eq!(media.selected.unwrap().value(), 1);
}
#[test]
fn seek_dispatches_only_release_and_rejects_retired_track_or_cancelled_capture() {
    let (mut media, adapter) = client();
    observe(&mut media, fixture());
    let original = media.seek_target().unwrap();
    let mut effects = Effects::default();
    media.update(
        Message::Seek(
            original,
            SliderEvent {
                value: 0.5,
                finished: false,
            },
        ),
        &mut effects,
    );
    assert!(effects.tasks.is_empty());
    assert!(media.seek_preview.is_some());
    observe(&mut media, with_art(2));
    media.update(
        Message::Seek(
            original,
            SliderEvent {
                value: 0.9,
                finished: true,
            },
        ),
        &mut effects,
    );
    assert!(effects.tasks.is_empty());
    let current = media.seek_target().unwrap();
    media.update(Message::CancelSeek, &mut effects);
    media.update(
        Message::Seek(
            current,
            SliderEvent {
                value: 0.9,
                finished: true,
            },
        ),
        &mut effects,
    );
    assert!(effects.tasks.is_empty());
    let current = media.seek_target().unwrap();
    media.update(
        Message::Seek(
            current,
            SliderEvent {
                value: 0.5,
                finished: true,
            },
        ),
        &mut effects,
    );
    assert_eq!(effects.tasks.len(), 1);
    let task = effects.tasks.pop().unwrap();
    assert_eq!(task.queue, crate::work::AUDIO);
    media.update(task.run(), &mut Effects::default());
    assert_eq!(
        *adapter.commands.lock().unwrap(),
        [MediaCommand::SetPosition {
            player: current.player,
            track: current.track,
            position: MediaTime::new(90_000_000).unwrap()
        }]
    );
    // Delivery acknowledgement does not invent an observed seeked position.
    assert_eq!(
        media
            .playback()
            .unwrap()
            .position
            .unwrap()
            .position
            .micros(),
        30_000_000
    );
}
#[test]
fn player_retirement_cancels_queued_work_and_stale_completion_cannot_clear_new_failure() {
    let (mut media, _) = client();
    observe(&mut media, fixture());
    let mut effects = Effects::default();
    media.update(
        Message::Request(media.toggle_command().unwrap()),
        &mut effects,
    );
    let task = effects.tasks.pop().unwrap();
    let revision = media.command_revision;
    observe(&mut media, MediaSnapshot::default());
    assert!(task.cancellation.cancelled());
    assert!(media.pending.is_none());
    drop(task);
    media.update(
        Message::Request(MediaCommand::Play(MediaPlayerId::new(1).unwrap())),
        &mut effects,
    );
    let error = media.error.clone();
    media.update(Message::Finished(revision, Ok(())), &mut effects);
    assert_eq!(media.error, error);
}
#[test]
fn selected_track_artwork_is_one_active_latest_and_same_uri_never_grants_old_revision() {
    let (mut media, adapter) = client();
    let mut first = observe(&mut media, with_art(1));
    assert_eq!(first.tasks.len(), 1);
    for track in 2..30 {
        assert!(observe(&mut media, with_art(track)).tasks.is_empty());
    }
    assert!(media.artwork.is_none());
    let mut followup = Effects::default();
    media.update(first.tasks.pop().unwrap().run(), &mut followup);
    assert!(media.artwork.is_none());
    assert_eq!(followup.tasks.len(), 1);
    media.update(followup.tasks.pop().unwrap().run(), &mut Effects::default());
    assert!(media.artwork.is_some());
    assert_eq!(adapter.images.lock().unwrap().len(), 2);
    assert_eq!(media.artwork_target.unwrap().1.value(), 29);
    observe(&mut media, MediaSnapshot::default());
    assert!(media.artwork.is_none());
    assert!(media.artwork_source.is_none());
}
#[test]
fn progress_subscription_exists_only_for_visible_selected_playing_observations() {
    let (mut media, _) = client();
    observe(&mut media, fixture());
    assert!(
        !media
            .subscriptions()
            .iter()
            .any(|subscription| subscription.id == "media-progress")
    );
    media.set_progress_visible(true);
    assert_eq!(
        media
            .subscriptions()
            .iter()
            .filter(|subscription| subscription.id == "media-progress")
            .count(),
        1
    );
    let mut paused = fixture();
    if let MediaPlayerState::Ready(playback) = &mut paused.players[0].state {
        playback.status = PlaybackStatus::Paused;
    }
    observe(&mut media, paused);
    assert!(
        !media
            .subscriptions()
            .iter()
            .any(|subscription| subscription.id == "media-progress")
    );
    observe(&mut media, fixture());
    media.set_progress_visible(false);
    let mut effects = Effects::default();
    media.update(Message::Open, &mut effects);
    assert_eq!(
        media
            .subscriptions()
            .iter()
            .filter(|subscription| subscription.id == "media-progress")
            .count(),
        1
    );
    media.update(Message::Close, &mut effects);
    assert!(!media.wants_progress());
    let generation = media.close_generation;
    media.update(Message::Open, &mut effects);
    media.update(Message::Closed(generation), &mut effects);
    assert!(media.visible());
    media.update(
        Message::Observed(Err(DomainError::Unavailable("offline".into()))),
        &mut effects,
    );
    assert!(
        !media
            .subscriptions()
            .iter()
            .any(|subscription| subscription.id == "media-progress")
    );
}
#[test]
fn ipc_requires_live_explicit_targets_and_observed_capabilities() {
    let (mut media, _) = client();
    observe(&mut media, fixture());
    assert!(matches!(
        media.command("pause 1"),
        Ok(Message::Request(MediaCommand::Pause(_)))
    ));
    assert!(
        matches!(media.command("play-pause"), Ok(Message::Request(MediaCommand::Pause(id))) if id.value() == 1)
    );
    assert!(
        matches!(media.command("next"), Ok(Message::Request(MediaCommand::Next(id))) if id.value() == 1)
    );
    assert!(
        matches!(media.command("previous"), Ok(Message::Request(MediaCommand::Previous(id))) if id.value() == 1)
    );
    assert!(matches!(
        media.command("seek 1 1 5000000"),
        Ok(Message::Request(MediaCommand::SetPosition { .. }))
    ));
    for invalid in [
        "play",
        "play 0",
        "play 2",
        "seek 1 2 0",
        "seek 1 1 999999999",
        "seek 1 1 -1",
    ] {
        assert!(media.command(invalid).is_err(), "{invalid}");
    }
    let mut snapshot = fixture();
    if let MediaPlayerState::Ready(playback) = &mut snapshot.players[0].state {
        playback.capabilities.can_pause = false;
    }
    observe(&mut media, snapshot);
    assert!(media.command("pause 1").is_err());
}
#[test]
fn initial_placeholders_choose_the_first_ready_playing_player_and_explicit_selection_is_preserved()
{
    let (mut media, _) = client();
    let mut a = fixture().players.remove(0);
    let mut b = a.clone();
    b.id = MediaPlayerId::new(2).unwrap();
    a.state = MediaPlayerState::Unavailable("loading".into());
    b.state = MediaPlayerState::Unavailable("loading".into());
    observe(
        &mut media,
        MediaSnapshot {
            players: vec![a.clone(), b.clone()],
        },
    );
    a.state = fixture().players.remove(0).state;
    if let MediaPlayerState::Ready(playback) = &mut a.state {
        playback.status = PlaybackStatus::Paused;
    }
    b.state = fixture().players.remove(0).state;
    observe(
        &mut media,
        MediaSnapshot {
            players: vec![a.clone(), b.clone()],
        },
    );
    assert_eq!(media.selected, Some(b.id));
    media.update(Message::Select(a.id), &mut Effects::default());
    observe(
        &mut media,
        MediaSnapshot {
            players: vec![a.clone(), b.clone()],
        },
    );
    assert_eq!(media.selected, Some(a.id));
    observe(
        &mut media,
        MediaSnapshot {
            players: vec![b.clone()],
        },
    );
    assert_eq!(media.selected, Some(b.id));
}
#[test]
fn command_and_artwork_failures_return_to_the_owning_operation_on_shared_lanes() {
    let (mut media, _) = client();
    observe(&mut media, fixture());
    let mut effects = Effects::default();
    media.update(
        Message::Request(media.toggle_command().unwrap()),
        &mut effects,
    );
    let mut task = effects.tasks.pop().unwrap();
    assert_eq!(task.queue, crate::work::AUDIO);
    let fail = task.take_failure_handler().unwrap();
    media.update(
        fail(lucent_api::RuntimeFailure {
            source: "effect queue audio".into(),
            reason: "queue full".into(),
        }),
        &mut effects,
    );
    assert!(media.pending.is_none());
    assert!(media.error.contains("queue full"));
    let mut art = observe(&mut media, with_art(1));
    let mut task = art.tasks.pop().unwrap();
    let fail = task.take_failure_handler().unwrap();
    media.update(
        fail(lucent_api::RuntimeFailure {
            source: "effect queue assets".into(),
            reason: "queue full".into(),
        }),
        &mut effects,
    );
    assert!(media.artwork_active.is_none());
    assert!(!media.artwork_error.is_empty());
    media.update(Message::RetryArtwork, &mut effects);
    assert_eq!(effects.tasks.len(), 1);
}
