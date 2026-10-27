use super::*;
use lucent_domain::MediaFrame;

fn state() -> State {
    let mut state = State::new(Arc::new(vec![0, 1_000_000_000]));
    state.size = (1, 1);
    state
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
        "mode":"retarget","phase_seconds":12,
        "expected_pts_ns":(0..240).map(|index| index*16_666_667u64).collect::<Vec<_>>()})
}

#[test]
fn full_demand_sequence_admits_source_distinct_from_missing_snapshot_and_advances_exactly() {
    let control = LatestWallpaperTargetAdapter::new(None);
    let initial = control.try_latest().unwrap().unwrap();
    assert_eq!(initial.target, None);
    let mut revision = initial.revision;
    for (index, (_, demand)) in PHASES.into_iter().enumerate().skip(1) {
        let value = control.set(revision, target(demand).unwrap()).unwrap();
        assert_eq!(value.revision.get(), index as u64 + 1);
        assert_eq!(value.target, target(demand).unwrap());
        assert_eq!(control.set(value.revision, value.target).unwrap(), value);
        revision = value.revision;
    }
    assert_eq!(revision.get(), 5);
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
