use super::*;
use crate::ports::{BackgroundTarget, Images};
use std::{collections::BTreeMap, sync::Mutex};
#[derive(Default)]
struct Adapter {
    commands: Mutex<Vec<MediaCommand>>,
    images: Mutex<Vec<String>>,
    admission_error: Mutex<Option<DomainError>>,
}
impl MediaPort for Adapter {
    fn watch(&self, emit: &mut dyn FnMut(Result<MediaSnapshot>), _: &dyn StopSignal) {
        emit(Ok(fixture()));
    }
    fn start(&self, command: MediaCommand) -> Result<MediaOperationId> {
        if let Some(error) = self.admission_error.lock().unwrap().take() {
            return Err(error);
        }
        let mut commands = self.commands.lock().unwrap();
        commands.push(command);
        MediaOperationId::new(commands.len() as u64)
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
fn operation(id: u64, command: MediaCommand, phase: MediaOperationPhase) -> MediaSnapshot {
    let mut snapshot = fixture();
    snapshot.operation = Some(MediaOperation {
        id: MediaOperationId::new(id).unwrap(),
        command,
        phase,
    });
    snapshot
}
fn media_view(media: &Media) -> lucent_api::Element<Message> {
    media.view(
        &lucent_api::ViewContext {
            surface: "media-controls",
            instance: None,
            width: 640.,
            height: 720.,
            now: 1.,
            density: lucent_api::Density::ONE,
        },
        lucent_design::Theme::from_palette(&lucent_design::palettes()[0]),
    )
}
fn seek_slider(media: &Media) -> Option<lucent_api::SliderCallback<Message>> {
    fn find(element: &lucent_api::Element<Message>) -> Option<lucent_api::SliderCallback<Message>> {
        if element.id.starts_with("media-seek-") {
            element.slider.clone()
        } else {
            element.children.iter().find_map(find)
        }
    }
    find(&media_view(media))
}
fn seek_event(finished: bool) -> SliderEvent {
    SliderEvent {
        value: 0.5,
        finished,
    }
}
fn media_layout() -> lucent_ui::Layout {
    lucent_ui::Layout::new(Arc::new(vec![
        lucent_ui::text::Font::from_bytes(
            include_bytes!("../../assets/LucentSans.ttf") as &[u8],
            Default::default(),
        )
        .unwrap(),
    ]))
}

#[test]
fn pending_status_stays_visible_below_scrolling_artwork_and_long_metadata() {
    use lucent_api::{Rect, ScrollEvent, ViewContext};
    use lucent_ui::{Interaction, Paint};
    fn contains(outer: Rect, inner: Rect) -> bool {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.x + inner.w <= outer.x + outer.w
            && inner.y + inner.h <= outer.y + outer.h
    }
    let layout = media_layout();
    for (width, height) in [(640., 580.), (390., 420.), (320., 420.)] {
        let (mut media, _) = client();
        let mut snapshot = with_art(1);
        if let MediaPlayerState::Ready(playback) = &mut snapshot.players[0].state {
            let track = playback.track.as_mut().unwrap();
            track.title =
                "A long track title that wraps across the available metadata width ".repeat(3);
            track.artists = vec!["A long artist name with accompanying performers ".repeat(3)];
            track.album = "A long album title with additional edition information ".repeat(3);
        }
        let mut effects = observe(&mut media, snapshot);
        media.update(effects.tasks.pop().unwrap().run(), &mut effects);
        assert!(media.artwork().is_some());
        media.update(Message::Open, &mut effects);
        media.update(
            Message::Request(media.toggle_command().unwrap()),
            &mut effects,
        );
        let cx = ViewContext {
            surface: "media-controls",
            instance: None,
            width,
            height,
            now: 1.,
            density: lucent_api::Density::ONE,
        };
        let theme = lucent_design::Theme::from_palette(&lucent_design::palettes()[0]);
        for expected in ["Sending command…", "Waiting for player…"] {
            if expected == "Waiting for player…" {
                media.update(effects.tasks.pop().unwrap().run(), &mut effects);
            }
            let mut fixed = None;
            for offset in [0., 10000.] {
                media.update(Message::Scroll(ScrollEvent { offset }), &mut effects);
                let scene = layout.build(
                    &media.view(&cx, theme),
                    width,
                    height,
                    &Interaction::default(),
                    1.,
                );
                let scroll = scene
                    .hits
                    .iter()
                    .find(|hit| hit.id == "media-scroll")
                    .unwrap();
                assert!(scroll.scroll.as_ref().unwrap().maximum > 0.);
                let (rect, clip) = scene
                    .paint
                    .iter()
                    .find_map(|paint| match paint {
                        Paint::Text {
                            text, rect, clip, ..
                        } if text == expected => Some((*rect, clip.bounds)),
                        _ => None,
                    })
                    .expect("pending status must be painted");
                assert!(contains(Rect::new(0., 0., width, height), rect));
                assert!(
                    contains(clip, rect),
                    "{expected} clipped at {width}×{height}"
                );
                assert!(
                    rect.y >= scroll.rect.y + scroll.rect.h,
                    "pending status belongs outside the metadata viewport"
                );
                if let Some(previous) = fixed {
                    assert_eq!(rect, previous, "scrolling cannot move pending status");
                }
                fixed = Some(rect);
                assert!(
                    scene
                        .hits
                        .iter()
                        .all(|hit| !hit.id.starts_with("media-seek-"))
                );
            }
        }
    }
}

#[test]
fn pending_seek_preserves_paint_geometry_without_hits_focus_or_capture() {
    use lucent_api::{Element, Kind, Rect};
    use lucent_ui::{Interaction, Paint, Scene};
    fn shapes(scene: &Scene<Message>, bounds: Rect) -> Vec<(Rect, f32)> {
        scene
            .paint
            .iter()
            .filter_map(|paint| match paint {
                Paint::Shape { rect, radius, .. }
                    if rect.x >= bounds.x
                        && rect.y >= bounds.y
                        && rect.x + rect.w <= bounds.x + bounds.w
                        && rect.y + rect.h <= bounds.y + bounds.h =>
                {
                    Some((*rect, *radius))
                }
                _ => None,
            })
            .collect()
    }
    fn has_text(tree: &Element<Message>, text: &str) -> bool {
        matches!(&tree.kind, Kind::Text(value) if value == text)
            || tree.children.iter().any(|child| has_text(child, text))
    }
    let layout = media_layout();
    let (mut media, _) = client();
    observe(&mut media, fixture());
    let mut effects = Effects::default();
    media.update(Message::Open, &mut effects);
    let mut input = Interaction::default();
    let scene = layout.build(&media_view(&media), 640., 720., &input, 1.);
    let hit = scene
        .hits
        .iter()
        .find(|hit| hit.id.starts_with("media-seek-"))
        .unwrap();
    let bounds = hit.rect;
    let before = shapes(&scene, bounds);
    assert_eq!(
        before.len(),
        3,
        "the slider paints its track, fill and thumb"
    );
    let point = (bounds.x + bounds.w / 2., bounds.y + bounds.h / 2.);
    assert!(matches!(
        input.press(&scene, point.0, point.1),
        Some(Message::Seek(..))
    ));
    let command = media.toggle_command().unwrap();
    media.update(Message::Request(command), &mut effects);
    let tree = media_view(&media);
    assert!(has_text(&tree, "Sending command…"));
    let pending = layout.build(&tree, 640., 720., &input, 1.);
    assert!(
        pending
            .hits
            .iter()
            .all(|hit| !hit.id.starts_with("media-seek-"))
    );
    assert_eq!(
        shapes(&pending, bounds),
        before,
        "pending preserves decorative slider geometry"
    );
    input.synchronize(&pending);
    assert!(
        !input
            .focus
            .as_deref()
            .is_some_and(|id| id.starts_with("media-seek-"))
    );
    assert!(
        input.release(point.0, point.1).is_none(),
        "retired slider capture cannot release"
    );
    media.update(effects.tasks.pop().unwrap().run(), &mut effects);
    let tree = media_view(&media);
    assert!(has_text(&tree, "Waiting for player…"));
    assert!(!has_text(&tree, "Sending command…"));
    let admitted = layout.build(&tree, 640., 720., &input, 1.);
    assert!(
        admitted
            .hits
            .iter()
            .all(|hit| !hit.id.starts_with("media-seek-"))
    );
    assert_eq!(shapes(&admitted, bounds), before);
    observe(
        &mut media,
        operation(1, command, MediaOperationPhase::Acknowledged),
    );
    let settled = layout.build(&media_view(&media), 640., 720., &input, 1.);
    assert!(
        settled
            .hits
            .iter()
            .any(|hit| hit.id.starts_with("media-seek-") && hit.slider.is_some())
    );
}

#[test]
fn rendered_seek_callbacks_retire_across_transport_and_seek_admission_and_completion() {
    let seek = MediaCommand::SetPosition {
        player: MediaPlayerId::new(1).unwrap(),
        track: MediaTrackId::new(1).unwrap(),
        position: MediaTime::new(90_000_000).unwrap(),
    };
    for command in [MediaCommand::Pause(seek.player()), seek] {
        for terminal_first in [false, true] {
            for failure in [false, true] {
                let (mut media, adapter) = client();
                let mut effects = Effects::default();
                observe(&mut media, fixture());
                media.update(Message::Open, &mut effects);
                let retired = seek_slider(&media).unwrap();
                media.update(retired(seek_event(false)), &mut effects);
                assert!(media.seek_preview.is_some());
                assert!(effects.tasks.is_empty());
                media.update(Message::Request(command), &mut effects);
                assert!(media.seek_preview.is_none());
                assert!(media.seek_target().is_none());
                assert!(seek_slider(&media).is_none());
                let admission = effects.tasks.pop().unwrap();
                for finished in [false, true] {
                    media.update(retired(seek_event(finished)), &mut effects);
                    assert!(effects.tasks.is_empty());
                    assert!(media.seek_preview.is_none());
                    assert!(
                        media.error.is_empty(),
                        "retired UI input cannot create a busy error"
                    );
                }
                let admitted = admission.run();
                let terminal = operation(
                    1,
                    command,
                    if failure {
                        MediaOperationPhase::Failed(DomainError::Failed("Player refused".into()))
                    } else {
                        MediaOperationPhase::Acknowledged
                    },
                );
                if terminal_first {
                    observe(&mut media, terminal);
                    assert!(
                        seek_slider(&media).is_none(),
                        "terminal observation still needs admission identity"
                    );
                    media.update(retired(seek_event(true)), &mut effects);
                    media.update(admitted, &mut effects);
                } else {
                    media.update(admitted, &mut effects);
                    observe(
                        &mut media,
                        operation(1, command, MediaOperationPhase::InProgress),
                    );
                    assert!(
                        seek_slider(&media).is_none(),
                        "admission is not native completion"
                    );
                    media.update(retired(seek_event(true)), &mut effects);
                    observe(&mut media, terminal);
                }
                assert!(media.pending.is_none());
                assert_eq!(media.error.contains("Player refused"), failure);
                let before = media.error.clone();
                media.update(retired(seek_event(true)), &mut effects);
                assert!(
                    effects.tasks.is_empty(),
                    "the original callback stays retired after recovery"
                );
                assert_eq!(media.error, before);
                let fresh = seek_slider(&media).unwrap();
                media.update(fresh(seek_event(true)), &mut effects);
                media.update(fresh(seek_event(true)), &mut effects);
                assert_eq!(effects.tasks.len(), 1);
                media.update(effects.tasks.pop().unwrap().run(), &mut Effects::default());
                assert_eq!(*adapter.commands.lock().unwrap(), [command, seek]);
                assert_eq!(
                    media
                        .playback()
                        .unwrap()
                        .position
                        .unwrap()
                        .position
                        .micros(),
                    30_000_000,
                    "seeking does not rewrite observed playback"
                );
            }
        }
    }
}

#[test]
fn rejected_admission_restores_fresh_seek_input_without_reviving_the_old_gesture() {
    let (mut media, adapter) = client();
    let mut effects = Effects::default();
    observe(&mut media, fixture());
    media.update(Message::Open, &mut effects);
    let retired = seek_slider(&media).unwrap();
    media.update(retired(seek_event(false)), &mut effects);
    *adapter.admission_error.lock().unwrap() =
        Some(DomainError::Unavailable("Mailbox busy".into()));
    media.update(
        Message::Request(media.toggle_command().unwrap()),
        &mut effects,
    );
    media.update(effects.tasks.pop().unwrap().run(), &mut Effects::default());
    assert!(media.pending.is_none());
    assert!(media.seek_preview.is_none());
    media.update(retired(seek_event(true)), &mut effects);
    assert!(effects.tasks.is_empty());
    assert!(media.error.contains("Mailbox busy"));
    let fresh = seek_slider(&media).unwrap();
    media.update(fresh(seek_event(true)), &mut effects);
    media.update(effects.tasks.pop().unwrap().run(), &mut Effects::default());
    assert_eq!(
        *adapter.commands.lock().unwrap(),
        [MediaCommand::SetPosition {
            player: MediaPlayerId::new(1).unwrap(),
            track: MediaTrackId::new(1).unwrap(),
            position: MediaTime::new(90_000_000).unwrap(),
        }]
    );
}

#[test]
fn observer_recovery_does_not_restore_a_rendered_seek_from_before_uncertain_admission() {
    let (mut media, adapter) = client();
    let mut effects = Effects::default();
    observe(&mut media, fixture());
    media.update(Message::Open, &mut effects);
    let retired = seek_slider(&media).unwrap();
    media.update(retired(seek_event(false)), &mut effects);
    let command = media.toggle_command().unwrap();
    media.update(Message::Request(command), &mut effects);
    let late = effects.tasks.pop().unwrap().run();
    media.update(
        Message::Observed(Err(DomainError::Unavailable("Disconnected".into()))),
        &mut effects,
    );
    assert!(seek_slider(&media).is_none());
    media.update(retired(seek_event(true)), &mut effects);
    observe(&mut media, fixture());
    media.update(late, &mut effects);
    media.update(retired(seek_event(true)), &mut effects);
    assert!(effects.tasks.is_empty());
    assert!(media.pending.is_none());
    assert!(media.error.contains("outcome is uncertain"));
    assert_eq!(*adapter.commands.lock().unwrap(), [command]);
    let fresh = seek_slider(&media).unwrap();
    media.update(fresh(seek_event(true)), &mut effects);
    assert_eq!(effects.tasks.len(), 1);
}

#[test]
fn global_player_selection_and_requests_work_closed_and_keep_exact_command_targets() {
    let (mut media, adapter) = client();
    let mut snapshot = fixture();
    let mut second = snapshot.players[0].clone();
    second.id = MediaPlayerId::new(2).unwrap();
    if let MediaPlayerState::Ready(playback) = &mut second.state {
        playback.track.as_mut().unwrap().id = MediaTrackId::new(2).unwrap();
    }
    snapshot.players.push(second);
    observe(&mut media, snapshot.clone());
    let mut effects = Effects::default();
    media.update(media.command("select 2").unwrap(), &mut effects);
    let selected = media.selected.unwrap();
    assert_eq!(selected.value(), 2);
    media.update(media.command("next").unwrap(), &mut effects);
    let admission = effects.tasks.pop().unwrap();
    media.update(media.command("select 1").unwrap(), &mut effects);
    assert_eq!(media.selected.unwrap().value(), 1);
    assert!(media.seek_target().is_none());
    media.update(admission.run(), &mut effects);
    let command = MediaCommand::Next(selected);
    assert_eq!(*adapter.commands.lock().unwrap(), [command]);
    snapshot.operation = Some(MediaOperation {
        id: MediaOperationId::new(1).unwrap(),
        command,
        phase: MediaOperationPhase::Acknowledged,
    });
    observe(&mut media, snapshot);
    assert!(!media.open());
    assert!(media.pending.is_none());
    let target = media.seek_target().unwrap();
    assert_eq!(target.player.value(), 1);
    assert_eq!(target.track.value(), 1);
    assert!(effects.tasks.is_empty());
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
    assert_eq!(task.queue, crate::work::CONTROLS);
    media.update(task.run(), &mut Effects::default());
    assert!(media.pending.is_some(), "admission is not completion");
    assert_eq!(
        *adapter.commands.lock().unwrap(),
        [MediaCommand::SetPosition {
            player: current.player,
            track: current.track,
            position: MediaTime::new(90_000_000).unwrap()
        }]
    );
    let command = adapter.commands.lock().unwrap()[0];
    observe(
        &mut media,
        operation(1, command, MediaOperationPhase::Acknowledged),
    );
    assert!(media.pending.is_none());
    // Native acknowledgement does not invent an observed seeked position.
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
fn selection_close_and_player_retirement_do_not_revoke_or_retarget_admitted_work() {
    let (mut media, adapter) = client();
    observe(&mut media, fixture());
    let command = media.toggle_command().unwrap();
    let mut effects = Effects::default();
    media.update(Message::Request(command), &mut effects);
    let task = effects.tasks.pop().unwrap();
    let admitted = task.run();
    assert_eq!(*adapter.commands.lock().unwrap(), [command]);
    let mut replacement = fixture();
    replacement.players[0].id = MediaPlayerId::new(2).unwrap();
    observe(&mut media, replacement.clone());
    media.update(
        Message::Select(MediaPlayerId::new(2).unwrap()),
        &mut effects,
    );
    media.update(Message::Close, &mut effects);
    assert!(media.pending.is_some());
    media.update(admitted, &mut effects);
    assert_eq!(
        media.pending.as_ref().unwrap().operation.unwrap().value(),
        1
    );
    assert_eq!(media.selected.unwrap().value(), 2);
    replacement.operation = Some(MediaOperation {
        id: MediaOperationId::new(1).unwrap(),
        command,
        phase: MediaOperationPhase::Failed(DomainError::Unavailable("Player retired".into())),
    });
    observe(&mut media, replacement);
    assert!(media.pending.is_none());
    assert!(media.error.contains("Player retired"));
    assert_eq!(*adapter.commands.lock().unwrap(), [command]);
}

#[test]
fn native_completion_joins_admission_in_either_order_without_inventing_playback() {
    for terminal_first in [false, true] {
        for failure in [false, true] {
            let (mut media, _) = client();
            observe(&mut media, fixture());
            let command = media.toggle_command().unwrap();
            let mut effects = Effects::default();
            media.update(Message::Request(command), &mut effects);
            assert_eq!(media.inspect()["command_phase"], "admitting");
            let admitted = effects.tasks.pop().unwrap().run();
            observe(
                &mut media,
                operation(1, command, MediaOperationPhase::InProgress),
            );
            assert!(media.pending.is_some());
            let terminal = operation(
                1,
                command,
                if failure {
                    MediaOperationPhase::Failed(DomainError::Failed("Player denied command".into()))
                } else {
                    MediaOperationPhase::Acknowledged
                },
            );
            if terminal_first {
                observe(&mut media, terminal);
                assert!(
                    media.pending.is_some(),
                    "completion needs the admitted identity"
                );
                media.update(admitted, &mut effects);
            } else {
                media.update(admitted, &mut effects);
                assert_eq!(media.inspect()["command_phase"], "in_progress");
                assert_eq!(media.inspect()["pending_operation"], 1);
                observe(&mut media, terminal);
            }
            assert!(media.pending.is_none());
            assert_eq!(media.error.contains("Player denied command"), failure);
            assert_eq!(media.playback().unwrap().status, PlaybackStatus::Playing);
        }
    }
}

#[test]
fn unrelated_operation_ids_commands_and_stale_admission_cannot_finish_newer_request() {
    let (mut media, _) = client();
    observe(&mut media, fixture());
    let command = media.toggle_command().unwrap();
    let mut effects = Effects::default();
    media.update(Message::Request(command), &mut effects);
    let first_revision = media.command_revision;
    media.update(effects.tasks.pop().unwrap().run(), &mut effects);
    observe(
        &mut media,
        operation(1, command, MediaOperationPhase::Acknowledged),
    );
    media.update(Message::Request(command), &mut effects);
    let admitted = effects.tasks.pop().unwrap().run();
    observe(
        &mut media,
        operation(
            1,
            command,
            MediaOperationPhase::Failed(DomainError::Failed("Old failure".into())),
        ),
    );
    media.update(
        Message::Admitted(
            first_revision,
            Err(DomainError::Failed("Old admission".into())),
        ),
        &mut effects,
    );
    assert!(media.pending.as_ref().unwrap().operation.is_none());
    assert!(media.error.is_empty());
    media.update(admitted, &mut effects);
    observe(
        &mut media,
        operation(1, command, MediaOperationPhase::Acknowledged),
    );
    assert_eq!(
        media.pending.as_ref().unwrap().operation.unwrap().value(),
        2
    );
    observe(
        &mut media,
        operation(
            2,
            MediaCommand::Next(command.player()),
            MediaOperationPhase::Acknowledged,
        ),
    );
    assert!(
        media.pending.is_some(),
        "matching ID alone does not grant command authority"
    );
    observe(
        &mut media,
        operation(2, command, MediaOperationPhase::Acknowledged),
    );
    assert!(media.pending.is_none());
}

#[test]
fn observation_loss_retires_queued_or_admitted_work_without_replaying_recovery_results() {
    for admitted_first in [false, true] {
        let (mut media, _) = client();
        observe(&mut media, fixture());
        let command = media.toggle_command().unwrap();
        let mut effects = Effects::default();
        media.update(Message::Request(command), &mut effects);
        let task = effects.tasks.pop().unwrap();
        let cancellation = task.cancellation.clone();
        let revision = media.command_revision;
        if admitted_first {
            media.update(task.run(), &mut effects);
        } else {
            drop(task);
        }
        media.update(
            Message::Observed(Err(DomainError::Unavailable("Disconnected".into()))),
            &mut effects,
        );
        assert!(cancellation.cancelled());
        assert!(media.pending.is_none());
        let uncertain = media.error.clone();
        assert!(uncertain.contains("outcome is uncertain"));
        media.update(
            Message::Admitted(revision, Ok(MediaOperationId::new(1).unwrap())),
            &mut effects,
        );
        observe(
            &mut media,
            operation(1, command, MediaOperationPhase::Acknowledged),
        );
        assert!(media.pending.is_none());
        assert_eq!(media.error, uncertain);
        assert!(
            effects.tasks.is_empty(),
            "reconnection must not replay an uncertain method"
        );
    }
}

#[test]
fn rejected_local_admission_clears_only_its_request_and_permits_explicit_retry() {
    let (mut media, adapter) = client();
    observe(&mut media, fixture());
    *adapter.admission_error.lock().unwrap() =
        Some(DomainError::Unavailable("Native mailbox busy".into()));
    let mut effects = Effects::default();
    let command = media.toggle_command().unwrap();
    media.update(Message::Request(command), &mut effects);
    media.update(effects.tasks.pop().unwrap().run(), &mut effects);
    assert!(media.pending.is_none());
    assert!(media.error.contains("Native mailbox busy"));
    assert!(adapter.commands.lock().unwrap().is_empty());
    media.update(Message::Request(command), &mut effects);
    media.update(effects.tasks.pop().unwrap().run(), &mut effects);
    assert!(media.pending.is_some());
    assert!(media.error.is_empty());
    assert_eq!(*adapter.commands.lock().unwrap(), [command]);
}

#[test]
fn waiting_for_native_media_reply_does_not_occupy_the_volume_lane() {
    let (mut media, _) = client();
    observe(&mut media, fixture());
    let mut effects = Effects::default();
    media.update(
        Message::Request(media.toggle_command().unwrap()),
        &mut effects,
    );
    let admission = effects.tasks.pop().unwrap();
    assert_eq!(admission.queue, crate::work::CONTROLS);
    media.update(admission.run(), &mut effects);
    assert!(
        effects.tasks.is_empty(),
        "native reply waiting stays in the observer"
    );
    assert!(media.pending.is_some());

    let adapter = Arc::new(crate::test_adapters::Fake::default());
    let mut audio = crate::audio::commands::Commands::new(adapter.clone());
    let snapshot = crate::audio::fixture();
    let mut volume = Effects::default();
    audio.request(
        AudioCommand::SetVolume(
            AudioObjectId::new(1).unwrap(),
            AudioVolume::new(0.4).unwrap(),
        ),
        Some(&snapshot),
        &mut volume,
    );
    let task = volume.tasks.pop().unwrap();
    assert_eq!(task.queue, crate::work::AUDIO);
    audio.update(task.run(), Some(&snapshot), &mut volume);
    assert!(!audio.busy());
    assert_eq!(*adapter.calls.lock().unwrap(), ["audio"]);
    assert!(
        media.pending.is_some(),
        "volume can finish while media still waits"
    );
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
            ..Default::default()
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
            ..Default::default()
        },
    );
    assert_eq!(media.selected, Some(b.id));
    media.update(Message::Select(a.id), &mut Effects::default());
    observe(
        &mut media,
        MediaSnapshot {
            players: vec![a.clone(), b.clone()],
            ..Default::default()
        },
    );
    assert_eq!(media.selected, Some(a.id));
    observe(
        &mut media,
        MediaSnapshot {
            players: vec![b.clone()],
            ..Default::default()
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
    assert_eq!(task.queue, crate::work::CONTROLS);
    let fail = task.take_failure_handler().unwrap();
    media.update(
        fail(lucent_api::RuntimeFailure {
            source: "effect queue controls".into(),
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
