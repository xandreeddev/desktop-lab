use super::*;
use lucent_domain::MediaFrame;

fn state() -> State {
    State::new(Arc::new(vec![0, 1_000_000_000]), (1, 1))
}
fn frame(revision: u64, pts: u64, size: (u32, u32), aspect: (u32, u32)) -> TargetedMediaFrame {
    TargetedMediaFrame {
        revision: WallpaperTargetRevision::new(revision).unwrap(),
        frame: MediaFrame::rgba_with_aspect(
            size.0,
            size.1,
            vec![7; (size.0 * size.1 * 4) as usize],
            aspect,
            Duration::from_nanos(pts),
        )
        .unwrap(),
    }
}
fn raw_input() -> Value {
    json!({"state":"/private/generated","asset":{
        "path":"/private/generated/media-generated/source.mp4",
        "source":{"name":"Generated","path":"/private/generated/media-generated/source.mp4"},
        "poster":{"name":"Generated","path":"/private/generated/media-generated/poster.png"}},
        "mode":"retarget","sequence":"source_and_targets","phase_seconds":12,
        "expected_pts_ns":(0..240).map(|index| index*16_666_667u64).collect::<Vec<_>>()})
}

#[test]
fn full_demand_sequence_admits_source_distinct_from_missing_snapshot_and_advances_exactly() {
    let control = LatestWallpaperTargetAdapter::new(None);
    let initial = control.try_latest().unwrap().unwrap();
    assert_eq!(initial.target, None);
    let mut revision = initial.revision;
    for (index, &(_, demand)) in Sequence::SourceAndTargets
        .phases()
        .iter()
        .enumerate()
        .skip(1)
    {
        let value = control.set(revision, target(demand).unwrap()).unwrap();
        assert_eq!(value.revision.get(), index as u64 + 1);
        assert_eq!(value.target, target(demand).unwrap());
        assert_eq!(control.set(value.revision, value.target).unwrap(), value);
        revision = value.revision;
    }
    assert_eq!(revision.get(), 5);
}

#[test]
fn known_sequence_starts_with_cold_fhd_and_never_admits_source_sized_demand() {
    let phases = Sequence::KnownTargets.phases();
    assert_eq!(phases.len(), 3);
    assert_eq!(phases[0], ("first_fhd", Some((1920, 1080))));
    assert_eq!(phases[2], ("restored_fhd", Some((1920, 1080))));
    let control = LatestWallpaperTargetAdapter::new(target(phases[0].1).unwrap());
    let initial = control.try_latest().unwrap().unwrap();
    let state = State::new(Arc::new(vec![0]), phases[0].1.unwrap());
    assert_eq!(state.size, (1920, 1080));
    assert!(state.latest.is_none());
    assert_eq!(state.metrics.count, 0);
    assert_eq!(initial.target.unwrap().size(), (1920, 1080));
    let mut revision = initial.revision;
    for (index, &(_, demand)) in phases.iter().enumerate().skip(1) {
        let admitted = control.set(revision, target(demand).unwrap()).unwrap();
        assert!(admitted.target.is_some());
        assert_eq!(admitted.revision.get(), index as u64 + 1);
        revision = admitted.revision;
    }
    assert_eq!(revision.get(), 3);
}

#[test]
fn sequence_is_explicitly_typed_and_cannot_silently_substitute_the_warm_source_matrix() {
    for (name, expected) in [
        ("known_targets", Sequence::KnownTargets),
        ("source_and_targets", Sequence::SourceAndTargets),
    ] {
        let mut value = raw_input();
        value["sequence"] = json!(name);
        let admitted = input(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(admitted.sequence, expected);
        assert_eq!(
            serde_json::to_value(admitted.sequence).unwrap(),
            json!(name)
        );
    }
    let mut absent = raw_input();
    absent.as_object_mut().unwrap().remove("sequence");
    assert!(input(&serde_json::to_vec(&absent).unwrap()).is_err());
    let mut unknown = raw_input();
    unknown["sequence"] = json!("guess_from_target");
    assert!(input(&serde_json::to_vec(&unknown).unwrap()).is_err());
}

#[test]
fn importer_input_is_bounded_and_separates_original_source_from_owned_storage() {
    let value = json!({"state":"/private/owned","source":{"name":"Generated 4K60","path":"/private/input/source.mp4"}});
    assert!(preparation(&serde_json::to_vec(&value).unwrap()).is_ok());
    for mutation in [
        "inside_state",
        "relative",
        "parent",
        "unknown",
        "nested_unknown",
    ] {
        let mut changed = value.clone();
        match mutation {
            "inside_state" => {
                changed["source"]["path"] = json!("/private/owned/media-generated/source.mp4")
            }
            "relative" => changed["source"]["path"] = json!("input/source.mp4"),
            "parent" => changed["state"] = json!("/private/../owned"),
            "unknown" => changed["skip_metadata"] = json!(true),
            _ => changed["source"]["skip_validation"] = json!(true),
        }
        assert!(
            preparation(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "{mutation}"
        );
    }
    assert!(preparation(&vec![b' '; 65537]).is_err());
    // Real prepare creates a unique media-* directory, not media-generated.
    let mut observed = raw_input();
    observed["asset"]["path"] = json!("/private/generated/media-123-1/source.mp4");
    observed["asset"]["source"]["path"] = json!("/private/input/source.mp4");
    observed["asset"]["poster"]["path"] = json!("/private/generated/media-123-1/poster.png");
    assert!(input(&serde_json::to_vec(&observed).unwrap()).is_ok());
}

#[test]
fn importer_invokes_prepare_once_and_never_synthesizes_an_asset_or_starts_playback() {
    struct Importer {
        calls: std::sync::atomic::AtomicU64,
        fail: bool,
    }
    impl WallpaperPlaybackPort for Importer {
        fn prepare(&self, source: &Wallpaper) -> lucent_domain::Result<LiveWallpaperAsset> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(lucent_domain::DomainError::Unavailable(
                    "private native error".into(),
                ));
            }
            Ok(LiveWallpaperAsset {
                source: source.clone(),
                path: "/private/owned/media-123-1/source.mp4".into(),
                poster: Wallpaper {
                    name: source.name.clone(),
                    path: "/private/owned/media-123-1/poster.png".into(),
                },
            })
        }
        fn play(
            &self,
            _: &LiveWallpaperAsset,
            _: PlaybackOptions,
            _: &mut dyn FnMut(lucent_domain::Result<MediaFrame>) -> bool,
            _: Arc<dyn StopSignal>,
        ) {
            panic!("import process cannot start a measured worker");
        }
    }
    let source = Wallpaper {
        name: "Generated 4K60".into(),
        path: "/private/input/source.mp4".into(),
    };
    for fail in [false, true] {
        let importer = Importer {
            calls: std::sync::atomic::AtomicU64::new(0),
            fail,
        };
        let result = prepare_asset(&importer, &source);
        assert_eq!(importer.calls.load(Ordering::SeqCst), 1);
        if fail {
            assert_eq!(result.err(), Some("asset_prepare_failed"));
        } else {
            let prepared = result.unwrap();
            assert_eq!(prepared.source, source);
            assert_eq!(prepared.path, "/private/owned/media-123-1/source.mp4");
            assert_ne!(prepared.path, source.path);
        }
    }
}

#[test]
fn diagnostic_prefixes_are_explicit_and_never_shorten_a_canonical_case() {
    let canonical = input(&serde_json::to_vec(&raw_input()).unwrap()).unwrap();
    assert!(canonical.diagnostic_stop_after.is_none());
    assert_eq!(canonical.phases().len(), 5);
    for count in [2, 4] {
        let mut raw = raw_input();
        raw["diagnostic_stop_after"] = json!(count);
        let admitted = input(&serde_json::to_vec(&raw).unwrap()).unwrap();
        assert_eq!(admitted.phases().len(), count);
        assert_eq!(admitted.phases().last().unwrap().1, Some((1920, 1080)));
        raw["mode"] = json!("reopen");
        assert!(input(&serde_json::to_vec(&raw).unwrap()).is_err());
        raw["mode"] = json!("retarget");
        raw["sequence"] = json!("known_targets");
        assert!(input(&serde_json::to_vec(&raw).unwrap()).is_err());
    }
    for invalid in [
        json!(0),
        json!(1),
        json!(3),
        json!(5),
        json!(99),
        json!(true),
    ] {
        let mut raw = raw_input();
        raw["diagnostic_stop_after"] = invalid;
        assert!(input(&serde_json::to_vec(&raw).unwrap()).is_err());
    }
}

#[test]
fn ownership_snapshots_distinguish_old_latest_and_callback_without_pixel_references() {
    let mut state = state();
    state.accept(frame(1, 0, (1, 1), (3840, 2160))).unwrap();
    let strong = Arc::strong_count(&state.latest.as_ref().unwrap()._pixels);
    let weak = Arc::weak_count(&state.latest.as_ref().unwrap()._pixels);
    let current = state.ownership("target_observed", Duration::ZERO).unwrap();
    assert_eq!(current["current_bytes"], 4);
    assert_eq!(current["old_last_good_bytes"], 0);
    assert_eq!(
        Arc::strong_count(&state.latest.as_ref().unwrap()._pixels),
        strong
    );
    assert_eq!(
        Arc::weak_count(&state.latest.as_ref().unwrap()._pixels),
        weak
    );
    state
        .request(WallpaperTargetRevision::new(2).unwrap(), (2, 1))
        .unwrap();
    let old = state.ownership("after_request", Duration::ZERO).unwrap();
    assert_eq!(old["latest_revision"], 1);
    assert_eq!(old["current_bytes"], 0);
    assert_eq!(old["old_last_good_bytes"], 4);
    let callback = state.callback_frame.clone(); // Metadata Arc, never pixels.
    let incoming = frame(2, 1_000_000_000, (2, 1), (3840, 2160));
    let footprint = CallbackGuard::enter(&callback, Some(&incoming)).unwrap();
    let pending = state.ownership("after_request", Duration::ZERO).unwrap();
    assert_eq!(pending["callback_inflight_revision"], 2);
    assert_eq!(pending["callback_inflight_bytes"], 8);
    state.accept(incoming).unwrap();
    drop(footprint);
    let current = state.ownership("end_target", Duration::ZERO).unwrap();
    assert_eq!(current["latest_revision"], 2);
    assert_eq!(current["current_bytes"], 8);
    assert_eq!(current["old_last_good_bytes"], 0);
    assert_eq!(current["callback_inflight_bytes"], 0);
    assert_eq!(
        Arc::strong_count(&state.latest.as_ref().unwrap()._pixels),
        1
    );
    assert_eq!(Arc::weak_count(&state.latest.as_ref().unwrap()._pixels), 0);
}

#[test]
fn callback_footprint_clears_for_stale_error_and_unwind_without_retaining_pixels() {
    let mut state = state();
    state
        .request(WallpaperTargetRevision::new(2).unwrap(), (1, 1))
        .unwrap();
    let callback = state.callback_frame.clone();
    let stale = frame(1, 0, (1, 1), (3840, 2160));
    let guard = CallbackGuard::enter(&callback, Some(&stale)).unwrap();
    state.accept(stale).unwrap();
    drop(guard);
    assert_eq!(state.stale, 1);
    assert!(callback.lock().unwrap().is_none());
    let error = CallbackGuard::enter(&callback, None).unwrap();
    drop(error);
    assert!(callback.lock().unwrap().is_none());
    let value = frame(2, 0, (1, 1), (3840, 2160));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = CallbackGuard::enter(&callback, Some(&value)).unwrap();
        panic!("injected metadata scope failure");
    }));
    assert!(result.is_err());
    assert!(callback.lock().unwrap().is_none());
}

#[test]
fn readiness_accepts_before_five_seconds_and_rejects_ready_at_and_after_boundary() {
    assert_eq!(
        ready_elapsed(true, READY - Duration::from_nanos(1)),
        Ok(Some(READY - Duration::from_nanos(1)))
    );
    assert_eq!(
        ready_elapsed(false, READY - Duration::from_nanos(1)),
        Ok(None)
    );
    for elapsed in [READY, READY + Duration::from_secs(1)] {
        assert_eq!(ready_elapsed(true, elapsed), Err("target_not_observed"));
        assert_eq!(ready_elapsed(false, elapsed), Err("target_not_observed"));
    }
}

#[test]
fn input_requires_exact_generated_asset_phase_duration_and_actual_ordered_pts() {
    assert!(input(&serde_json::to_vec(&raw_input()).unwrap()).is_ok());
    for mutation in [
        "duration",
        "extra",
        "duplicate_pts",
        "poster",
        "source_extra",
    ] {
        let mut raw = raw_input();
        match mutation {
            "duration" => raw["phase_seconds"] = json!(11),
            "extra" => raw["path"] = json!("private"),
            "duplicate_pts" => raw["expected_pts_ns"][1] = json!(0),
            "poster" => raw["asset"]["poster"]["path"] = json!("/elsewhere/poster.png"),
            _ => raw["asset"]["source"]["extra"] = json!(true),
        }
        assert!(
            input(&serde_json::to_vec(&raw).unwrap()).is_err(),
            "{mutation}"
        );
    }
    assert!(input(&vec![b' '; 65537]).is_err());
}

#[test]
fn publication_checks_real_byte_extent_aspect_and_normalized_ffprobe_pts() {
    let mut state = state();
    state
        .accept(frame(1, 1_000_000_000, (1, 1), (3840, 2160)))
        .unwrap();
    assert_eq!(state.metrics.rgba_bytes, Some(4));
    assert_eq!(state.metrics.first_pts_ns, Some(1_000_000_000));
    assert_eq!(state.metrics.last_size, Some((1, 1)));
    assert_eq!(state.metrics.count, 1);
    assert!(state.latest.as_ref().unwrap().capacity >= 4);
}

#[test]
fn stale_revision_rejection_keeps_last_good_and_does_not_validate_or_relabel_old_geometry() {
    let mut state = state();
    state.accept(frame(1, 0, (1, 1), (3840, 2160))).unwrap();
    let held = state.latest.as_ref().unwrap()._pixels.clone();
    state
        .request(WallpaperTargetRevision::new(2).unwrap(), (2, 1))
        .unwrap();
    state.accept(frame(1, 333, (1, 1), (1, 1))).unwrap();
    assert_eq!(state.stale, 1);
    assert_eq!(state.metrics.count, 0);
    assert!(Arc::ptr_eq(&held, &state.latest.as_ref().unwrap()._pixels));
    state
        .accept(frame(2, 1_000_000_000, (2, 1), (3840, 2160)))
        .unwrap();
    assert_eq!(state.metrics.count, 1);
    assert!(!Arc::ptr_eq(&held, &state.latest.as_ref().unwrap()._pixels));
}

#[test]
fn invalid_geometry_aspect_or_pts_never_replaces_last_good() {
    for (size, aspect, pts) in [
        ((2, 1), (3840, 2160), 0),
        ((1, 1), (16, 9), 0),
        ((1, 1), (3840, 2160), 100),
    ] {
        let mut state = state();
        state.accept(frame(1, 0, (1, 1), (3840, 2160))).unwrap();
        let held = state.latest.as_ref().unwrap()._pixels.clone();
        assert_eq!(
            state.accept(frame(1, pts, size, aspect)),
            Err("frame_contract_failed")
        );
        assert!(Arc::ptr_eq(&held, &state.latest.as_ref().unwrap()._pixels));
        assert_eq!(state.metrics.count, 1);
    }
}

#[test]
fn target_change_preserves_source_clock_and_last_good_but_resets_phase_metrics() {
    let mut state = state();
    state
        .accept(frame(1, 1_000_000_000, (1, 1), (3840, 2160)))
        .unwrap();
    state
        .request(WallpaperTargetRevision::new(2).unwrap(), (1, 1))
        .unwrap();
    assert!(state.latest.is_some());
    assert_eq!(state.previous_pts_ns, Some(1_000_000_000));
    assert_eq!(state.metrics.count, 0);
    state.accept(frame(2, 0, (1, 1), (3840, 2160))).unwrap();
    assert_eq!(state.observed_loop_wraps, 1);
}

#[test]
fn failed_or_nonadvancing_target_request_does_not_clear_error_or_last_good() {
    let mut state = state();
    state.accept(frame(1, 0, (1, 1), (3840, 2160))).unwrap();
    assert_eq!(
        state.request(WallpaperTargetRevision::initial(), (1, 1)),
        Err("non_advancing_revision")
    );
    state.error = Some("adapter_error");
    assert_eq!(
        state.request(WallpaperTargetRevision::new(2).unwrap(), (2, 1)),
        Err("adapter_error")
    );
    assert_eq!(state.expected.get(), 1);
    assert!(state.latest.is_some());
    assert_eq!(state.error, Some("adapter_error"));
}

struct WaitingAdapter {
    static_calls: std::sync::atomic::AtomicU64,
    retarget_calls: std::sync::atomic::AtomicU64,
}
impl WallpaperPlaybackPort for WaitingAdapter {
    fn prepare(&self, _: &lucent_domain::Wallpaper) -> lucent_domain::Result<LiveWallpaperAsset> {
        unreachable!("experiment never imports or mutates preferences")
    }
    fn play(
        &self,
        _: &LiveWallpaperAsset,
        _: PlaybackOptions,
        _: &mut dyn FnMut(lucent_domain::Result<MediaFrame>) -> bool,
        stop: Arc<dyn StopSignal>,
    ) {
        self.static_calls.fetch_add(1, Ordering::SeqCst);
        while !stop.cancelled() {
            stop.wait(Duration::from_millis(1));
        }
    }
    fn play_retargetable(
        &self,
        _: &LiveWallpaperAsset,
        _: RetargetablePlaybackOptions,
        _: Arc<dyn WallpaperTargetPort>,
        _: &mut dyn FnMut(lucent_domain::Result<TargetedMediaFrame>) -> bool,
        stop: Arc<dyn StopSignal>,
    ) {
        self.retarget_calls.fetch_add(1, Ordering::SeqCst);
        while !stop.cancelled() {
            stop.wait(Duration::from_millis(1));
        }
    }
}

#[test]
fn exact_worker_join_observes_cancellation_and_selects_only_one_port_entrypoint() {
    for mode in [Mode::Reopen, Mode::Retarget] {
        let adapter = Arc::new(WaitingAdapter {
            static_calls: std::sync::atomic::AtomicU64::new(0),
            retarget_calls: std::sync::atomic::AtomicU64::new(0),
        });
        let asset = input(&serde_json::to_vec(&raw_input()).unwrap())
            .unwrap()
            .asset;
        let worker = spawn(
            adapter.clone(),
            asset,
            mode,
            Arc::new(LatestWallpaperTargetAdapter::new(None)),
            Arc::new(Mutex::new(state())),
            7,
        )
        .unwrap();
        let result = join(worker).unwrap();
        assert_eq!(result["worker_ordinal"], 7);
        assert_eq!(result["watch_joined"], true);
        assert_eq!(result["cancellation_observed"], true);
        assert_eq!(
            adapter.static_calls.load(Ordering::SeqCst),
            u64::from(mode == Mode::Reopen)
        );
        assert_eq!(
            adapter.retarget_calls.load(Ordering::SeqCst),
            u64::from(mode == Mode::Retarget)
        );
    }
}

#[test]
fn early_worker_return_cannot_become_a_successful_cancelled_join() {
    let handle = thread::spawn(|| false);
    while !handle.is_finished() {
        thread::yield_now();
    }
    let result = join(Worker {
        stop: Arc::new(Stop::new()),
        handle,
        ordinal: 1,
    });
    assert_eq!(result.err(), Some("worker_ended_early"));
}

#[test]
fn worker_panic_is_terminal_instead_of_authorizing_replacement() {
    let handle = thread::spawn(|| panic!("private fake worker panic"));
    let result = join(Worker {
        stop: Arc::new(Stop::new()),
        handle,
        ordinal: 1,
    });
    assert_eq!(result.err(), Some("worker_panicked"));
}
