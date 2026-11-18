use super::*;
use lucent_api::Key;
use lucent_ui::Interaction;

fn sources() -> Vec<Wallpaper> {
    ["A", "B"]
        .into_iter()
        .map(|name| Wallpaper {
            path: format!("/fixture/{name}.mp4"),
            name: name.into(),
        })
        .collect()
}
pub(super) fn ready() -> LiveWallpaper {
    let mut controller = controller();
    controller.assets = Arc::new(Posters(Arc::new(
        ImageData::from_rgba("lifetime-poster", 1, 1, vec![255; 4]).unwrap(),
    )));
    let mut effects = Effects::default();
    controller.update(Message::Open, &mut effects);
    controller.update(Message::Frame(0.), &mut effects);
    controller.update(
        Message::LibraryLoaded(controller.library_revision, Ok(sources())),
        &mut effects,
    );
    controller
}
pub(super) fn control(controller: &mut LiveWallpaper, id: &str, keyboard: bool) -> Message {
    let settings = LiveWallpapers::default();
    let mut input = Interaction::default();
    let mut scene = library_scene(controller, &settings);
    if keyboard {
        for _ in 0..=scene.hits.len() {
            for message in input.key(&scene, &Key::Tab) {
                controller.update(message, &mut Effects::default());
            }
            scene = library_scene(controller, &settings);
            if input.focus.as_deref() == Some(id) {
                break;
            }
        }
        assert_eq!(input.focus.as_deref(), Some(id));
        input.key(&scene, &Key::Enter).pop().unwrap()
    } else {
        let hit = scene.hits.iter().find(|hit| hit.id == id).unwrap();
        // Neighbor cards intentionally extend beyond the carousel viewport.
        // Exercise their visible fragment through the real clipped hit map.
        let (x, y) = (
            (hit.rect.x.max(hit.clip.bounds.x)
                + (hit.rect.x + hit.rect.w).min(hit.clip.bounds.x + hit.clip.bounds.w))
                / 2.,
            (hit.rect.y.max(hit.clip.bounds.y)
                + (hit.rect.y + hit.rect.h).min(hit.clip.bounds.y + hit.clip.bounds.h))
                / 2.,
        );
        assert!(hit.contains(x, y), "{id} must be reachable");
        assert!(input.press(&scene, x, y).is_none());
        input.release(x, y).unwrap()
    }
}
fn refresh(
    controller: &mut LiveWallpaper,
    sources: Vec<Wallpaper>,
    effects: &mut Effects<Message>,
) {
    controller.update(Message::RefreshLibrary, effects);
    controller.update(
        Message::LibraryLoaded(controller.library_revision, Ok(sources)),
        effects,
    );
    effects.tasks.clear();
}

#[test]
fn rendered_cards_cannot_select_recycled_slots_after_discovery_replaces_the_library() {
    for keyboard in [false, true] {
        for replacement in 0..3 {
            let mut controller = ready();
            let old = control(&mut controller, "live-library-0", keyboard);
            let mut library = sources();
            match replacement {
                0 => library.reverse(),
                1 => {
                    library.remove(0);
                }
                _ => library[0].name = "Same path, replaced descriptor".into(),
            }
            let mut effects = Effects::default();
            refresh(&mut controller, library, &mut effects);
            let selected = controller.selected_source().cloned();
            controller.update(old, &mut effects);
            assert_eq!(controller.selected_source(), selected.as_ref());
            assert!(effects.tasks.is_empty());
            assert!(controller.importing.is_none());
            let current = control(&mut controller, "live-library-0", keyboard);
            controller.update(current, &mut effects);
            assert_eq!(controller.selection, 0);
            assert!(controller.importing.is_none());
        }
    }
}

#[test]
fn apply_cannot_cross_source_or_target_round_trips_and_current_apply_captures_both() {
    for keyboard in [false, true] {
        for target_change in [false, true] {
            let mut controller = ready();
            let old = control(&mut controller, "live-apply", keyboard);
            let mut effects = Effects::default();
            if target_change {
                for target in [
                    LiveWallpaperTarget::Monitor(MonitorId::new("DEMO-1").unwrap()),
                    LiveWallpaperTarget::Default,
                ] {
                    controller.update(Message::Target(target), &mut effects);
                }
            } else {
                for index in [1, 0] {
                    controller.update(controller.select_message(index), &mut effects);
                }
            }
            controller.error = "Keep current feedback".into();
            controller.update(old, &mut effects);
            assert!(controller.importing.is_none());
            assert_eq!(controller.error, "Keep current feedback");
            assert!(effects.tasks.is_empty());
            let current = control(&mut controller, "live-apply", keyboard);
            assert!(matches!(
                controller.update(current, &mut effects),
                Some(Intent::ImportStarted(LiveWallpaperTarget::Default, _))
            ));
            let completion = effects.tasks.pop().unwrap().run();
            assert!(
                matches!(&completion, Message::Prepared(_, Ok((asset, _))) if asset.source == sources()[0])
            );
        }
    }
}

#[test]
fn same_path_refresh_retires_apply_and_loading_cards_but_keeps_preview_only_browsing() {
    let mut controller = ready();
    let retiring = controller.preview.subscriptions().pop().unwrap();
    let retiring_id = retiring.id.clone();
    let old = control(&mut controller, "live-apply", true);
    let mut effects = Effects::default();
    controller.update(Message::RefreshLibrary, &mut effects);
    effects.tasks.clear();
    controller.update(old.clone(), &mut effects);
    assert!(controller.importing.is_none());
    assert!(matches!(controller.apply_message(), Message::Ignore));
    let loading_card = control(&mut controller, "live-library-0", false);
    controller.update(controller.browse_message(1), &mut effects);
    assert_eq!(controller.selection, 1);
    let state = controller.preview.inspect();
    assert_eq!(state["selected"], false);
    assert_eq!(state["retiring"], true);
    let subscriptions = controller.preview.subscriptions();
    assert_eq!(subscriptions.len(), 1);
    assert_eq!(subscriptions[0].id, retiring_id);

    // Refresh retires demand, not ownership of a decoder that has not returned.
    // Run the cancelled fixture body and deliver its actual terminal ACK before
    // expecting discovery to have no preview subscription at all.
    let (sender, receiver) = std::sync::mpsc::channel();
    (retiring.run)(
        lucent_api::Emitter::new(move |message| sender.send(message).unwrap()),
        Cancellation::default(),
    );
    let messages: Vec<_> = receiver.try_iter().collect();
    assert!(matches!(
        messages.as_slice(),
        [preview::Message::Ended(_, Ok(()))]
    ));
    assert_eq!(controller.preview.subscriptions()[0].id, retiring_id);
    for message in messages {
        controller.update(Message::Preview(message), &mut effects);
    }
    assert!(controller.preview.subscriptions().is_empty());
    assert_eq!(controller.preview.inspect()["selected"], false);
    controller.update(
        Message::LibraryLoaded(controller.library_revision, Ok(sources())),
        &mut effects,
    );
    let subscriptions = controller.preview.subscriptions();
    assert_eq!(subscriptions.len(), 1);
    assert_ne!(subscriptions[0].id, retiring_id);
    controller.update(loading_card, &mut effects);
    assert_eq!(controller.selection, 1);
    controller.update(controller.select_message(0), &mut effects);
    controller.update(old, &mut effects);
    assert!(controller.importing.is_none());
    let current = control(&mut controller, "live-apply", false);
    assert!(matches!(
        controller.update(current, &mut effects),
        Some(Intent::ImportStarted(_, _))
    ));
}

#[test]
fn preview_frames_viewport_and_reselecting_the_same_source_preserve_apply_authority() {
    struct Frames;
    impl WallpaperPlaybackPort for Frames {
        fn prepare(&self, source: &Wallpaper) -> Result<LiveWallpaperAsset> {
            let mut asset = setting("/owned/fixture.mp4").asset;
            asset.source = source.clone();
            Ok(asset)
        }
        fn play(
            &self,
            _: &LiveWallpaperAsset,
            _: PlaybackOptions,
            _: &mut dyn FnMut(Result<MediaFrame>) -> bool,
            _: Arc<dyn StopSignal>,
        ) {
            panic!("preview cannot start background playback");
        }
        fn preview(
            &self,
            _: &Wallpaper,
            _: WallpaperPreviewOptions,
            emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
            _: Arc<dyn StopSignal>,
        ) {
            for value in [40, 80] {
                if !emit(MediaFrame::rgba(
                    1,
                    1,
                    vec![value, 0, 0, 255],
                    std::time::Duration::ZERO,
                )) {
                    break;
                }
            }
        }
    }
    let mut controller = LiveWallpaper::new(
        Arc::new(Frames),
        Arc::new(Posters(Arc::new(
            ImageData::from_rgba("lifetime-poster", 1, 1, vec![255; 4]).unwrap(),
        ))),
    );
    let mut effects = Effects::default();
    controller.update(Message::Open, &mut effects);
    controller.update(Message::Frame(0.), &mut effects);
    controller.update(
        Message::LibraryLoaded(controller.library_revision, Ok(sources())),
        &mut effects,
    );
    effects.tasks.clear();
    let apply = control(&mut controller, "live-apply", true);
    controller.update(controller.select_message(0), &mut effects);
    for subscription in controller.preview.subscriptions() {
        let (sender, receiver) = std::sync::mpsc::channel();
        (subscription.run)(
            lucent_api::Emitter::new(move |message| {
                sender.send(message).unwrap();
            }),
            Cancellation::default(),
        );
        for message in receiver.try_iter() {
            controller.update(Message::Preview(message), &mut effects);
        }
    }
    assert!(controller.preview.frame_for(&sources()[0].path).is_some());
    controller.update(
        Message::PreviewViewport(800., 600., lucent_api::Density::ONE),
        &mut effects,
    );
    controller.update(Message::Frame(2.), &mut effects);
    assert!(matches!(
        controller.update(apply, &mut effects),
        Some(Intent::ImportStarted(_, _))
    ));
}

#[test]
fn closing_and_reopening_retires_rendered_form_submit_and_close_callbacks() {
    for reopen in [false, true] {
        let mut controller = ready();
        controller.path = "/fixture/from-form.mp4".into();
        let import = control(&mut controller, "live-import", true);
        let close = control(&mut controller, "live-close", true);
        let apply = control(&mut controller, "live-apply", true);
        let mut effects = Effects::default();
        controller.update(Message::Close, &mut effects);
        if reopen {
            controller.update(Message::Open, &mut effects);
            controller.update(
                Message::LibraryLoaded(controller.library_revision, Ok(sources())),
                &mut effects,
            );
        }
        effects.tasks.clear();
        controller.error = "Preserve current feedback".into();
        for stale in [import, close, apply] {
            controller.update(stale, &mut effects);
        }
        assert_eq!(controller.open, reopen);
        assert!(controller.importing.is_none());
        assert!(effects.tasks.is_empty());
        assert_eq!(controller.error, "Preserve current feedback");
        if !reopen {
            controller.update(Message::Import(LiveWallpaperTarget::Default), &mut effects);
            controller.update(
                Message::EditPath(lucent_api::TextEdit::Insert("late".into())),
                &mut effects,
            );
            controller.update(
                Message::Target(LiveWallpaperTarget::Monitor(
                    MonitorId::new("DEMO-1").unwrap(),
                )),
                &mut effects,
            );
            controller.update(
                Message::Scroll(lucent_api::ScrollEvent { offset: 900. }),
                &mut effects,
            );
            assert_eq!(controller.path, "/fixture/from-form.mp4");
            assert_eq!(controller.target, LiveWallpaperTarget::Default);
            assert!(controller.importing.is_none());
            assert!(effects.tasks.is_empty());
        }
    }
}

#[test]
fn retired_path_edit_cannot_change_a_reopened_form() {
    let mut controller = ready();
    let mut input = Interaction::default();
    let mut scene = library_scene(&controller, &LiveWallpapers::default());
    for _ in 0..=scene.hits.len() {
        for message in input.key(&scene, &Key::Tab) {
            controller.update(message, &mut Effects::default());
        }
        scene = library_scene(&controller, &LiveWallpapers::default());
        if input.focus.as_deref() == Some("live-path") {
            break;
        }
    }
    assert_eq!(input.focus.as_deref(), Some("live-path"));
    let edit = input.key(&scene, &Key::Text("late".into())).pop().unwrap();
    controller.update(Message::Close, &mut Effects::default());
    controller.update(Message::Open, &mut Effects::default());
    controller.path = "/fixture/new-form.mp4".into();
    controller.update(edit, &mut Effects::default());
    assert_eq!(controller.path, "/fixture/new-form.mp4");
}

#[test]
fn accepted_import_retains_its_target_and_repeated_apply_cannot_clear_new_feedback() {
    let mut controller = ready();
    let apply = control(&mut controller, "live-apply", false);
    let mut effects = Effects::default();
    let Some(Intent::ImportStarted(target, revision)) =
        controller.update(apply.clone(), &mut effects)
    else {
        panic!("import must be admitted");
    };
    controller.update(
        Message::Target(LiveWallpaperTarget::Monitor(
            MonitorId::new("DEMO-1").unwrap(),
        )),
        &mut effects,
    );
    controller.update(controller.select_message(1), &mut effects);
    controller.error = "New feedback".into();
    controller.update(apply, &mut effects);
    assert_eq!(controller.error, "New feedback");
    assert_eq!(controller.importing.as_ref().unwrap().revision, revision);
    assert_eq!(controller.importing_target(), Some(&target));
    assert_eq!(effects.tasks.len(), 1);
    let completion = effects.tasks.pop().unwrap().run();
    assert!(
        matches!(controller.update(completion, &mut effects), Some(Intent::Apply(captured, profile))
        if captured == target && profile.asset.source == sources()[0])
    );
}

#[test]
fn exhausted_ui_authorities_fail_closed_without_wrapping() {
    let mut controller = ready();
    controller.library_presentation = u64::MAX;
    assert!(matches!(controller.select_message(0), Message::Ignore));
    assert!(matches!(controller.apply_message(), Message::Ignore));
    controller.library_presentation = 1;
    controller.selection_revision = u64::MAX;
    assert!(matches!(controller.apply_message(), Message::Ignore));
    controller.panel_revision = u64::MAX;
    assert!(
        controller
            .admit(Message::Panel(u64::MAX, Box::new(Message::Close)))
            .is_none()
    );
}
