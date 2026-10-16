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
    controller.reconcile(
        &settings,
        &[id.clone(), other.clone()],
        true,
        &WallpaperPlaybackContext::default(),
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
        .publish(Delivery::Frame(image));
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
    );
    assert_eq!(controller.subscriptions().len(), 1);
    assert!(controller.frame(Some(&other)).is_none());
    controller.reconcile(&settings, &[], true, &WallpaperPlaybackContext::default());
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
    controller.reconcile(&settings, &[id], true, &WallpaperPlaybackContext::default());
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
    controller.reconcile(&settings, &[], false, &WallpaperPlaybackContext::default());
    let key = controller.routes[&LiveWallpaperTarget::Default].key.clone();
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
    controller.reconcile(
        &settings,
        &[id.clone(), other.clone()],
        true,
        &WallpaperPlaybackContext::default(),
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
    settings.monitors.get_mut(&id).unwrap().fps = WallpaperFps::new(15).unwrap();
    controller.reconcile(
        &settings,
        &[id.clone(), other.clone()],
        true,
        &WallpaperPlaybackContext::default(),
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
    controller.reconcile(&settings, &[], false, &WallpaperPlaybackContext::default());
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
    controller.reconcile(&settings, &monitors, true, &context);
    let original = put_frame(&mut controller, &left_target);
    let key = controller.routes[&left_target].key.clone();
    let revision = controller.players[&key].revision;
    context.fullscreen_monitors.insert(left.clone());
    controller.reconcile(&settings, &monitors, true, &context);
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
        .publish(Delivery::Frame(later.clone()));
    let notice = controller.update(
        Message::Wake(key.clone(), revision),
        &mut Effects::default(),
    );
    assert!(
        matches!(notice, Some(Intent::FrameChanged(targets)) if targets == [right_target.clone()])
    );
    controller.reconcile(&settings, &monitors, true, &context);
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
    controller.reconcile(&settings, &monitors, true, &context);
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
    controller.reconcile(&settings, &monitors, true, &context);
    assert_eq!(controller.subscriptions().len(), 1);
    assert!(Arc::ptr_eq(controller.frame(Some(&left)).unwrap(), &held));
    settings.policy.pause_when_fullscreen = false;
    controller.reconcile(&settings, &monitors, true, &context);
    assert!(
        controller
            .routes
            .values()
            .all(|route| route.held.is_none() && route.pause.is_none())
    );
    assert_eq!(controller.subscriptions().len(), 1);
    controller.reconcile(&settings, &[], true, &context);
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
