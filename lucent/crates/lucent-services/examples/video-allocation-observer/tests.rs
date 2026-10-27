use super::*;
use lucent_domain::{DomainError, Result, Wallpaper};
use std::sync::{Weak, atomic::AtomicUsize};

fn descriptor() -> Value {
    json!({"state":"/private/library with spaces", "phase_seconds":10,
        "asset":{"source":{"path":"/private/original file.mp4", "name":"Private name"},
                 "path":"/private/library with spaces/media-owned/clip 雪.mp4",
                 "poster":{"path":"/private/library with spaces/media-owned/poster.png", "name":"Poster"}}})
}
fn request() -> Input {
    input(&serde_json::to_vec(&descriptor()).unwrap()).unwrap()
}
fn frame(pixels: Arc<Vec<u8>>) -> MediaFrame {
    MediaFrame::rgba_shared_with_aspect(1, 1, pixels, (1, 1), Duration::ZERO).unwrap()
}

#[test]
fn arguments_are_inert_without_explicit_run_and_do_not_take_paths_from_argv() {
    assert!(!arguments([]).unwrap());
    assert!(arguments(["--run".into()]).unwrap());
    for args in [
        vec!["--run", "--run"],
        vec!["--file", "/private/name"],
        vec!["--seconds", "999"],
    ] {
        assert_eq!(
            arguments(args.into_iter().map(str::to_owned)),
            Err("use_run_with_private_json_stdin")
        );
    }
}

#[test]
fn input_is_bounded_known_schema_and_allows_legitimate_local_names() {
    let original = descriptor();
    assert_eq!(request().phase_seconds, 10);
    assert!(request().asset.path.contains('雪'));
    let mut values = Vec::new();
    for seconds in [0, 31, u64::MAX] {
        let mut bad = original.clone();
        bad["phase_seconds"] = json!(seconds);
        values.push(bad);
    }
    for (field, value) in [
        ("state", json!("relative")),
        ("credential", json!("private-token")),
    ] {
        let mut bad = original.clone();
        bad[field] = value;
        values.push(bad);
    }
    let mut extra = original.clone();
    extra["asset"]["ignored_secret"] = json!("private");
    values.push(extra);
    let mut parent = original.clone();
    parent["asset"]["path"] = json!("/private/library with spaces/media-owned/../clip.mp4");
    values.push(parent);
    let mut foreign = original.clone();
    foreign["asset"]["poster"]["path"] = json!("/private/foreign/poster.png");
    values.push(foreign);
    for bad in values {
        assert_eq!(
            input(&serde_json::to_vec(&bad).unwrap()).err(),
            Some("invalid_input")
        );
    }
    assert_eq!(
        input(&vec![b' '; INPUT_BYTES + 1]).err(),
        Some("bounded_input_required")
    );
    assert_eq!(
        input(b"private incomplete JSON").err(),
        Some("invalid_input")
    );
}

#[test]
fn allocator_statistics_require_a_boolean_opt_in_and_default_to_disabled() {
    assert!(!request().allocator_stats);
    assert!(!request().retarget);
    let mut retarget = descriptor();
    retarget["retarget"] = json!(true);
    assert!(
        input(&serde_json::to_vec(&retarget).unwrap())
            .unwrap()
            .retarget
    );
    retarget["retarget"] = json!("true");
    assert_eq!(
        input(&serde_json::to_vec(&retarget).unwrap()).err(),
        Some("invalid_input")
    );
    for enabled in [false, true] {
        let mut value = descriptor();
        value["allocator_stats"] = json!(enabled);
        assert_eq!(
            input(&serde_json::to_vec(&value).unwrap())
                .unwrap()
                .allocator_stats,
            enabled
        );
    }
    for invalid in [json!("true"), json!(1), Value::Null] {
        let mut value = descriptor();
        value["allocator_stats"] = invalid;
        assert_eq!(
            input(&serde_json::to_vec(&value).unwrap()).err(),
            Some("invalid_input")
        );
    }
}

fn accounting() -> MallocAccounting {
    MallocAccounting {
        arena: 64,
        in_use: 17,
        free: 47,
        direct_mmap: 99,
        direct_mmap_regions: 3,
        free_chunks: 5,
    }
}

#[test]
fn allocator_accounting_separates_arena_direct_mmap_bytes_and_counts() {
    let result = allocator_report("2.42", accounting());
    assert_eq!(
        result,
        json!({"kind":"glibc_mallinfo2", "available":true, "glibc_version":"2.42",
            "bytes":{"arena":64, "arena_accounted_in_use":17, "arena_free":47,
                     "direct_mmap":99, "malloc_accounted_in_use_total":116},
            "counts":{"arena_free_chunks":5, "direct_mmap_regions":3}})
    );
    let mut empty = accounting();
    empty.arena = 0;
    empty.in_use = 0;
    empty.free = 0;
    empty.direct_mmap = 0;
    empty.direct_mmap_regions = 0;
    empty.free_chunks = 0;
    let empty = allocator_report("2.33", empty);
    assert_eq!(empty["available"], true);
    assert_eq!(empty["bytes"]["malloc_accounted_in_use_total"], 0);
    assert_ne!(empty, allocator_unavailable("unsupported_target"));
}

#[test]
fn allocator_totals_never_wrap_and_invalid_runtime_versions_stay_private() {
    let mut large = accounting();
    large.in_use = u64::MAX;
    let result = allocator_report("2.42.1", large);
    assert_eq!(result["bytes"]["arena_accounted_in_use"], u64::MAX);
    assert!(result["bytes"]["malloc_accounted_in_use_total"].is_null());
    for version in ["", "2", "2.private-device", "/private/2.42", "2..42"] {
        let result = allocator_report(version, accounting());
        assert_eq!(result["available"], false);
        assert_eq!(result["reason"], "invalid_runtime_version");
        assert!(!result.to_string().contains("private"));
        assert!(result.get("bytes").is_none());
    }
    assert_eq!(
        allocator_report("2.32", accounting())["reason"],
        "unsupported_glibc_version"
    );
}

#[test]
fn unsupported_allocator_observation_is_explicit_and_never_polled_during_play() {
    let mut calls = 0;
    let mut memory = || json!({"rss_kib":4});
    let mut query = || {
        calls += 1;
        allocator_unavailable("unsupported_target")
    };
    let mut samples = Samples {
        memory: &mut memory,
        allocator: Some(&mut query),
    };
    let before = samples.capture(Moment::BeforePlay, 0);
    assert_eq!(before["memory"], json!({"rss_kib":4}));
    assert_eq!(before["allocator"]["available"], false);
    assert_eq!(before["allocator"]["reason"], "unsupported_target");
    for elapsed in [250, 500, 750] {
        let during = samples.capture(Moment::DuringPlay, elapsed);
        assert!(during.get("allocator").is_none());
        assert_eq!(during["memory"], json!({"rss_kib":4}));
    }
    assert_eq!(calls, 1);
}

#[test]
fn latest_replacement_retains_only_current_domain_storage_and_actual_capacity() {
    let mut first = Vec::with_capacity(128);
    first.extend_from_slice(&[1, 2, 3, 255]);
    let first = Arc::new(first);
    let old = Arc::downgrade(&first);
    let capacity = first.capacity();
    let mut publications = Publications::default();
    publications.accept(frame(first), None).unwrap();
    assert_eq!(old.strong_count(), 1);
    assert_eq!(publications.latest.as_ref().unwrap().capacity, capacity);
    let second = Arc::new(vec![4, 5, 6, 255]);
    let current = Arc::downgrade(&second);
    publications.accept(frame(second), None).unwrap();
    assert!(old.upgrade().is_none());
    assert_eq!(current.strong_count(), 1);
    publications.latest = None;
    assert!(current.upgrade().is_none());
    assert_eq!(publications.count, 2);
}

#[test]
fn optional_sdk_bridge_shares_opaque_storage_and_preserves_shared_alpha_owner() {
    let stream = ImageStream::new();
    let opaque = Arc::new(vec![100, 50, 20, 255]);
    let latest = Latest::new(frame(opaque.clone()), Some(&stream)).unwrap();
    let Pixels::Sdk(image) = latest.pixels else {
        panic!("wrong consumer mode")
    };
    assert!(Arc::ptr_eq(&opaque, &image.levels()[0].pixels));
    let alpha = Arc::new(vec![100, 50, 20, 128]);
    let latest = Latest::new(frame(alpha.clone()), Some(&stream)).unwrap();
    let Pixels::Sdk(image) = latest.pixels else {
        panic!("wrong consumer mode")
    };
    assert!(!Arc::ptr_eq(&alpha, &image.levels()[0].pixels));
    assert_eq!(alpha.as_slice(), [100, 50, 20, 128]);
    assert_eq!(image.levels()[0].pixels.as_slice(), [50, 25, 10, 128]);
}

#[test]
fn memory_status_reports_missing_invalid_or_duplicate_fields_as_unavailable() {
    let result =
        memory_status("Name:\tprivate media name\nVmRSS:\t120 kB\nRssAnon:\t0 kB\nVmSwap:\t4 kB\n");
    assert_eq!(
        result,
        json!({"rss_kib":120,"anonymous_kib":0,"swap_kib":4,"rss_high_water_kib":null})
    );
    assert!(!result.to_string().contains("private"));
    for raw in [
        "VmRSS: 1 kB\nVmRSS: 1 kB",
        "VmRSS: -1 kB",
        "VmRSS: 1 bytes",
        "VmRSS: 9999999999999999999999999 kB",
    ] {
        assert!(memory_status(raw)["rss_kib"].is_null());
    }
}

#[derive(Default)]
struct Fake {
    active: AtomicUsize,
    calls: AtomicUsize,
    order: Mutex<Vec<(&'static str, (u32, u32))>>,
    pixels: Mutex<Vec<Weak<Vec<u8>>>>,
    error: bool,
    early: bool,
    panic: bool,
    ignore_stop: AtomicBool,
}
impl WallpaperPlaybackPort for Fake {
    fn prepare(&self, _: &Wallpaper) -> Result<LiveWallpaperAsset> {
        panic!("diagnostic must never import media")
    }
    fn play(
        &self,
        _: &LiveWallpaperAsset,
        options: PlaybackOptions,
        emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
        stop: Arc<dyn StopSignal>,
    ) {
        assert_eq!(
            self.active.fetch_add(1, Ordering::SeqCst),
            0,
            "overlapping old worker"
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
        let size = options.target.unwrap().size();
        self.order.lock().unwrap().push(("start", size));
        assert!(matches!(options.fps, WallpaperFrameRate::Source));
        assert!(options.looping);
        if self.panic {
            self.active.fetch_sub(1, Ordering::SeqCst);
            panic!("injected private backend error");
        }
        if self.error {
            emit(Err(DomainError::Failed(
                "private native source path".into(),
            )));
        } else {
            while !stop.cancelled() || self.ignore_stop.load(Ordering::Acquire) {
                let pixels = Arc::new(vec![10, 20, 30, 255]);
                self.pixels.lock().unwrap().push(Arc::downgrade(&pixels));
                if !emit(Ok(frame(pixels))) && !self.ignore_stop.load(Ordering::Acquire) {
                    break;
                }
                if self.early {
                    break;
                }
                if self.ignore_stop.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(1));
                } else {
                    stop.wait(Duration::from_millis(1));
                }
            }
        }
        self.order.lock().unwrap().push(("end", size));
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[test]
fn matrix_cancels_and_joins_each_watch_before_drop_and_next_phase_for_both_consumers() {
    for sdk in [false, true] {
        let adapter = Arc::new(Fake::default());
        let mut request = request();
        request.sdk_bridge = sdk;
        let mut reports = Vec::new();
        observe(
            adapter.clone(),
            request,
            Duration::from_millis(15),
            Duration::from_secs(1),
            &mut || json!({"rss_kib":null}),
            &mut || panic!("disabled allocator callback must not run"),
            &mut |value| {
                reports.push(value);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            *adapter.order.lock().unwrap(),
            vec![
                ("start", (1920, 1080)),
                ("end", (1920, 1080)),
                ("start", (3840, 2160)),
                ("end", (3840, 2160)),
                ("start", (1920, 1080)),
                ("end", (1920, 1080))
            ]
        );
        assert!(
            adapter
                .pixels
                .lock()
                .unwrap()
                .iter()
                .all(|pixels| pixels.upgrade().is_none())
        );
        assert_eq!(reports.len(), 4);
        for report in reports.iter().take(3) {
            assert!(report["result"]["publications"].as_u64().unwrap() > 0);
            assert_eq!(report["result"]["watch_joined"], true);
            assert_eq!(
                report["result"]["consumer_held_pixel_capacity_after_drop"],
                0
            );
            assert_eq!(
                report["result"]["memory_samples"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()["moment"],
                "after_latest_dropped"
            );
            assert!(
                report["result"]["memory_samples"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|sample| sample.get("allocator").is_none())
            );
        }
        assert_eq!(reports[3]["performance_accepted"], false);
        assert!(!serde_json::to_string(&reports).unwrap().contains("private"));
    }
}

#[test]
fn enabled_allocator_queries_run_only_before_start_after_join_and_after_latest_drop() {
    let adapter = Arc::new(Fake::default());
    let mut request = request();
    request.allocator_stats = true;
    let mut calls = 0;
    let mut reports = Vec::new();
    observe(
        adapter.clone(),
        request,
        Duration::from_millis(15),
        Duration::from_secs(1),
        &mut || json!({"rss_kib":4}),
        &mut || {
            assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
            let held = adapter
                .pixels
                .lock()
                .unwrap()
                .iter()
                .filter(|pixels| pixels.upgrade().is_some())
                .count();
            assert_eq!(held, usize::from(calls % 3 == 1));
            calls += 1;
            allocator_report("2.42", accounting())
        },
        &mut |value| {
            reports.push(value);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(calls, 9);
    for report in reports.iter().take(3) {
        let samples = report["result"]["memory_samples"].as_array().unwrap();
        let allocator_samples: Vec<_> = samples
            .iter()
            .filter(|sample| sample.get("allocator").is_some())
            .map(|sample| sample["moment"].as_str().unwrap())
            .collect();
        assert_eq!(
            allocator_samples,
            [
                "before_play",
                "after_join_latest_held",
                "after_latest_dropped"
            ]
        );
        assert!(
            samples
                .iter()
                .all(|sample| sample["memory"]["rss_kib"] == 4)
        );
    }
}

#[test]
fn errors_panics_and_early_return_stop_sequence_without_native_error_output() {
    for (error, early, panic, expected) in [
        (true, false, false, "adapter_error"),
        (false, true, false, "worker_ended_before_finite_observation"),
        (false, false, true, "worker_panicked"),
    ] {
        let adapter = Arc::new(Fake {
            error,
            early,
            panic,
            ..Default::default()
        });
        let mut request = request();
        request.allocator_stats = true;
        let mut allocator_calls = 0;
        let mut reports = Vec::new();
        let result = observe(
            adapter.clone(),
            request,
            Duration::from_millis(20),
            Duration::from_secs(1),
            &mut || json!({"rss_kib":null}),
            &mut || {
                assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
                allocator_calls += 1;
                allocator_report("2.42", accounting())
            },
            &mut |value| {
                reports.push(value);
                Ok(())
            },
        );
        assert_eq!(result, Err(expected));
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
        assert_eq!(allocator_calls, 1);
        assert!(reports.is_empty());
        assert!(!expected.contains("private"));
    }
}

#[test]
fn missing_join_confirmation_never_starts_another_phase() {
    let adapter = Arc::new(Fake::default());
    adapter.ignore_stop.store(true, Ordering::Release);
    let mut request = request();
    request.allocator_stats = true;
    let mut allocator_calls = 0;
    let result = observe(
        adapter.clone(),
        request,
        Duration::from_millis(10),
        Duration::from_millis(2),
        &mut || json!({"rss_kib":null}),
        &mut || {
            assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
            allocator_calls += 1;
            allocator_report("2.42", accounting())
        },
        &mut |_| panic!("unjoined phase cannot publish success"),
    );
    assert_eq!(result, Err("worker_join_timeout"));
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
    assert_eq!(allocator_calls, 1);
    // Test-only release of the deliberately noncooperative mock. The real
    // executable instead exits failed; its independent outer unit is bounded.
    adapter.ignore_stop.store(false, Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(1);
    while adapter.active.load(Ordering::SeqCst) != 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
}

#[test]
fn stdout_failure_stops_after_joined_phase_without_allocating_next_watch() {
    let adapter = Arc::new(Fake::default());
    let result = observe(
        adapter.clone(),
        request(),
        Duration::from_millis(10),
        Duration::from_secs(1),
        &mut || json!({}),
        &mut || panic!("disabled allocator callback must not run"),
        &mut |_| Err("output_unavailable"),
    );
    assert_eq!(result, Err("output_unavailable"));
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
    assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
}
