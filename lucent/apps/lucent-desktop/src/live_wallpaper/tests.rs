use super::*;
#[test]
fn mailbox_keeps_only_latest_frame_and_one_pending_wake() {
    let stream = ImageStream::new();
    let mut mailbox = Mailbox::default();
    for n in 0..1000 {
        let frame = Arc::new(stream.frame_rgba(1, 1, vec![n as u8, 0, 0, 255]).unwrap());
        assert_eq!(mailbox.publish(Delivery::Frame(frame)), n == 0);
    }
    let Some(Delivery::Frame(image)) = mailbox.take() else {
        panic!("missing latest frame")
    };
    assert_eq!(image.pixels()[0], 999u16 as u8);
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
        fps: WallpaperFps::default(),
        paused: false,
        enabled: true,
    }
}
fn controller() -> LiveWallpaper {
    let adapters = crate::test_adapters::adapters();
    LiveWallpaper::new(adapters.playback, adapters.assets)
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
    controller.reconcile(&settings, &[id.clone(), other.clone()], true, false);
    assert_eq!(controller.players.len(), 1);
    assert_eq!(controller.subscriptions().len(), 1);
    let key = controller.routes[&LiveWallpaperTarget::Monitor(id.clone())].clone();
    let revision = controller.players[&key].revision;
    let stream = ImageStream::new();
    let image = Arc::new(stream.frame_rgba(1, 1, vec![255, 0, 0, 255]).unwrap());
    controller.players[&key]
        .mailbox
        .lock()
        .unwrap()
        .publish(Delivery::Frame(image));
    let mut effects = Effects::default();
    assert!(
        matches!(controller.update(Message::Wake(key.clone(),revision),&mut effects),Some(Intent::FrameChanged(targets)) if targets.len()==2)
    );
    assert!(controller.frame(Some(&id)).is_some());
    settings.paused = true;
    controller.reconcile(&settings, &[id.clone(), other.clone()], true, false);
    assert!(controller.subscriptions().is_empty());
    assert!(controller.frame(Some(&id)).is_some());
    assert!(
        controller
            .update(Message::Wake(key.clone(), revision), &mut effects)
            .is_none()
    );
    settings.paused = false;
    controller.reconcile(&settings, std::slice::from_ref(&id), true, false);
    assert_eq!(controller.subscriptions().len(), 1);
    assert!(controller.frame(Some(&other)).is_none());
    controller.reconcile(&settings, &[], true, false);
    assert!(controller.subscriptions().is_empty());
    assert!(controller.players.is_empty());
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
    controller.reconcile(&settings, &[id], true, false);
    assert!(controller.players.is_empty());
    let mut effects = Effects::default();
    controller.update(Message::Open, &mut effects);
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
    controller.reconcile(&settings, &[], false, false);
    let key = controller.routes[&LiveWallpaperTarget::Default].clone();
    let revision = controller.players[&key].revision;
    let image = Arc::new(ImageStream::new().frame_rgba(1, 1, vec![255; 4]).unwrap());
    controller.players[&key]
        .mailbox
        .lock()
        .unwrap()
        .publish(Delivery::Frame(image.clone()));
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
    let key = controller.routes[target].clone();
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
        .publish(Delivery::Frame(frame.clone()));
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
        matches!(controller.update(Message::Rate(captured.clone(), WallpaperFps::new(15).unwrap()), &mut effects), Some(Intent::Rate(target, rate)) if target == captured && rate.value() == 15)
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
    controller.reconcile(&settings, &[id.clone(), other.clone()], true, false);
    let original = put_frame(&mut controller, &target);
    let mut paused = settings.default.clone().unwrap();
    paused.paused = true;
    settings.monitors.insert(id.clone(), paused);
    controller.reconcile(&settings, &[id.clone(), other.clone()], true, false);
    assert!(Arc::ptr_eq(controller.frame(Some(&id)).unwrap(), &original));
    assert_eq!(controller.subscriptions().len(), 1);
    let paused_key = controller.routes[&target].clone();
    let old_revision = controller.players[&paused_key].revision;
    let imported_poster =
        Arc::new(ImageData::from_rgba("older-import-poster", 1, 1, vec![0, 0, 255, 255]).unwrap());
    controller
        .prepared
        .insert(settings.monitors[&id].asset.path.clone(), imported_poster);
    settings.monitors.get_mut(&id).unwrap().fps = WallpaperFps::new(15).unwrap();
    controller.reconcile(&settings, &[id.clone(), other.clone()], true, false);
    assert!(Arc::ptr_eq(controller.frame(Some(&id)).unwrap(), &original));
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
    controller.reconcile(&settings, &[id.clone(), other.clone()], true, false);
    let image = put_frame(&mut controller, &target);
    let key = controller.routes[&target].clone();
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
    controller.reconcile(&settings, &[], false, false);
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
