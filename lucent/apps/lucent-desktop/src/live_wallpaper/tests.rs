use super::*;
#[test]
fn mailbox_keeps_only_latest_frame_and_one_pending_wake() {
    let stream = ImageStream::new();
    let mut mailbox = Mailbox::default();
    for n in 0..1000 {
        let frame = Arc::new(stream.frame_rgba(1, 1, vec![n as u8, 0, 0, 255]).unwrap());
        assert_eq!(mailbox.publish(Delivery::Frame(frame.into())), n == 0);
    }
    let Some(Delivery::Frame(image)) = mailbox.take() else {
        panic!("missing latest frame")
    };
    assert_eq!(image.as_image().unwrap().pixels()[0], 999u16 as u8);
    assert!(!mailbox.notified);
    assert!(mailbox.latest.is_none());
    assert!(mailbox.publish(Delivery::Error("fixture".into())));
}

#[test]
fn rejected_import_retires_its_ticket_and_closing_cancels_owned_work() {
    let mut controller = controller();
    let mut effects = Effects::default();
    controller.update(
        Message::ImportPath(LiveWallpaperTarget::Default, "/fixture/clip.mp4".into()),
        &mut effects,
    );
    let mut task = effects.tasks.pop().unwrap();
    let rejected = task.take_failure_handler().unwrap()(lucent_api::RuntimeFailure {
        source: "appearance queue".into(),
        reason: "not admitted".into(),
    });
    controller.update(rejected, &mut effects);
    assert!(controller.importing.is_none());
    assert!(controller.error.contains("not admitted"));
    controller.update(
        Message::ImportPath(LiveWallpaperTarget::Default, "/fixture/clip.mp4".into()),
        &mut effects,
    );
    let cancellation = effects.tasks.pop().unwrap().cancellation;
    controller.update(Message::Close, &mut effects);
    assert!(cancellation.cancelled());
    assert!(controller.importing.is_none());
}

fn setting(path: &str) -> LiveWallpaperSettings {
    LiveWallpaperSettings {
        asset: LiveWallpaperAsset {
            source: Wallpaper {
                path: path.into(),
                name: "Fixture".into(),
            },
            path: path.into(),
            poster: Wallpaper {
                path: "/fixture/poster.png".into(),
                name: "Poster".into(),
            },
        },
        fps: WallpaperFrameRate::default(),
        paused: false,
        enabled: true,
    }
}
fn controller() -> LiveWallpaper {
    let adapters = crate::test_adapters::adapters();
    LiveWallpaper::new(adapters.playback, adapters.assets)
}
#[test]
fn library_refresh_is_worker_owned_and_retires_stale_and_closed_deliveries() {
    let mut controller = controller();
    let mut effects = Effects::default();
    controller.update(Message::Open, &mut effects);
    assert!(controller.library_loading);
    let first = effects.tasks.pop().unwrap();
    assert_eq!(first.queue, crate::work::APPEARANCE);
    let first_revision = controller.library_revision;
    controller.update(Message::RefreshLibrary, &mut effects);
    assert!(first.cancellation.cancelled());
    let second = effects.tasks.pop().unwrap();
    let revision = controller.library_revision;
    let source = Wallpaper {
        path: "/media/moon-lake.mp4".into(),
        name: "Moon lake".into(),
    };
    controller.update(
        Message::LibraryLoaded(first_revision, Ok(vec![source.clone()])),
        &mut effects,
    );
    assert!(controller.library_loading);
    assert!(controller.library.is_empty());
    controller.update(
        Message::LibraryLoaded(revision, Ok(vec![source.clone()])),
        &mut effects,
    );
    assert!(!controller.library_loading);
    assert_eq!(controller.library.as_slice(), std::slice::from_ref(&source));
    controller.update(Message::RefreshLibrary, &mut effects);
    let pending = effects.tasks.pop().unwrap();
    let revision = controller.library_revision;
    controller.update(Message::Close, &mut effects);
    assert!(pending.cancellation.cancelled());
    controller.update(Message::LibraryLoaded(revision, Ok(vec![])), &mut effects);
    assert_eq!(controller.library, [source]);
    assert!(!controller.library_loading);
    controller.update(second.run(), &mut effects);
    assert_eq!(controller.library.len(), 1);
}

#[test]
fn library_task_rejection_keeps_last_good_sources_and_explicit_refresh_recovers() {
    let mut controller = controller();
    let mut effects = Effects::default();
    controller.update(Message::Open, &mut effects);
    let mut pending = effects.tasks.pop().unwrap();
    controller.library.push(Wallpaper {
        path: "/media/moon-lake.mp4".into(),
        name: "Moon lake".into(),
    });
    let rejected = pending.take_failure_handler().unwrap()(lucent_api::RuntimeFailure {
        source: "appearance queue".into(),
        reason: "not admitted".into(),
    });
    controller.update(rejected, &mut effects);
    assert!(!controller.library_loading);
    assert!(controller.library_error.contains("not admitted"));
    assert_eq!(controller.library.len(), 1);
    controller.update(Message::RefreshLibrary, &mut effects);
    let pending = effects.tasks.pop().unwrap();
    controller.update(pending.run(), &mut effects);
    assert!(!controller.library_loading);
    assert!(controller.library_error.is_empty());
    assert!(controller.library.is_empty());
}

fn library_scene(
    controller: &LiveWallpaper,
    settings: &LiveWallpapers,
) -> lucent_ui::Scene<Message> {
    use lucent_api::Density;
    use lucent_ui::{Interaction, Layout};
    let font = lucent_ui::text::Font::from_bytes(
        include_bytes!("../../assets/LucentSans.ttf") as &[u8],
        Default::default(),
    )
    .unwrap();
    let context = lucent_api::ViewContext {
        surface: "live-wallpaper",
        instance: None,
        width: 960.,
        height: 800.,
        now: 1.,
        density: Density::ONE,
    };
    let theme = lucent_design::Theme::new(false);
    let tree = theme.apply(controller.view(
        &context,
        settings,
        &[],
        crate::artwork::Artwork {
            images: &std::collections::BTreeMap::new(),
            theme,
        },
        SaveState {
            status: crate::persistence::Status::Saved,
            error: "",
            writable: true,
        },
    ));
    Layout::new(Arc::new(vec![font])).build(&tree, 960., 800., &Interaction::default(), 1.)
}

#[test]
fn library_card_previews_and_explicit_apply_uses_existing_validated_import_for_its_target() {
    use lucent_ui::Interaction;
    let mut controller = controller();
    controller.assets = Arc::new(Posters(Arc::new(
        ImageData::from_rgba("library-poster", 1, 1, vec![255; 4]).unwrap(),
    )));
    let mut effects = Effects::default();
    controller.update(Message::Open, &mut effects);
    controller.update(Message::Frame(0.), &mut effects);
    let target = LiveWallpaperTarget::Monitor(MonitorId::new("DEMO-1").unwrap());
    controller.target = target.clone();
    let source = Wallpaper {
        path: "/media/moon-lake.mp4".into(),
        name: "Moon lake".into(),
    };
    controller.update(
        Message::LibraryLoaded(controller.library_revision, Ok(vec![source.clone()])),
        &mut effects,
    );
    let mut input = Interaction::default();
    let scene = library_scene(&controller, &LiveWallpapers::default());
    let hit = scene
        .hits
        .iter()
        .find(|hit| hit.id == "live-library-0")
        .unwrap();
    let (x, y) = (hit.rect.x + hit.rect.w / 2., hit.rect.y + hit.rect.h / 2.);
    assert!(
        hit.contains(x, y),
        "the library option must be visible on opening"
    );
    assert!(input.press(&scene, x, y).is_none());
    let message = input.release(x, y).unwrap();
    assert!(matches!(&message, Message::SelectSource(0)));
    let pending_before = effects.tasks.len();
    assert!(controller.update(message, &mut effects).is_none());
    assert!(controller.importing.is_none());
    assert_eq!(
        effects.tasks.len(),
        pending_before,
        "card selection cannot import or save"
    );
    let scene = library_scene(&controller, &LiveWallpapers::default());
    let apply = scene
        .hits
        .iter()
        .find(|hit| hit.id == "live-apply")
        .unwrap();
    let (x, y) = (
        apply.rect.x + apply.rect.w / 2.,
        apply.rect.y + apply.rect.h / 2.,
    );
    assert!(input.press(&scene, x, y).is_none());
    let message = input.release(x, y).unwrap();
    assert!(matches!(message, Message::ApplySelection));
    assert!(
        matches!(controller.update(message, &mut effects), Some(Intent::ImportStarted(captured, _)) if captured == target)
    );
    let prepared = effects.tasks.pop().unwrap().run();
    assert!(matches!(&prepared, Message::Prepared(_, Ok((asset, _))) if asset.source == source));
    let Some(Intent::Apply(captured, profile)) = controller.update(prepared, &mut effects) else {
        panic!("a validated import must apply its captured target");
    };
    assert_eq!(captured, target);
    assert_eq!(profile.fps, WallpaperFrameRate::Source);
}

#[test]
fn preview_navigation_keeps_applied_profile_and_import_busy_failure_stays_separate() {
    use lucent_ui::Paint;
    let mut controller = controller();
    let mut effects = Effects::default();
    controller.update(Message::Open, &mut effects);
    controller.update(Message::Frame(0.), &mut effects);
    let source = Wallpaper {
        path: "/media/moon-lake.mp4".into(),
        name: "Moon lake".into(),
    };
    let other = Wallpaper {
        path: "/media/amber-lake.mp4".into(),
        name: "Amber lake".into(),
    };
    controller.update(
        Message::LibraryLoaded(
            controller.library_revision,
            Ok(vec![source.clone(), other.clone()]),
        ),
        &mut effects,
    );
    effects.tasks.clear();
    let mut profile = setting("/owned/moon-lake.mp4");
    profile.asset.source = source.clone();
    profile.fps = WallpaperFps::new(15).unwrap().into();
    let settings = LiveWallpapers {
        default: Some(profile.clone()),
        ..Default::default()
    };
    assert!(
        controller
            .update(Message::SelectSource(0), &mut effects)
            .is_none()
    );
    assert!(
        controller
            .update(Message::Browse(1), &mut effects)
            .is_none()
    );
    assert_eq!(controller.selected_source(), Some(&other));
    assert!(
        effects.tasks.is_empty(),
        "preview selection declares a subscription, not an import task"
    );
    assert!(controller.importing.is_none());
    assert_eq!(settings.default.as_ref(), Some(&profile));
    let Some(Intent::ImportStarted(_, revision)) =
        controller.update(Message::ApplySelection, &mut effects)
    else {
        panic!("Apply must start the validated import path");
    };
    let busy = library_scene(&controller, &settings);
    assert!(busy.paint.iter().any(|paint| matches!(paint, Paint::Text { text, rect, clip, .. }
        if text == "Importing and decoding…" && rect.y >= clip.bounds.y && rect.y + rect.h <= clip.bounds.y + clip.bounds.h)));
    assert!(
        busy.hits
            .iter()
            .all(|hit| hit.id != "live-apply" || !hit.tab_stop)
    );
    let task_count = effects.tasks.len();
    assert!(
        controller
            .update(Message::ApplySelection, &mut effects)
            .is_none()
    );
    assert!(
        controller
            .update(Message::SelectSource(0), &mut effects)
            .is_none()
    );
    assert_eq!(controller.importing.as_ref().unwrap().revision, revision);
    assert_eq!(
        effects.tasks.len(),
        task_count,
        "navigation cannot replace an admitted import"
    );
    controller.update(
        Message::Prepared(
            revision,
            Err(DomainError::Failed("fixture decode failed".into())),
        ),
        &mut effects,
    );
    let failed = library_scene(&controller, &settings);
    assert!(failed.paint.iter().any(
        |paint| matches!(paint, Paint::Text { text, .. } if text.contains("fixture decode failed"))
    ));
    assert!(
        failed
            .hits
            .iter()
            .any(|hit| hit.id == "live-apply" && hit.tab_stop)
    );
    assert_eq!(settings.default.as_ref(), Some(&profile));
}

#[test]
fn refreshed_library_preserves_selected_path_and_browse_clamps_without_applying() {
    let mut controller = controller();
    let mut effects = Effects::default();
    controller.path = "/manual-input.mp4".into();
    controller.update(Message::Open, &mut effects);
    let sources: Vec<_> = (0..3)
        .map(|index| Wallpaper {
            path: format!("/fixture/clip{index}.mp4"),
            name: format!("Clip {index}"),
        })
        .collect();
    controller.update(
        Message::LibraryLoaded(controller.library_revision, Ok(sources.clone())),
        &mut effects,
    );
    effects.tasks.clear();
    assert!(
        controller
            .update(Message::Browse(i32::MAX), &mut effects)
            .is_none()
    );
    assert_eq!(controller.selection, 2);
    assert_eq!(controller.carousel.target_value(), 2.);
    assert!(controller.preview.inspect()["selected"].as_bool().unwrap());
    assert!(controller.importing.is_none());
    assert!(effects.tasks.is_empty());
    assert_eq!(
        controller.path, "/manual-input.mp4",
        "carousel does not rewrite the controlled path field"
    );
    controller.update(Message::RefreshLibrary, &mut effects);
    let selected = sources[2].clone();
    controller.update(
        Message::LibraryLoaded(
            controller.library_revision,
            Ok(vec![selected.clone(), sources[0].clone()]),
        ),
        &mut effects,
    );
    assert_eq!(controller.selection, 0);
    assert_eq!(controller.selected_source(), Some(&selected));
    assert_eq!(controller.carousel.target_value(), 0.);
    controller.update(Message::Browse(i32::MIN), &mut effects);
    assert_eq!(controller.selection, 0);
    controller.update(Message::SelectSource(usize::MAX), &mut effects);
    assert_eq!(
        controller.selection, 0,
        "stale card identities cannot retarget a missing index"
    );
    controller.update(Message::Close, &mut effects);
    assert_eq!(controller.preview.retained_bytes(), 0);
    controller.update(Message::Browse(1), &mut effects);
    controller.update(Message::ApplySelection, &mut effects);
    assert!(
        controller.importing.is_none(),
        "closed preview actions cannot start imports"
    );
}

#[test]
fn refresh_invalidates_same_path_pixels_failures_and_waits_for_fresh_discovery_and_decoder_ack() {
    use std::sync::{
        atomic::{AtomicU8, AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::Duration;
    struct Call {
        stop: Arc<dyn StopSignal>,
        release: mpsc::Sender<()>,
    }
    struct Adapter {
        marker: AtomicU8,
        mode: AtomicU8,
        calls: AtomicUsize,
        started: mpsc::Sender<Call>,
    }
    impl WallpaperPlaybackPort for Adapter {
        fn prepare(&self, _: &Wallpaper) -> Result<LiveWallpaperAsset> {
            panic!("Refresh cannot import");
        }
        fn play(
            &self,
            _: &LiveWallpaperAsset,
            _: PlaybackOptions,
            _: &mut dyn FnMut(Result<MediaFrame>) -> bool,
            _: Arc<dyn StopSignal>,
        ) {
            panic!("Refresh cannot apply");
        }
        fn preview(
            &self,
            _: &Wallpaper,
            _: WallpaperPreviewOptions,
            emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
            stop: Arc<dyn StopSignal>,
        ) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !emit(MediaFrame::rgba(
                1,
                1,
                vec![self.marker.load(Ordering::SeqCst), 0, 0, 255],
                Duration::ZERO,
            )) {
                return;
            }
            match self.mode.load(Ordering::SeqCst) {
                0 => {
                    emit(Err(DomainError::Unavailable(
                        "owned old-file failure".into(),
                    )));
                }
                2 => {
                    let (release, wait) = mpsc::channel();
                    self.started.send(Call { stop, release }).unwrap();
                    // Delay native teardown after cancellation to prove that a
                    // fresh LibraryLoaded is not itself a decoder retirement ACK.
                    let _ = wait.recv_timeout(Duration::from_secs(2));
                }
                _ => (),
            }
        }
    }
    let (started, calls) = mpsc::channel();
    let adapter = Arc::new(Adapter {
        marker: AtomicU8::new(11),
        mode: AtomicU8::new(0),
        calls: AtomicUsize::new(0),
        started,
    });
    let mut controller =
        LiveWallpaper::new(adapter.clone(), crate::test_adapters::adapters().assets);
    let mut effects = Effects::default();
    let source = Wallpaper {
        path: "/owned-fixture/same-path.mp4".into(),
        name: "Same path".into(),
    };
    let finite = |controller: &mut LiveWallpaper| {
        let (send, receive) = mpsc::channel();
        (controller.subscriptions().pop().unwrap().run)(
            lucent_api::Emitter::new(move |message| {
                send.send(message).unwrap();
            }),
            Cancellation::default(),
        );
        for message in receive.try_iter() {
            controller.update(message, &mut Effects::default());
        }
    };
    controller.update(Message::Open, &mut effects);
    assert!(
        controller.subscriptions().is_empty(),
        "Open waits for discovery before declaring preview work"
    );
    controller.update(
        Message::LibraryLoaded(controller.library_revision, Ok(vec![source.clone()])),
        &mut effects,
    );
    finite(&mut controller);
    assert_eq!(controller.preview.status(), preview::Status::Failed);
    assert_eq!(controller.preview.frame().unwrap().pixels()[0], 11);
    assert_eq!(controller.preview.inspect()["cached"], 1);

    adapter.marker.store(44, Ordering::SeqCst);
    adapter.mode.store(2, Ordering::SeqCst);
    controller.update(Message::RefreshLibrary, &mut effects);
    let stale_library = controller.library_revision;
    assert_eq!(controller.preview.status(), preview::Status::Idle);
    assert!(controller.preview.error().is_empty());
    assert_eq!(controller.preview.inspect()["cached"], 0);
    assert_eq!(controller.preview.retained_bytes(), 0);
    for message in [
        Message::Browse(0),
        Message::SelectSource(0),
        Message::RetryPreview,
    ] {
        controller.update(message, &mut effects);
        assert!(
            controller.subscriptions().is_empty(),
            "pending discovery cannot restart a known old path"
        );
    }
    controller.update(Message::RefreshLibrary, &mut effects);
    controller.update(
        Message::LibraryLoaded(stale_library, Ok(vec![source.clone()])),
        &mut effects,
    );
    assert!(controller.library_loading);
    assert!(controller.subscriptions().is_empty());
    controller.update(
        Message::LibraryLoaded(controller.library_revision, Ok(vec![source.clone()])),
        &mut effects,
    );
    let subscription = controller.subscriptions().pop().unwrap();
    let retiring_id = subscription.id.clone();
    let generation: u64 = retiring_id
        .strip_prefix("live-wallpaper-preview:")
        .unwrap()
        .parse()
        .unwrap();
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        (subscription.run)(
            lucent_api::Emitter::new(move |message| {
                send.send(message).unwrap();
            }),
            Cancellation::default(),
        );
    });
    let running = calls.recv_timeout(Duration::from_secs(2)).unwrap();
    controller.update(
        receive.recv_timeout(Duration::from_secs(2)).unwrap(),
        &mut effects,
    );
    assert_eq!(controller.preview.frame().unwrap().pixels()[0], 44);

    adapter.marker.store(77, Ordering::SeqCst);
    adapter.mode.store(1, Ordering::SeqCst);
    controller.update(Message::RefreshLibrary, &mut effects);
    assert!(running.stop.cancelled());
    assert_eq!(controller.preview.retained_bytes(), 0);
    assert_eq!(controller.preview.inspect()["cached"], 0);
    controller.update(Message::Browse(0), &mut effects);
    assert_eq!(controller.preview.inspect()["selected"], false);
    assert_eq!(controller.subscriptions()[0].id, retiring_id);
    controller.update(
        Message::LibraryLoaded(controller.library_revision, Ok(vec![source])),
        &mut effects,
    );
    assert_eq!(
        controller.subscriptions()[0].id,
        retiring_id,
        "same-path replacement waits for the old decoder to return"
    );
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 2);
    running.release.send(()).unwrap();
    let ended = receive.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        ended,
        Message::Preview(preview::Message::Ended(_, _))
    ));
    worker.join().unwrap();
    controller.update(ended, &mut effects);
    assert_ne!(controller.subscriptions()[0].id, retiring_id);
    controller.update(
        Message::Preview(preview::Message::Wake(generation)),
        &mut effects,
    );
    assert!(
        controller.preview.frame().is_none(),
        "late old-file wakes cannot flash old pixels"
    );
    finite(&mut controller);
    assert_eq!(controller.preview.frame().unwrap().pixels()[0], 77);
    assert_eq!(controller.preview.status(), preview::Status::Ready);
    assert!(controller.preview.error().is_empty());
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 3);
    assert!(controller.importing.is_none());
    controller.update(Message::Close, &mut effects);
    assert!(controller.subscriptions().is_empty());
    assert_eq!(controller.preview.retained_bytes(), 0);
}

#[test]
fn preview_viewport_uses_shared_hero_geometry_and_real_density_with_a_checked_cap() {
    let mut controller = controller();
    let mut effects = Effects::default();
    let viewport = (960., 800.);
    controller.update(
        Message::PreviewViewport(viewport.0, viewport.1, lucent_api::Density::ONE),
        &mut effects,
    );
    let logical = super::view::preview_size(viewport);
    let one = controller.preview_options.size();
    assert_eq!(one, (logical.0.ceil() as u32, logical.1.ceil() as u32));
    assert!(
        controller.subscriptions().is_empty(),
        "closed resize cannot start a decoder"
    );
    controller.update(
        Message::PreviewViewport(
            viewport.0,
            viewport.1,
            lucent_api::Density::new(5, 4).unwrap(),
        ),
        &mut effects,
    );
    assert_eq!(
        controller.preview_options.size(),
        (
            (logical.0 * 1.25).ceil() as u32,
            (logical.1 * 1.25).ceil() as u32
        )
    );
    controller.update(
        Message::PreviewViewport(
            viewport.0,
            viewport.1,
            lucent_api::Density::integer(8).unwrap(),
        ),
        &mut effects,
    );
    let bounded = controller.preview_options.size();
    assert_eq!(bounded.0.max(bounded.1), WallpaperPreviewOptions::MAX_EDGE);
    assert!(bounded.0 > 0 && bounded.1 > 0);
    controller.update(
        Message::PreviewViewport(f32::NAN, viewport.1, lucent_api::Density::ONE),
        &mut effects,
    );
    assert_eq!(controller.preview_options.size(), bounded);
    assert_eq!(controller.preview.retained_bytes(), 0);
    assert!(effects.tasks.is_empty());
}

#[test]
fn root_blocks_unwritable_preview_apply_and_routes_only_exact_preview_failures() {
    use crate::desktop::{Desktop, Message as DesktopMessage};
    use lucent_api::{Application, Component, RuntimeFailure};
    let mut app = Desktop::new(crate::test_adapters::adapters());
    let mut effects = Effects::default();
    app.live_wallpaper.open = true;
    app.live_wallpaper.library = vec![Wallpaper {
        path: "/fixture/clip.mp4".into(),
        name: "Fixture".into(),
    }];
    app.settings_writable = false;
    let before = serde_json::to_value(&app.settings).unwrap();
    app.update(
        DesktopMessage::LiveWallpaper(Message::ApplySelection),
        &mut effects,
    );
    assert!(app.live_wallpaper.importing.is_none());
    assert!(app.live_wallpaper.error.contains("not writable"));
    assert_eq!(serde_json::to_value(&app.settings).unwrap(), before);
    assert!(effects.tasks.is_empty());
    let failure = app.runtime_failure(RuntimeFailure {
        source: "subscription live-wallpaper-preview:17".into(),
        reason: "fixture".into(),
    });
    assert!(matches!(
        failure,
        Some(DesktopMessage::LiveWallpaper(Message::Preview(
            preview::Message::WorkerFailed(17)
        )))
    ));
    assert!(matches!(
        app.runtime_failure(RuntimeFailure {
            source: "subscription live-wallpaper-preview:17-extra".into(),
            reason: "fixture".into(),
        }),
        Some(DesktopMessage::Completed(Err(_)))
    ));
    assert!(matches!(
        app.runtime_failure(RuntimeFailure {
            source: "subscription live-wallpaper:17".into(),
            reason: "fixture".into(),
        }),
        Some(DesktopMessage::LiveWallpaper(Message::WorkerFailed(17, _)))
    ));
}

#[test]
fn selector_reveal_waits_for_its_first_submission_then_keeps_the_full_duration() {
    let mut controller = controller();
    let mut effects = Effects {
        now: 5.,
        ..Effects::default()
    };
    controller.update(Message::Open, &mut effects);
    assert_eq!(controller.awaiting_first_frame, Some(5.));
    assert_eq!(controller.motion.value(100.), 0.);
    effects.redraw.clear();
    effects.now = 100.;
    controller.update(Message::Frame(4.), &mut effects);
    assert_eq!(controller.awaiting_first_frame, Some(5.));
    assert!(
        effects.redraw.is_empty(),
        "a stale submission cannot start this reveal"
    );
    controller.update(Message::Frame(5.), &mut effects);
    assert!(controller.awaiting_first_frame.is_none());
    assert!(effects.redraw.contains("live-wallpaper"));
    assert_eq!(controller.motion.value(100.), 0.);
    let duration = lucent_design::motion::WALLPAPER_REVEAL;
    let middle = controller.motion.value(100. + duration / 2.);
    assert!(middle > 0. && middle < 1.);
    assert!(controller.motion.active(100. + duration / 2.));
    assert_eq!(controller.motion.value(100. + duration), 1.);
    assert!(!controller.motion.active(100. + duration));
    effects.redraw.clear();
    controller.update(Message::Frame(100. + duration), &mut effects);
    assert!(
        effects.redraw.is_empty(),
        "a settled selector must not repaint continuously"
    );
}

#[test]
fn closing_before_the_first_frame_retires_the_gate_and_late_frames_cannot_reopen_it() {
    let mut controller = controller();
    let mut effects = Effects::default();
    controller.update(Message::Open, &mut effects);
    controller.update(Message::Close, &mut effects);
    assert!(controller.awaiting_first_frame.is_none());
    effects.now = 10.;
    controller.update(Message::Frame(0.), &mut effects);
    assert!(!controller.mounted);
    assert!(!controller.open);
    assert_eq!(controller.motion.target_value(), 0.);
    controller.update(Message::Open, &mut effects);
    assert_eq!(controller.awaiting_first_frame, Some(10.));
    controller.update(Message::Frame(1.), &mut effects);
    assert_eq!(controller.awaiting_first_frame, Some(10.));
    assert_eq!(controller.motion.value(20.), 0.);
}

#[test]
fn unseen_mounted_reopen_waits_for_a_new_submission_instead_of_consuming_its_reveal() {
    let mut controller = controller();
    let mut effects = Effects {
        now: 10.,
        ..Effects::default()
    };
    controller.update(Message::Open, &mut effects);
    controller.update(Message::Close, &mut effects);
    assert!(controller.mounted);
    assert!(!controller.first_frame_submitted);
    effects.now = 12.;
    controller.update(Message::Open, &mut effects);
    assert_eq!(controller.awaiting_first_frame, Some(12.));
    assert_eq!(controller.motion.value(100.), 0.);
    effects.now = 100.;
    controller.update(Message::Frame(10.), &mut effects);
    assert_eq!(controller.awaiting_first_frame, Some(12.));
    controller.update(Message::Frame(12.), &mut effects);
    assert!(controller.first_frame_submitted);
    assert!(controller.awaiting_first_frame.is_none());
    assert_eq!(controller.motion.value(100.), 0.);
    assert_eq!(
        controller
            .motion
            .value(100. + lucent_design::motion::WALLPAPER_REVEAL),
        1.
    );
}

#[test]
fn mounted_selector_reverses_without_a_jump_and_repeated_open_preserves_progress() {
    let mut controller = controller();
    let mut effects = Effects::default();
    controller.update(Message::Open, &mut effects);
    controller.update(Message::Frame(0.), &mut effects);
    let duration = lucent_design::motion::WALLPAPER_REVEAL;
    effects.now = duration / 2.;
    let opened = controller.motion.value(effects.now);
    controller.update(Message::Close, &mut effects);
    assert_eq!(controller.motion.value(effects.now), opened);
    effects.now += duration / 4.;
    let closing = controller.motion.value(effects.now);
    assert!(closing > 0. && closing < opened);
    controller.update(Message::Open, &mut effects);
    assert!(controller.awaiting_first_frame.is_none());
    assert_eq!(controller.motion.value(effects.now), closing);
    let revision = controller.library_revision;
    let task_count = effects.tasks.len();
    effects.now += duration / 4.;
    let opening = controller.motion.value(effects.now);
    assert!(opening > closing && opening < 1.);
    controller.scroll = 64.;
    controller.update(Message::Open, &mut effects);
    assert_eq!(controller.motion.value(effects.now), opening);
    assert_eq!(controller.library_revision, revision);
    assert_eq!(effects.tasks.len(), task_count);
    assert_eq!(controller.scroll, 64.);
}

#[test]
fn selector_close_cleanup_uses_the_time_of_the_rendered_view() {
    let mut controller = controller();
    let mut effects = Effects::default();
    controller.update(Message::Open, &mut effects);
    controller.update(Message::Frame(0.), &mut effects);
    let duration = lucent_design::motion::WALLPAPER_REVEAL;
    effects.now = duration;
    controller.update(Message::Close, &mut effects);
    effects.now = 100.;
    controller.update(Message::Frame(duration * 1.5), &mut effects);
    assert!(
        controller.mounted,
        "a slowly submitted partial close must retain its surface"
    );
    controller.update(Message::Frame(duration * 2.), &mut effects);
    assert!(!controller.mounted);
}

#[test]
fn shared_streams_pause_without_workers_and_hotplug_rejects_old_frames() {
    let mut controller = controller();
    let id = MonitorId::new("DEMO-1").unwrap();
    let other = MonitorId::new("DEMO-2").unwrap();
    let mut settings = LiveWallpapers {
        default: Some(setting("/fixture/default.mp4")),
        ..Default::default()
    };
    controller.reconcile(
        &settings,
        &[id.clone(), other.clone()],
        true,
        &WallpaperPlaybackContext::default(),
        &BTreeMap::new(),
    );
    assert_eq!(controller.players.len(), 1);
    assert_eq!(controller.subscriptions().len(), 1);
    let key = controller.routes[&LiveWallpaperTarget::Monitor(id.clone())]
        .key
        .clone();
    let revision = controller.players[&key].revision;
    let stream = ImageStream::new();
    let image = Arc::new(stream.frame_rgba(1, 1, vec![255, 0, 0, 255]).unwrap());
    controller.players[&key]
        .mailbox
        .lock()
        .unwrap()
        .publish(Delivery::Frame(image.into()));
    let mut effects = Effects::default();
    assert!(
        matches!(controller.update(Message::Wake(key.clone(),revision),&mut effects),Some(Intent::FrameChanged(targets)) if targets.len()==2)
    );
    assert!(controller.frame(Some(&id)).is_some());
    settings.paused = true;
    controller.reconcile(
        &settings,
        &[id.clone(), other.clone()],
        true,
        &WallpaperPlaybackContext::default(),
        &BTreeMap::new(),
    );
    assert!(controller.subscriptions().is_empty());
    assert!(controller.frame(Some(&id)).is_some());
    assert!(
        controller
            .update(Message::Wake(key.clone(), revision), &mut effects)
            .is_none()
    );
    settings.paused = false;
    controller.reconcile(
        &settings,
        std::slice::from_ref(&id),
        true,
        &WallpaperPlaybackContext::default(),
        &BTreeMap::new(),
    );
    assert_eq!(controller.subscriptions().len(), 1);
    assert!(controller.frame(Some(&other)).is_none());
    controller.reconcile(
        &settings,
        &[],
        true,
        &WallpaperPlaybackContext::default(),
        &BTreeMap::new(),
    );
    assert!(controller.subscriptions().is_empty());
    assert!(controller.players.is_empty());
}

fn pixel_target(width: u32, height: u32) -> WallpaperPixelTarget {
    WallpaperPixelTarget::new(width, height).unwrap()
}

#[derive(Default)]
struct FittedPlayback(Mutex<Vec<PlaybackOptions>>);
impl WallpaperPlaybackPort for FittedPlayback {
    fn prepare(&self, _: &Wallpaper) -> Result<LiveWallpaperAsset> {
        Err(DomainError::Unavailable("Unused fixture import".into()))
    }
    fn play(
        &self,
        _: &LiveWallpaperAsset,
        options: PlaybackOptions,
        emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
        _: Arc<dyn StopSignal>,
    ) {
        self.0.lock().unwrap().push(options);
        let (width, height) = options
            .target
            .map_or(Ok((17, 13)), |target| target.cover_size(17, 13))
            .unwrap();
        emit(MediaFrame::rgba_with_aspect(
            width,
            height,
            [80, 120, 160, 255].repeat(width as usize * height as usize),
            (17, 13),
            std::time::Duration::from_millis(125),
        ));
    }
}

fn deliver_subscription(controller: &mut LiveWallpaper, subscription: Subscription<Message>) {
    let (sender, receiver) = std::sync::mpsc::channel();
    (subscription.run)(
        lucent_api::Emitter::new(move |message| {
            sender.send(message).unwrap();
        }),
        Cancellation::default(),
    );
    let messages: Vec<_> = receiver.try_iter().collect();
    assert_eq!(
        messages.len(),
        1,
        "one latest-frame wake, never a pixel message"
    );
    for message in messages {
        controller.update(message, &mut Effects::default());
    }
}

#[test]
fn connected_consumers_merge_cover_demand_only_within_their_shared_profile() {
    let mut controller = controller();
    let left = MonitorId::new("DP-1").unwrap();
    let right = MonitorId::new("DP-2").unwrap();
    let other = MonitorId::new("DP-3").unwrap();
    let left_target = LiveWallpaperTarget::Monitor(left.clone());
    let right_target = LiveWallpaperTarget::Monitor(right.clone());
    let other_target = LiveWallpaperTarget::Monitor(other.clone());
    let monitors = [left, right.clone(), other.clone()];
    let settings = LiveWallpapers {
        default: Some(setting("/fixture/shared.mp4")),
        monitors: BTreeMap::from([(other, setting("/fixture/independent.mp4"))]),
        ..Default::default()
    };
    let mut targets = BTreeMap::from([
        (left_target.clone(), pixel_target(1920, 1080)),
        (right_target.clone(), pixel_target(1080, 1920)),
        (other_target.clone(), pixel_target(3840, 2160)),
    ]);
    let mut context = WallpaperPlaybackContext::default();
    controller.reconcile(&settings, &monitors, true, &context, &targets);
    let key = controller.routes[&left_target].key.clone();
    let independent = controller.routes[&other_target].key.clone();
    assert_eq!(controller.routes[&right_target].key, key);
    assert_eq!(controller.players.len(), 2);
    assert_eq!(controller.subscriptions().len(), 2);
    assert_eq!(
        controller.players[&key].target,
        Some(pixel_target(1920, 1920))
    );
    assert_eq!(
        controller.players[&independent].target,
        Some(pixel_target(3840, 2160))
    );
    assert_eq!(controller.players[&key].fps, WallpaperFrameRate::Source);
    let revision = controller.players[&key].revision;
    context.fullscreen_monitors.insert(right);
    controller.reconcile(&settings, &monitors, true, &context, &targets);
    assert_eq!(
        controller.players[&key].target,
        Some(pixel_target(1920, 1920))
    );
    assert_eq!(controller.players[&key].revision, revision);
    assert!(controller.players[&key].running);
    targets.remove(&right_target);
    controller.reconcile(&settings, &monitors, true, &context, &targets);
    assert_eq!(
        controller.players[&key].target, None,
        "unknown consumer geometry keeps source detail"
    );
    assert_eq!(
        controller.players[&independent].target,
        Some(pixel_target(3840, 2160))
    );
}

#[test]
fn pixel_retarget_retires_mailbox_and_old_wakes_while_retaining_last_good_pixels() {
    let adapter = Arc::new(FittedPlayback::default());
    let mut controller =
        LiveWallpaper::new(adapter.clone(), crate::test_adapters::adapters().assets);
    let settings = LiveWallpapers {
        default: Some(setting("/fixture/resizing.mp4")),
        ..Default::default()
    };
    let mut targets = BTreeMap::from([(LiveWallpaperTarget::Default, pixel_target(8, 6))]);
    let context = WallpaperPlaybackContext::default();
    controller.reconcile(&settings, &[], false, &context, &targets);
    let first = controller.subscriptions().pop().unwrap();
    deliver_subscription(&mut controller, first);
    let previous = controller.frame(None).unwrap().clone();
    assert_eq!((previous.width(), previous.height()), (8, 7));
    assert_eq!(previous.aspect_size(), (17, 13));
    let key = controller.routes[&LiveWallpaperTarget::Default].key.clone();
    let old_revision = controller.players[&key].revision;
    let old_mailbox = controller.players[&key].mailbox.clone();
    let late = controller.subscriptions().pop().unwrap();
    targets.insert(LiveWallpaperTarget::Default, pixel_target(16, 12));
    controller.reconcile(&settings, &[], false, &context, &targets);
    let revision = controller.players[&key].revision;
    assert_ne!(revision, old_revision);
    assert!(!Arc::ptr_eq(
        &old_mailbox,
        &controller.players[&key].mailbox
    ));
    assert!(Arc::ptr_eq(controller.frame(None).unwrap(), &previous));
    deliver_subscription(&mut controller, late);
    assert!(
        Arc::ptr_eq(controller.frame(None).unwrap(), &previous),
        "retired worker cannot install pixels"
    );
    let current = controller.subscriptions().pop().unwrap();
    deliver_subscription(&mut controller, current);
    let frame = controller.frame(None).unwrap();
    assert_eq!((frame.width(), frame.height()), (16, 13));
    assert_eq!(frame.aspect_size(), (17, 13));
    assert_eq!(previous.pixels(), [80, 120, 160, 255].repeat(8 * 7));
    assert_eq!(controller.subscriptions().len(), 1);
    controller.reconcile(&settings, &[], false, &context, &targets);
    assert_eq!(
        controller.players[&key].revision, revision,
        "unchanged geometry keeps the producer"
    );
    assert!(
        adapter
            .0
            .lock()
            .unwrap()
            .iter()
            .all(|options| options.fps == WallpaperFrameRate::Source && options.looping)
    );
}

#[test]
fn paused_pixels_stay_frozen_through_growth_and_resume_uses_the_fitted_active_frame() {
    let mut controller = LiveWallpaper::new(
        Arc::new(FittedPlayback::default()),
        crate::test_adapters::adapters().assets,
    );
    let left = MonitorId::new("DP-1").unwrap();
    let right = MonitorId::new("DP-2").unwrap();
    let monitors = [left.clone(), right.clone()];
    let left_target = LiveWallpaperTarget::Monitor(left.clone());
    let right_target = LiveWallpaperTarget::Monitor(right.clone());
    let mut targets = BTreeMap::from([
        (left_target.clone(), pixel_target(8, 6)),
        (right_target, pixel_target(8, 6)),
    ]);
    let settings = LiveWallpapers {
        default: Some(setting("/fixture/frozen.mp4")),
        ..Default::default()
    };
    let mut context = WallpaperPlaybackContext::default();
    controller.reconcile(&settings, &monitors, true, &context, &targets);
    let subscription = controller.subscriptions().pop().unwrap();
    deliver_subscription(&mut controller, subscription);
    context.fullscreen_monitors.insert(left.clone());
    controller.reconcile(&settings, &monitors, true, &context, &targets);
    let held = controller.frame(Some(&left)).unwrap().clone();
    assert!(held.stream_revision().is_none());
    targets.insert(left_target.clone(), pixel_target(16, 12));
    controller.reconcile(&settings, &monitors, true, &context, &targets);
    let key = controller.routes[&left_target].key.clone();
    assert_eq!(controller.players[&key].target, Some(pixel_target(16, 12)));
    assert!(Arc::ptr_eq(controller.frame(Some(&left)).unwrap(), &held));
    let subscription = controller.subscriptions().pop().unwrap();
    deliver_subscription(&mut controller, subscription);
    assert!(Arc::ptr_eq(controller.frame(Some(&left)).unwrap(), &held));
    assert_eq!(held.pixels(), [80, 120, 160, 255].repeat(8 * 7));
    let active = controller.frame(Some(&right)).unwrap().clone();
    assert_eq!((active.width(), active.height()), (16, 13));
    context.fullscreen_monitors.clear();
    controller.reconcile(&settings, &monitors, true, &context, &targets);
    assert!(Arc::ptr_eq(controller.frame(Some(&left)).unwrap(), &active));
}

#[test]
fn desktop_uses_configured_physical_pixels_for_legacy_and_current_monitor_generations() {
    use crate::desktop::{Desktop, Message as DesktopMessage};
    use lucent_api::{Component, Density, DisplayOutput, OutputName};
    let mut app = Desktop::new(crate::test_adapters::adapters());
    app.settings.live_wallpapers.default = Some(setting("/fixture/output-fit.mp4"));
    let settings = app.settings.clone();
    let observed_target = |app: &Desktop| {
        assert_eq!(app.live_wallpaper.players.len(), 1);
        app.live_wallpaper.players.values().next().unwrap().target
    };
    let density = Density::new(150, 120).unwrap();
    app.update(DesktopMessage::Ignore, &mut Effects::default());
    assert_eq!(observed_target(&app), None, "no assumed startup viewport");
    app.update(
        DesktopMessage::Resize("background", 65., 37., density),
        &mut Effects::default(),
    );
    assert_eq!(observed_target(&app), Some(pixel_target(82, 47)));
    let id = MonitorId::new("DP-1").unwrap();
    let output = |global| DisplayOutput {
        global,
        name: Some(OutputName::new("DP-1").unwrap()),
        logical_size: Some((65, 37)),
        integer_scale: 1,
    };
    app.update(
        DesktopMessage::Outputs(vec![output(1)]),
        &mut Effects::default(),
    );
    assert_eq!(
        observed_target(&app),
        None,
        "named output waits for its own configure"
    );
    let route = app.monitors.connected[&id].route(&id);
    app.update(
        DesktopMessage::OnMonitor(
            route.clone(),
            Box::new(DesktopMessage::Resize("background", 65., 37., density)),
        ),
        &mut Effects::default(),
    );
    assert_eq!(observed_target(&app), Some(pixel_target(82, 47)));
    let revision = app.live_wallpaper.players.values().next().unwrap().revision;
    app.update(
        DesktopMessage::Outputs(vec![output(1)]),
        &mut Effects::default(),
    );
    assert_eq!(observed_target(&app), Some(pixel_target(82, 47)));
    assert_eq!(
        app.live_wallpaper.players.values().next().unwrap().revision,
        revision,
        "unchanged integer metadata must not replace fractional geometry"
    );
    app.update(
        DesktopMessage::OnMonitor(
            route.clone(),
            Box::new(DesktopMessage::Resize("background", f32::NAN, 37., density)),
        ),
        &mut Effects::default(),
    );
    assert_eq!(observed_target(&app), Some(pixel_target(82, 47)));
    app.update(
        DesktopMessage::Outputs(vec![output(2)]),
        &mut Effects::default(),
    );
    assert_eq!(
        observed_target(&app),
        None,
        "a replacement protocol owner needs fresh geometry"
    );
    app.update(
        DesktopMessage::OnMonitor(
            route,
            Box::new(DesktopMessage::Resize(
                "background",
                4096.,
                4096.,
                Density::ONE,
            )),
        ),
        &mut Effects::default(),
    );
    assert_eq!(
        observed_target(&app),
        None,
        "retired output cannot supply new demand"
    );
    let route = app.monitors.connected[&id].route(&id);
    app.update(
        DesktopMessage::OnMonitor(
            route,
            Box::new(DesktopMessage::Resize(
                "background",
                1920.,
                1080.,
                Density::ONE,
            )),
        ),
        &mut Effects::default(),
    );
    assert_eq!(observed_target(&app), Some(pixel_target(1920, 1080)));
    app.update(DesktopMessage::Outputs(vec![]), &mut Effects::default());
    assert!(app.live_wallpaper.players.is_empty());
    assert_eq!(
        app.settings, settings,
        "display fit never writes preferences"
    );
}
#[test]
fn per_monitor_disable_overrides_default_and_frame_submission_never_creates_an_idle_loop() {
    let mut controller = controller();
    let id = MonitorId::new("DEMO-1").unwrap();
    let mut settings = LiveWallpapers {
        default: Some(setting("/fixture/default.mp4")),
        ..Default::default()
    };
    let mut disabled = setting("/fixture/default.mp4");
    disabled.enabled = false;
    settings.monitors.insert(id.clone(), disabled);
    controller.reconcile(
        &settings,
        &[id],
        true,
        &WallpaperPlaybackContext::default(),
        &BTreeMap::new(),
    );
    assert!(controller.players.is_empty());
    let mut effects = Effects::default();
    controller.update(Message::Open, &mut effects);
    controller.update(Message::Frame(0.), &mut effects);
    effects.redraw.clear();
    effects.now = 1.;
    controller.update(Message::Frame(1.), &mut effects);
    assert!(effects.redraw.is_empty());
    controller.update(Message::Close, &mut effects);
    assert!(controller.mounted);
    effects.redraw.clear();
    controller.update(Message::Frame(2.), &mut effects);
    assert!(!controller.mounted);
    effects.redraw.clear();
    controller.update(Message::Frame(3.), &mut effects);
    assert!(effects.redraw.is_empty());
}
#[test]
fn cancelled_import_and_decoder_error_keep_configuration_and_last_frame_separate() {
    let mut controller = controller();
    let mut effects = Effects::default();
    controller.update(
        Message::ImportPath(LiveWallpaperTarget::Default, "/fixture/clip.mp4".into()),
        &mut effects,
    );
    let revision = controller.importing.as_ref().unwrap().revision;
    controller.update(Message::Close, &mut effects);
    assert!(controller.importing.is_none());
    assert!(
        controller
            .update(
                Message::Prepared(revision, Err(DomainError::Failed("late fixture".into()))),
                &mut effects
            )
            .is_none()
    );
    assert!(controller.error.is_empty());
    let settings = LiveWallpapers {
        default: Some(setting("/fixture/default.mp4")),
        ..Default::default()
    };
    controller.reconcile(
        &settings,
        &[],
        false,
        &WallpaperPlaybackContext::default(),
        &BTreeMap::new(),
    );
    let key = controller.routes[&LiveWallpaperTarget::Default].key.clone();
    let revision = controller.players[&key].revision;
    let image = Arc::new(ImageStream::new().frame_rgba(1, 1, vec![255; 4]).unwrap());
    controller.players[&key]
        .mailbox
        .lock()
        .unwrap()
        .publish(Delivery::Frame(image.clone().into()));
    controller.update(Message::Wake(key.clone(), revision), &mut effects);
    controller.players[&key]
        .mailbox
        .lock()
        .unwrap()
        .publish(Delivery::Error("decoder fixture".into()));
    controller.update(Message::Wake(key, revision), &mut effects);
    assert!(Arc::ptr_eq(controller.frame(None).unwrap(), &image));
    assert!(controller.subscriptions().is_empty());
    assert_eq!(controller.status(), "decoder fixture");
}

fn put_frame(controller: &mut LiveWallpaper, target: &LiveWallpaperTarget) -> Arc<ImageData> {
    let key = controller.routes[target].key.clone();
    let revision = controller.players[&key].revision;
    let frame = Arc::new(
        ImageStream::new()
            .frame_rgba(1, 1, vec![255, 40, 20, 255])
            .unwrap(),
    );
    controller.players[&key]
        .mailbox
        .lock()
        .unwrap()
        .publish(Delivery::Frame(frame.clone().into()));
    controller.update(Message::Wake(key, revision), &mut Effects::default());
    frame
}

#[test]
fn off_retires_only_its_captured_import_target_and_rejects_late_success() {
    let mut controller = controller();
    let mut effects = Effects::default();
    let target = LiveWallpaperTarget::Monitor(MonitorId::new("DEMO-1").unwrap());
    let Some(Intent::ImportStarted(started, revision)) = controller.update(
        Message::ImportPath(target.clone(), "/fixture/clip.mp4".into()),
        &mut effects,
    ) else {
        panic!("import was not admitted")
    };
    assert_eq!(started, target);
    let cancellation = effects.tasks.pop().unwrap().cancellation;
    controller.update(Message::Off(LiveWallpaperTarget::Default), &mut effects);
    assert_eq!(controller.importing_target(), Some(&target));
    assert!(!cancellation.cancelled());
    controller.update(Message::Target(LiveWallpaperTarget::Default), &mut effects);
    assert!(
        matches!(controller.update(Message::Off(target.clone()), &mut effects), Some(Intent::Off(captured)) if captured == target)
    );
    assert!(cancellation.cancelled());
    let poster =
        Arc::new(ImageData::from_rgba("cancelled-live-poster", 1, 1, vec![255; 4]).unwrap());
    assert!(
        controller
            .update(
                Message::Prepared(revision, Ok((setting("/fixture/clip.mp4").asset, poster))),
                &mut effects
            )
            .is_none()
    );
    assert!(controller.prepared.is_empty());
}

#[test]
fn controls_use_their_captured_target_and_import_reads_the_current_path() {
    let mut controller = controller();
    let mut effects = Effects::default();
    let captured = LiveWallpaperTarget::Monitor(MonitorId::new("DEMO-1").unwrap());
    controller.update(Message::Target(LiveWallpaperTarget::Default), &mut effects);
    assert!(
        matches!(controller.update(Message::Pause(captured.clone(), true), &mut effects), Some(Intent::Pause(target, true)) if target == captured)
    );
    assert!(
        matches!(controller.update(Message::Rate(captured.clone(), WallpaperFps::new(15).unwrap().into()), &mut effects), Some(Intent::Rate(target, rate)) if target == captured && rate.cap().is_some_and(|cap| cap.value() == 15))
    );
    assert!(
        matches!(controller.update(Message::Rate(captured.clone(), WallpaperFrameRate::Source), &mut effects), Some(Intent::Rate(target, WallpaperFrameRate::Source)) if target == captured)
    );
    controller.path = "/fixture/latest.mp4".into();
    assert!(
        matches!(controller.update(Message::Import(captured.clone()), &mut effects), Some(Intent::ImportStarted(target, _)) if target == captured)
    );
    assert_eq!(controller.importing_target(), Some(&captured));
}

#[test]
fn paused_profiles_keep_their_own_frame_across_fps_changes_and_stream_splitting() {
    let mut controller = controller();
    let id = MonitorId::new("DEMO-1").unwrap();
    let other = MonitorId::new("DEMO-2").unwrap();
    let target = LiveWallpaperTarget::Monitor(id.clone());
    let mut settings = LiveWallpapers {
        default: Some(setting("/fixture/default.mp4")),
        ..Default::default()
    };
    controller.reconcile(
        &settings,
        &[id.clone(), other.clone()],
        true,
        &WallpaperPlaybackContext::default(),
        &BTreeMap::new(),
    );
    let original = put_frame(&mut controller, &target);
    let mut paused = settings.default.clone().unwrap();
    paused.paused = true;
    settings.monitors.insert(id.clone(), paused);
    controller.reconcile(
        &settings,
        &[id.clone(), other.clone()],
        true,
        &WallpaperPlaybackContext::default(),
        &BTreeMap::new(),
    );
    let held = controller.frame(Some(&id)).unwrap().clone();
    assert_eq!(held.pixels(), original.pixels());
    assert!(held.stream_revision().is_none());
    assert_eq!(controller.subscriptions().len(), 1);
    let paused_key = controller.routes[&target].key.clone();
    let old_revision = controller.players[&paused_key].revision;
    let imported_poster =
        Arc::new(ImageData::from_rgba("older-import-poster", 1, 1, vec![0, 0, 255, 255]).unwrap());
    controller
        .prepared
        .insert(settings.monitors[&id].asset.path.clone(), imported_poster);
    settings.monitors.get_mut(&id).unwrap().fps = WallpaperFps::new(15).unwrap().into();
    controller.reconcile(
        &settings,
        &[id.clone(), other.clone()],
        true,
        &WallpaperPlaybackContext::default(),
        &BTreeMap::new(),
    );
    assert!(Arc::ptr_eq(controller.frame(Some(&id)).unwrap(), &held));
    assert!(
        controller
            .update(
                Message::Wake(paused_key, old_revision),
                &mut Effects::default()
            )
            .is_none()
    );
    assert_eq!(controller.selected_status(&settings, &target), "Paused");
    assert_eq!(
        controller.selected_status(&settings, &LiveWallpaperTarget::Monitor(other)),
        "Playing"
    );
}

#[test]
fn retry_preserves_last_good_frame_retires_old_notices_and_is_selected() {
    let mut controller = controller();
    let id = MonitorId::new("DEMO-1").unwrap();
    let other = MonitorId::new("DEMO-2").unwrap();
    let target = LiveWallpaperTarget::Monitor(id.clone());
    let mut settings = LiveWallpapers {
        default: Some(setting("/fixture/default.mp4")),
        ..Default::default()
    };
    settings
        .monitors
        .insert(other.clone(), setting("/fixture/other.mp4"));
    controller.reconcile(
        &settings,
        &[id.clone(), other.clone()],
        true,
        &WallpaperPlaybackContext::default(),
        &BTreeMap::new(),
    );
    let image = put_frame(&mut controller, &target);
    let key = controller.routes[&target].key.clone();
    let old_revision = controller.players[&key].revision;
    controller.update(
        Message::WorkerFailed(old_revision, "decoder failed".into()),
        &mut Effects::default(),
    );
    assert_eq!(
        controller.selected_status(&settings, &target),
        "decoder failed"
    );
    assert_eq!(
        controller.selected_status(&settings, &LiveWallpaperTarget::Monitor(other)),
        "Playing"
    );
    assert!(controller.retry_available(&settings, &target));
    controller.update(Message::Retry(target.clone()), &mut Effects::default());
    assert!(!controller.retry_available(&settings, &target));
    assert!(Arc::ptr_eq(controller.frame(Some(&id)).unwrap(), &image));
    assert_ne!(controller.players[&key].revision, old_revision);
    controller.update(
        Message::WorkerFailed(old_revision, "stale failure".into()),
        &mut Effects::default(),
    );
    assert_eq!(controller.selected_status(&settings, &target), "Playing");
    assert_eq!(controller.subscriptions().len(), 2);
}

struct Posters(Arc<ImageData>);
impl AssetPort for Posters {
    fn initial(&self) -> BTreeMap<String, Arc<ImageData>> {
        BTreeMap::new()
    }
    fn application_icons(&self, _: &[Application]) -> crate::ports::Images {
        vec![]
    }
    fn thumbnail(&self, _: &str) -> Result<Arc<ImageData>> {
        Ok(self.0.clone())
    }
    fn preview(&self, _: &ImageData) -> Arc<ImageData> {
        self.0.clone()
    }
    fn background(
        &self,
        _: &str,
        _: Option<crate::ports::BackgroundTarget>,
    ) -> Result<Arc<ImageData>> {
        Ok(self.0.clone())
    }
}

#[test]
fn restored_paused_profiles_prepare_one_poster_without_starting_playback_or_idle_work() {
    let adapters = crate::test_adapters::adapters();
    let poster =
        Arc::new(ImageData::from_rgba("paused-live-poster", 1, 1, vec![12, 34, 56, 255]).unwrap());
    let mut controller = LiveWallpaper::new(adapters.playback, Arc::new(Posters(poster.clone())));
    let mut profile = setting("/fixture/paused.mp4");
    profile.paused = true;
    let settings = LiveWallpapers {
        default: Some(profile),
        ..Default::default()
    };
    controller.reconcile(
        &settings,
        &[],
        false,
        &WallpaperPlaybackContext::default(),
        &BTreeMap::new(),
    );
    let mut subscriptions = controller.subscriptions();
    assert_eq!(subscriptions.len(), 1);
    let notices = Arc::new(Mutex::new(Vec::new()));
    let output = notices.clone();
    (subscriptions.pop().unwrap().run)(
        lucent_api::Emitter::new(move |message| output.lock().unwrap().push(message)),
        Cancellation::default(),
    );
    assert_eq!(notices.lock().unwrap().len(), 1);
    let wake = notices.lock().unwrap().pop().unwrap();
    assert!(
        matches!(controller.update(wake, &mut Effects::default()), Some(Intent::FrameChanged(targets)) if targets == [LiveWallpaperTarget::Default])
    );
    assert!(Arc::ptr_eq(controller.frame(None).unwrap(), &poster));
    assert!(controller.subscriptions().is_empty());
    assert_eq!(
        controller.selected_status(&settings, &LiveWallpaperTarget::Default),
        "Paused"
    );
}

#[test]
fn fullscreen_freezes_one_output_without_stopping_or_repainting_its_visible_peer() {
    let mut controller = controller();
    let left = MonitorId::new("DP-1").unwrap();
    let right = MonitorId::new("DP-2").unwrap();
    let left_target = LiveWallpaperTarget::Monitor(left.clone());
    let right_target = LiveWallpaperTarget::Monitor(right.clone());
    let monitors = [left.clone(), right.clone()];
    let mut settings = LiveWallpapers {
        default: Some(setting("/fixture/shared.mp4")),
        ..Default::default()
    };
    let original_settings = settings.clone();
    let mut context = WallpaperPlaybackContext::default();
    controller.reconcile(&settings, &monitors, true, &context, &BTreeMap::new());
    let original = put_frame(&mut controller, &left_target);
    let key = controller.routes[&left_target].key.clone();
    let revision = controller.players[&key].revision;
    context.fullscreen_monitors.insert(left.clone());
    controller.reconcile(&settings, &monitors, true, &context, &BTreeMap::new());
    assert_eq!(controller.players.len(), 1);
    assert_eq!(controller.subscriptions().len(), 1);
    assert_eq!(controller.players[&key].revision, revision);
    let held = controller.frame(Some(&left)).unwrap().clone();
    assert_eq!(held.pixels(), original.pixels());
    assert!(
        held.stream_revision().is_none(),
        "a shared GPU stream must not mutate a paused output"
    );
    let later = Arc::new(
        ImageStream::new()
            .frame_rgba(1, 1, vec![10, 20, 30, 255])
            .unwrap(),
    );
    controller.players[&key]
        .mailbox
        .lock()
        .unwrap()
        .publish(Delivery::Frame(later.clone().into()));
    let notice = controller.update(
        Message::Wake(key.clone(), revision),
        &mut Effects::default(),
    );
    assert!(
        matches!(notice, Some(Intent::FrameChanged(targets)) if targets == [right_target.clone()])
    );
    controller.reconcile(&settings, &monitors, true, &context, &BTreeMap::new());
    assert!(Arc::ptr_eq(controller.frame(Some(&left)).unwrap(), &held));
    assert!(Arc::ptr_eq(controller.frame(Some(&right)).unwrap(), &later));
    assert_eq!(
        controller.selected_status(&settings, &left_target),
        "Paused behind a fullscreen app"
    );
    assert_eq!(
        controller.selected_status(&settings, &right_target),
        "Playing"
    );
    assert_eq!(
        controller.selected_status(&settings, &LiveWallpaperTarget::Default),
        "Playing"
    );

    context.fullscreen_monitors.insert(right.clone());
    controller.reconcile(&settings, &monitors, true, &context, &BTreeMap::new());
    assert!(controller.subscriptions().is_empty());
    assert_ne!(controller.players[&key].revision, revision);
    assert!(
        controller
            .update(
                Message::Wake(key.clone(), revision),
                &mut Effects::default()
            )
            .is_none()
    );
    assert_eq!(
        controller.selected_status(&settings, &LiveWallpaperTarget::Default),
        "Paused behind a fullscreen app"
    );
    assert_eq!(
        settings, original_settings,
        "observations must never persist a manual pause"
    );
    context.fullscreen_monitors.remove(&right);
    controller.reconcile(&settings, &monitors, true, &context, &BTreeMap::new());
    assert_eq!(controller.subscriptions().len(), 1);
    assert!(Arc::ptr_eq(controller.frame(Some(&left)).unwrap(), &held));
    settings.policy.pause_when_fullscreen = false;
    controller.reconcile(&settings, &monitors, true, &context, &BTreeMap::new());
    assert!(
        controller
            .routes
            .values()
            .all(|route| route.held.is_none() && route.pause.is_none())
    );
    assert_eq!(controller.subscriptions().len(), 1);
    controller.reconcile(&settings, &[], true, &context, &BTreeMap::new());
    assert!(controller.routes.is_empty() && controller.players.is_empty());
}

#[test]
fn battery_and_sleep_observations_do_not_save_preferences_or_resume_a_manual_pause() {
    use crate::desktop::{Desktop, Message as DesktopMessage};
    use lucent_api::Component;
    let mut app = Desktop::new(crate::test_adapters::adapters());
    app.settings.live_wallpapers.default = Some(setting("/fixture/power.mp4"));
    let settings = app.settings.clone();
    let snapshot = |on_battery, sleeping| PowerSnapshot {
        battery: PowerCapability::Ready(Some(Battery {
            charge: Percentage::new(50.).unwrap(),
            level: BatteryLevel::None,
            state: BatteryState::Unknown,
            on_battery,
            seconds_to_empty: None,
            seconds_to_full: None,
        })),
        preparing_for_sleep: sleeping,
        ..Default::default()
    };
    let observe = |app: &mut Desktop, value| {
        let mut effects = Effects::default();
        app.update(
            DesktopMessage::Power(crate::power::Message::Observed(value)),
            &mut effects,
        );
        assert!(
            effects.tasks.is_empty(),
            "an observation must not write preferences"
        );
    };
    observe(&mut app, Ok(snapshot(false, false)));
    assert_eq!(app.live_wallpaper.subscriptions().len(), 1);
    put_frame(&mut app.live_wallpaper, &LiveWallpaperTarget::Default);
    observe(&mut app, Ok(snapshot(true, false)));
    assert!(app.live_wallpaper.subscriptions().is_empty());
    assert_eq!(
        app.live_wallpaper
            .selected_status(&app.settings.live_wallpapers, &LiveWallpaperTarget::Default),
        "Paused on battery power"
    );
    observe(&mut app, Err(DomainError::Unavailable("fixture".into())));
    assert_eq!(app.wallpaper_context.on_battery, None);
    assert_eq!(app.live_wallpaper.subscriptions().len(), 1);
    observe(&mut app, Ok(snapshot(false, true)));
    assert!(app.live_wallpaper.subscriptions().is_empty());
    observe(&mut app, Err(DomainError::Unavailable("fixture".into())));
    assert!(
        app.live_wallpaper.subscriptions().is_empty(),
        "disconnect is not a resume acknowledgement"
    );
    observe(&mut app, Ok(snapshot(false, false)));
    assert_eq!(app.live_wallpaper.subscriptions().len(), 1);
    assert_eq!(app.settings, settings);
    app.settings.live_wallpapers.paused = true;
    observe(&mut app, Ok(snapshot(true, false)));
    observe(&mut app, Ok(snapshot(false, false)));
    assert!(app.live_wallpaper.subscriptions().is_empty());
    assert!(app.settings.live_wallpapers.paused);
}

#[test]
fn old_settings_get_optional_pause_policies_and_roundtrip_preserves_opt_outs() {
    let old = serde_json::json!({"default":null,"monitors":{},"paused":true});
    let mut settings: LiveWallpapers = serde_json::from_value(old).unwrap();
    assert!(
        settings.paused
            && settings.policy.pause_on_battery
            && settings.policy.pause_when_fullscreen
    );
    settings.policy.pause_on_battery = false;
    let restored: LiveWallpapers =
        serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
    assert_eq!(restored, settings);
    let partial: LiveWallpapers =
        serde_json::from_value(serde_json::json!({"policy":{"pause_when_fullscreen":false}}))
            .unwrap();
    assert!(partial.policy.pause_on_battery);
    assert!(!partial.policy.pause_when_fullscreen);
}

#[test]
fn session_lock_observation_lifetime_and_failure_preserve_manual_preferences() {
    use crate::desktop::{Desktop, Message as DesktopMessage};
    use lucent_api::{Application, Component};
    let mut app = Desktop::new(crate::test_adapters::adapters());
    let watching = |app: &Desktop| app.subscriptions().iter().any(|s| s.id == "session-lock");
    assert!(!watching(&app), "static wallpapers need no lock polling");
    app.settings.live_wallpapers.default = Some(setting("/fixture/session.mp4"));
    app.settings.live_wallpapers.policy.pause_on_battery = false;
    app.settings.live_wallpapers.policy.pause_when_fullscreen = false;
    let settings = app.settings.clone();
    let observe = |app: &mut Desktop, value| {
        let mut effects = Effects::default();
        app.update(DesktopMessage::SessionLock(value), &mut effects);
        assert!(
            effects.tasks.is_empty(),
            "observations must not change saved preferences"
        );
    };
    observe(&mut app, Ok(SessionLockState::Unlocked));
    assert!(watching(&app));
    assert_eq!(app.live_wallpaper.subscriptions().len(), 1);
    let frame = put_frame(&mut app.live_wallpaper, &LiveWallpaperTarget::Default);
    observe(&mut app, Ok(SessionLockState::Locked));
    assert!(
        watching(&app),
        "paused playback still needs unlock observations"
    );
    assert!(app.live_wallpaper.subscriptions().is_empty());
    assert_eq!(
        app.live_wallpaper.frame(None).unwrap().pixels(),
        frame.pixels()
    );
    assert_eq!(
        app.live_wallpaper
            .selected_status(&app.settings.live_wallpapers, &LiveWallpaperTarget::Default),
        "Paused while the session is locked"
    );
    observe(&mut app, Err(DomainError::Unavailable("fixture".into())));
    assert!(app.live_wallpaper.subscriptions().is_empty());
    observe(&mut app, Ok(SessionLockState::Unlocked));
    assert_eq!(app.live_wallpaper.subscriptions().len(), 1);
    assert_eq!(app.settings, settings);

    app.settings
        .live_wallpapers
        .default
        .as_mut()
        .unwrap()
        .paused = true;
    observe(&mut app, Ok(SessionLockState::Locked));
    observe(&mut app, Ok(SessionLockState::Unlocked));
    assert!(app.live_wallpaper.subscriptions().is_empty());
    assert!(
        app.settings
            .live_wallpapers
            .default
            .as_ref()
            .unwrap()
            .paused
    );
    app.settings.live_wallpapers.default = None;
    observe(&mut app, Ok(SessionLockState::Unlocked));
    assert!(
        !watching(&app),
        "removing playback retires lock observation"
    );
}

fn deliver_current_subscription(controller: &mut LiveWallpaper) {
    let subscription = controller.subscriptions().pop().unwrap();
    deliver_subscription(controller, subscription);
}
#[derive(Default)]
struct NativePlanePlayback {
    calls: Mutex<Vec<(bool, Option<WallpaperPixelTarget>)>>,
    marker: std::sync::atomic::AtomicU8,
}
fn native_poster() -> Arc<ImageData> {
    Arc::new(ImageData::from_rgba("native-safe-poster", 2, 2, [90, 20, 10, 255].repeat(4)).unwrap())
}
fn neutral_planes(marker: u8) -> lucent_api::video_frame::Yuv420Frame {
    use lucent_api::video_frame::{
        Bt709, Bytes, ChromaSiting, Plane, Range, Transfer, Yuv420Frame,
    };
    Yuv420Frame::new(
        2,
        2,
        [
            Plane::new(Bytes::from(vec![marker; 4]), 2, 2, 2).unwrap(),
            Plane::new(Bytes::from(vec![128]), 1, 1, 1).unwrap(),
            Plane::new(Bytes::from(vec![128]), 1, 1, 1).unwrap(),
        ],
        Bt709 {
            range: Range::Full,
            transfer: Transfer::Srgb,
            chroma: ChromaSiting::Left,
        },
    )
    .unwrap()
}
impl WallpaperPlaybackPort for NativePlanePlayback {
    fn prepare(&self, _: &Wallpaper) -> Result<LiveWallpaperAsset> {
        Err(DomainError::Unavailable("unused".into()))
    }
    fn play(
        &self,
        _: &LiveWallpaperAsset,
        options: PlaybackOptions,
        emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
        _: Arc<dyn StopSignal>,
    ) {
        self.calls.lock().unwrap().push((false, options.target));
        emit(MediaFrame::rgba(
            2,
            2,
            [30, 40, 50, 255].repeat(4),
            std::time::Duration::ZERO,
        ));
    }
    fn play_native(
        &self,
        _: &LiveWallpaperAsset,
        options: NativePlaybackOptions,
        emit: &mut dyn FnMut(Result<NativeMediaFrame>) -> bool,
        _: Arc<dyn StopSignal>,
    ) {
        self.calls.lock().unwrap().push((true, options.target));
        let marker = 11
            + self
                .marker
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        emit(Ok(NativeMediaFrame::Yuv420 {
            frame: neutral_planes(marker),
            timestamp: std::time::Duration::from_millis(120),
            publication: PlanePublication {
                referenced_bytes: 0,
                copied_bytes: 6,
            },
        }));
    }
}
#[test]
fn native_plane_opt_in_keeps_default_unknown_and_shared_routes_on_rgba() {
    let adapter = Arc::new(NativePlanePlayback::default());
    let mut controller = LiveWallpaper::new(adapter.clone(), Arc::new(Posters(native_poster())));
    let settings = LiveWallpapers {
        default: Some(setting("/fixture/native.mp4")),
        ..Default::default()
    };
    let left = MonitorId::new("native-left").unwrap();
    let right = MonitorId::new("native-right").unwrap();
    let context = WallpaperPlaybackContext::default();
    let mut targets = BTreeMap::from([(
        LiveWallpaperTarget::Monitor(left.clone()),
        pixel_target(16, 16),
    )]);
    controller.reconcile(
        &settings,
        std::slice::from_ref(&left),
        true,
        &context,
        &targets,
    );
    deliver_current_subscription(&mut controller);
    assert!(controller.frame(Some(&left)).is_some());
    assert!(!adapter.calls.lock().unwrap().last().unwrap().0);
    controller.enable_native_planes(true);
    let unobserved = BTreeMap::from([(LiveWallpaperTarget::Default, pixel_target(16, 16))]);
    controller.reconcile(&settings, &[], false, &context, &unobserved);
    deliver_current_subscription(&mut controller);
    assert!(
        !adapter.calls.lock().unwrap().last().unwrap().0,
        "unobserved output enumeration stays RGBA even with a stale target"
    );
    controller.reconcile(
        &settings,
        std::slice::from_ref(&left),
        true,
        &context,
        &BTreeMap::new(),
    );
    deliver_current_subscription(&mut controller);
    assert!(
        !adapter.calls.lock().unwrap().last().unwrap().0,
        "unknown dimensions stay RGBA"
    );
    controller.reconcile(
        &settings,
        std::slice::from_ref(&left),
        true,
        &context,
        &targets,
    );
    deliver_current_subscription(&mut controller);
    let frame = controller.presentation_source(Some(&left)).unwrap().1;
    assert_eq!(frame.representation(), "yuv420");
    assert!(controller.frame(Some(&left)).is_none());
    assert_eq!(
        adapter.calls.lock().unwrap().last(),
        Some(&(true, Some(pixel_target(16, 16))))
    );
    targets.insert(
        LiveWallpaperTarget::Monitor(right.clone()),
        pixel_target(8, 8),
    );
    controller.reconcile(&settings, &[left.clone(), right], true, &context, &targets);
    deliver_current_subscription(&mut controller);
    assert!(
        !adapter.calls.lock().unwrap().last().unwrap().0,
        "a max aggregate cannot prove native density for each consumer"
    );
    assert_eq!(
        controller
            .presentation_source(Some(&left))
            .unwrap()
            .1
            .representation(),
        "rgba"
    );
    let retired = controller.subscriptions().pop().unwrap();
    controller.reconcile(
        &settings,
        std::slice::from_ref(&left),
        true,
        &context,
        &targets,
    );
    deliver_current_subscription(&mut controller);
    let restored = controller
        .presentation_source(Some(&left))
        .unwrap()
        .1
        .cache_id();
    deliver_subscription(&mut controller, retired);
    assert_eq!(
        controller
            .presentation_source(Some(&left))
            .unwrap()
            .1
            .cache_id(),
        restored,
        "retired shared RGBA worker cannot overwrite the newly admitted native owner"
    );
    assert_eq!(
        controller
            .presentation_source(Some(&left))
            .unwrap()
            .1
            .representation(),
        "yuv420"
    );
}
#[test]
fn native_plane_frames_keep_last_good_and_freeze_across_pause_fps_and_stale_wakes() {
    let adapter = Arc::new(NativePlanePlayback::default());
    let mut controller = LiveWallpaper::new(adapter, Arc::new(Posters(native_poster())));
    controller.enable_native_planes(true);
    let mut settings = LiveWallpapers {
        default: Some(setting("/fixture/native-pause.mp4")),
        ..Default::default()
    };
    let id = MonitorId::new("native-only").unwrap();
    let monitors = std::slice::from_ref(&id);
    let targets = BTreeMap::from([(
        LiveWallpaperTarget::Monitor(id.clone()),
        pixel_target(16, 16),
    )]);
    let context = WallpaperPlaybackContext::default();
    controller.reconcile(&settings, monitors, true, &context, &targets);
    deliver_current_subscription(&mut controller);
    let first = controller.presentation_source(Some(&id)).unwrap().1.clone();
    let late = controller.subscriptions().pop().unwrap();
    let key = controller.routes[&LiveWallpaperTarget::Monitor(id.clone())]
        .key
        .clone();
    let revision = controller.players[&key].revision;
    controller.players[&key]
        .mailbox
        .lock()
        .unwrap()
        .publish(Delivery::Error("native fixture failed".into()));
    controller.update(
        Message::Wake(key.clone(), revision),
        &mut Effects::default(),
    );
    assert_eq!(
        controller
            .presentation_source(Some(&id))
            .unwrap()
            .1
            .cache_id(),
        first.cache_id()
    );
    assert!(controller.status().contains("native fixture failed"));
    settings.default.as_mut().unwrap().paused = true;
    controller.reconcile(&settings, monitors, true, &context, &targets);
    let held = controller.presentation_source(Some(&id)).unwrap().1.clone();
    assert_eq!(held.cache_id(), first.cache_id());
    assert!(held.stream_revision().is_none());
    assert!(controller.subscriptions().is_empty());
    settings.default.as_mut().unwrap().fps =
        WallpaperFrameRate::Capped(WallpaperFps::new(60).unwrap());
    controller.reconcile(&settings, monitors, true, &context, &targets);
    deliver_subscription(&mut controller, late);
    assert_eq!(
        controller
            .presentation_source(Some(&id))
            .unwrap()
            .1
            .cache_id(),
        held.cache_id()
    );
    let PresentationFrame::Video { data, .. } =
        controller.presentation_source(Some(&id)).unwrap().1
    else {
        panic!("native frozen frame lost")
    };
    assert_eq!(data.frame().planes()[0].row(0).unwrap(), &[11, 11]);
    settings.default.as_mut().unwrap().paused = false;
    controller.reconcile(&settings, monitors, true, &context, &targets);
    deliver_current_subscription(&mut controller);
    assert!(
        controller
            .presentation_source(Some(&id))
            .unwrap()
            .1
            .stream_revision()
            .is_some()
    );
    assert_ne!(
        controller
            .presentation_source(Some(&id))
            .unwrap()
            .1
            .cache_id(),
        held.cache_id()
    );
}

fn assert_rgba_presentation(controller: &LiveWallpaper, id: &MonitorId, poster: &ImageData) {
    let frame = controller.presentation_source(Some(id)).unwrap().1;
    assert_eq!(frame.cache_id(), poster.cache_id());
    let view: lucent_api::Element<()> = frame.element().fill();
    let scene = lucent_ui::Layout::new(Arc::new(vec![])).build(
        &view,
        8.,
        8.,
        &lucent_ui::Interaction::default(),
        0.,
    );
    let lucent_ui::Paint::Image { data, .. } = &scene.paint[0] else {
        panic!("current paint must use the prepared RGBA handoff")
    };
    assert_eq!(data.cache_id(), poster.cache_id());
    assert_eq!(data.pixels(), poster.pixels());
}
#[test]
fn native_geometry_changes_paint_same_asset_rgba_during_pending_or_failed_replacement() {
    for case in [
        "smaller",
        "shared",
        "unobserved",
        "withdrawn",
        "paused-smaller",
        "paused-withdrawn",
        "paused-shared",
    ] {
        let adapter = Arc::new(NativePlanePlayback::default());
        let poster = native_poster();
        let mut controller = LiveWallpaper::new(adapter, Arc::new(Posters(poster.clone())));
        controller.enable_native_planes(true);
        let mut settings = LiveWallpapers {
            default: Some(setting("/fixture/native-safe.mp4")),
            ..Default::default()
        };
        let left = MonitorId::new("safe-left").unwrap();
        let right = MonitorId::new("safe-right").unwrap();
        let mut monitors = vec![left.clone()];
        let context = WallpaperPlaybackContext::default();
        let mut targets = BTreeMap::from([(
            LiveWallpaperTarget::Monitor(left.clone()),
            pixel_target(16, 16),
        )]);
        controller.reconcile(&settings, &monitors, true, &context, &targets);
        deliver_current_subscription(&mut controller);
        assert_eq!(
            controller
                .presentation_source(Some(&left))
                .unwrap()
                .1
                .representation(),
            "yuv420"
        );
        let old = controller.subscriptions().pop().unwrap();
        if case.starts_with("paused-") {
            settings.default.as_mut().unwrap().paused = true;
        }
        if case.ends_with("smaller") {
            targets.insert(
                LiveWallpaperTarget::Monitor(left.clone()),
                pixel_target(1, 1),
            );
        }
        if case.ends_with("shared") {
            monitors.push(right.clone());
            targets.insert(LiveWallpaperTarget::Monitor(right), pixel_target(16, 16));
        }
        if case.ends_with("withdrawn") {
            targets.clear();
        }
        controller.reconcile(
            &settings,
            &monitors,
            case != "unobserved",
            &context,
            &targets,
        );
        assert_rgba_presentation(&controller, &left, &poster);
        assert_eq!(
            controller.selected_status(&settings, &LiveWallpaperTarget::Monitor(left.clone())),
            if case.starts_with("paused-") {
                "Paused"
            } else {
                "Preparing live wallpaper…"
            }
        );
        let key = controller.routes[&LiveWallpaperTarget::Monitor(left.clone())]
            .key
            .clone();
        let player = &controller.players[&key];
        let revision = player.revision;
        player
            .mailbox
            .lock()
            .unwrap()
            .publish(Delivery::Error("replacement fixture failed".into()));
        let mut failed = Effects::default();
        controller.update(Message::Wake(key, revision), &mut failed);
        assert!(
            failed.redraw.contains("live-wallpaper"),
            "failure status must invalidate its visible panel"
        );
        assert!(
            controller.status().contains("replacement fixture failed"),
            "{case}"
        );
        assert_rgba_presentation(&controller, &left, &poster);
        deliver_subscription(&mut controller, old);
        assert_rgba_presentation(&controller, &left, &poster);
        if case.starts_with("paused-") {
            settings.default.as_mut().unwrap().fps =
                WallpaperFrameRate::Capped(WallpaperFps::new(60).unwrap());
            controller.reconcile(&settings, &monitors, true, &context, &targets);
            assert_rgba_presentation(&controller, &left, &poster);
        }
    }
}
#[test]
fn native_poster_survives_latest_frame_coalescing_before_one_generation_wake() {
    let poster = native_poster();
    let mut controller = LiveWallpaper::new(
        Arc::new(NativePlanePlayback::default()),
        Arc::new(Posters(poster.clone())),
    );
    controller.enable_native_planes(true);
    let settings = LiveWallpapers {
        default: Some(setting("/fixture/coalesced.mp4")),
        ..Default::default()
    };
    let left = MonitorId::new("coalesced-left").unwrap();
    let context = WallpaperPlaybackContext::default();
    let mut targets = BTreeMap::from([(
        LiveWallpaperTarget::Monitor(left.clone()),
        pixel_target(16, 16),
    )]);
    controller.reconcile(
        &settings,
        std::slice::from_ref(&left),
        true,
        &context,
        &targets,
    );
    let key = controller.routes[&LiveWallpaperTarget::Monitor(left.clone())]
        .key
        .clone();
    let player = &controller.players[&key];
    let revision = player.revision;
    let stream = ImageStream::new();
    {
        let mut mailbox = player.mailbox.lock().unwrap();
        assert!(mailbox.publish_poster(&player.asset.path, revision, poster.clone()));
        for marker in 0..100 {
            let data = Arc::new(stream.frame_yuv420(neutral_planes(marker)).unwrap());
            assert!(!mailbox.publish(Delivery::Frame(PresentationFrame::native(
                data,
                poster.clone()
            ))));
        }
    }
    controller.update(Message::Wake(key, revision), &mut Effects::default());
    let PresentationFrame::Video { data, .. } =
        controller.presentation_source(Some(&left)).unwrap().1
    else {
        panic!("latest native frame missing")
    };
    assert_eq!(data.frame().planes()[0].row(0).unwrap(), &[99, 99]);
    targets.clear();
    controller.reconcile(
        &settings,
        std::slice::from_ref(&left),
        true,
        &context,
        &targets,
    );
    assert_rgba_presentation(&controller, &left, &poster);
}
#[test]
fn native_poster_authority_rejects_other_assets_old_generations_and_retired_sources() {
    let poster = native_poster();
    let mut controller = LiveWallpaper::new(
        Arc::new(NativePlanePlayback::default()),
        Arc::new(Posters(poster.clone())),
    );
    controller.enable_native_planes(true);
    let mut settings = LiveWallpapers {
        default: Some(setting("/fixture/first.mp4")),
        ..Default::default()
    };
    let left = MonitorId::new("poster-authority").unwrap();
    let context = WallpaperPlaybackContext::default();
    let targets = BTreeMap::from([(
        LiveWallpaperTarget::Monitor(left.clone()),
        pixel_target(16, 16),
    )]);
    controller.reconcile(
        &settings,
        std::slice::from_ref(&left),
        true,
        &context,
        &targets,
    );
    let key = controller.routes[&LiveWallpaperTarget::Monitor(left.clone())]
        .key
        .clone();
    let revision = controller.players[&key].revision;
    for (asset, tag) in [
        ("/fixture/other.mp4", revision),
        ("/fixture/first.mp4", revision + 1),
    ] {
        controller.players[&key]
            .mailbox
            .lock()
            .unwrap()
            .publish_poster(asset, tag, poster.clone());
        controller.update(
            Message::Wake(key.clone(), revision),
            &mut Effects::default(),
        );
        assert!(controller.players[&key].rgba_poster.is_none());
        assert!(controller.presentation_source(Some(&left)).is_none());
    }
    let old = controller.subscriptions().pop().unwrap();
    settings.default = Some(setting("/fixture/second.mp4"));
    controller.reconcile(
        &settings,
        std::slice::from_ref(&left),
        true,
        &context,
        &targets,
    );
    deliver_subscription(&mut controller, old);
    assert!(
        controller.presentation_source(Some(&left)).is_none(),
        "old source worker cannot admit its fallback into a new asset"
    );
    assert!(controller.players.values().all(|p| p.rgba_poster.is_none()));
}

struct MissingNativePoster;
impl AssetPort for MissingNativePoster {
    fn initial(&self) -> BTreeMap<String, Arc<ImageData>> {
        BTreeMap::new()
    }
    fn application_icons(&self, _: &[Application]) -> crate::ports::Images {
        vec![]
    }
    fn thumbnail(&self, _: &str) -> Result<Arc<ImageData>> {
        Err(DomainError::Unavailable("missing fixture poster".into()))
    }
    fn preview(&self, image: &ImageData) -> Arc<ImageData> {
        Arc::new(image.clone())
    }
    fn background(
        &self,
        _: &str,
        _: Option<crate::ports::BackgroundTarget>,
    ) -> Result<Arc<ImageData>> {
        Err(DomainError::Unavailable("missing fixture poster".into()))
    }
}
#[test]
fn native_poster_failure_uses_existing_rgba_playback_without_admitting_native_pixels() {
    let adapter = Arc::new(NativePlanePlayback::default());
    let mut controller = LiveWallpaper::new(adapter.clone(), Arc::new(MissingNativePoster));
    controller.enable_native_planes(true);
    let settings = LiveWallpapers {
        default: Some(setting("/fixture/missing-poster.mp4")),
        ..Default::default()
    };
    let left = MonitorId::new("missing-poster").unwrap();
    let context = WallpaperPlaybackContext::default();
    let targets = BTreeMap::from([(
        LiveWallpaperTarget::Monitor(left.clone()),
        pixel_target(16, 16),
    )]);
    controller.reconcile(
        &settings,
        std::slice::from_ref(&left),
        true,
        &context,
        &targets,
    );
    deliver_current_subscription(&mut controller);
    assert_eq!(
        controller
            .presentation_source(Some(&left))
            .unwrap()
            .1
            .representation(),
        "rgba"
    );
    assert_eq!(
        adapter.calls.lock().unwrap().as_slice(),
        &[(false, Some(pixel_target(16, 16)))]
    );
}
