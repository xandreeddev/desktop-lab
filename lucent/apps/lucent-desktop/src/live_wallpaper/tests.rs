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
        Message::ImportPath(None, "/fixture/clip.mp4".into()),
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
        Message::ImportPath(None, "/fixture/clip.mp4".into()),
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
    let key = controller.routes[&Some(id.clone())].clone();
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
        Message::ImportPath(None, "/fixture/clip.mp4".into()),
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
    let key = controller.routes[&None].clone();
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
