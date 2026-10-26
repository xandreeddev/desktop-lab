//! Independent one-play observation; the desktop never selects this path.
use super::*;
use lucent_domain::{
    RetargetablePlaybackOptions, TargetedMediaFrame, WallpaperTargetPort, WallpaperTargetRevision,
};
use lucent_services::video::LatestWallpaperTargetAdapter;

const TARGET_READY_TIMEOUT: Duration = Duration::from_secs(5);

fn admitted_ready_elapsed(ready: bool, elapsed: Duration) -> Check<Option<Duration>> {
    if elapsed >= TARGET_READY_TIMEOUT {
        return Err("target_not_observed");
    }
    Ok(ready.then_some(elapsed))
}

struct State {
    expected: WallpaperTargetRevision,
    publications: Publications,
    stale: u64,
}
impl State {
    fn accept(&mut self, value: TargetedMediaFrame, stream: Option<&ImageStream>) -> Check<()> {
        if value.revision != self.expected {
            self.stale = self.stale.checked_add(1).ok_or("counter_exhausted")?;
            return Ok(());
        }
        self.publications.accept(value.frame, stream)
    }
    fn request(
        &mut self,
        control: &LatestWallpaperTargetAdapter,
        target: WallpaperPixelTarget,
    ) -> Check<()> {
        if let Some(error) = self.publications.error {
            return Err(error);
        }
        let next = control
            .set(self.expected, target)
            .map_err(|_| "target_control_failed")?;
        if next.revision != self.expected {
            // Only metrics reset. Keep the old latest allocation until its
            // valid replacement arrives, as an ordinary latest-only consumer.
            let latest = self.publications.latest.take();
            self.publications = Publications {
                latest,
                ..Publications::default()
            };
            self.expected = next.revision;
        }
        Ok(())
    }
}

fn point(samples: &mut Samples<'_>, label: &'static str, elapsed: Duration) -> Value {
    // Per-target points are memory-only even when allocator_stats is enabled.
    json!({"moment":label,"elapsed_ms":elapsed.as_millis(),"memory":(samples.memory)()})
}

pub(super) fn observe(
    adapter: Arc<dyn WallpaperPlaybackPort>,
    request: Input,
    duration: Duration,
    join_timeout: Duration,
    memory: &mut dyn FnMut() -> Value,
    allocator_query: &mut dyn FnMut() -> Value,
    emit: &mut dyn FnMut(Value) -> Check<()>,
) -> Check<()> {
    let mut samples = Samples {
        memory,
        allocator: request.allocator_stats.then_some(allocator_query),
    };
    let total = duration
        .checked_mul(3)
        .and_then(|value| value.checked_add(TARGET_READY_TIMEOUT * 3))
        .ok_or("observation_deadline_overflow")?;
    let started = Instant::now();
    let mut whole = vec![samples.capture(Moment::BeforePlay, 0)];
    let target =
        WallpaperPixelTarget::new(PHASES[0].1, PHASES[0].2).map_err(|_| "invalid_target")?;
    let control = Arc::new(LatestWallpaperTargetAdapter::new(target));
    let initial = control
        .try_latest()
        .map_err(|_| "target_control_failed")?
        .ok_or("initial_target_unavailable")?;
    let stop = Arc::new(Stop::new(total));
    let state = Arc::new(Mutex::new(State {
        expected: initial.revision,
        publications: Publications::default(),
        stale: 0,
    }));
    let observed = state.clone();
    let cancelled = stop.clone();
    let targets = control.clone();
    let worker = thread::spawn(move || {
        let stream = request.sdk_bridge.then(ImageStream::new);
        adapter.play_retargetable(
            &request.asset,
            RetargetablePlaybackOptions {
                fps: WallpaperFrameRate::Source,
                looping: true,
            },
            targets,
            &mut |value| {
                if cancelled.cancelled() {
                    return false;
                }
                let Ok(mut state) = observed.lock() else {
                    cancelled.cancel();
                    return false;
                };
                let result = value
                    .map_err(|_| "adapter_error")
                    .and_then(|value| state.accept(value, stream.as_ref()));
                if let Err(error) = result {
                    state.publications.error = Some(error);
                    cancelled.cancel();
                    return false;
                }
                true
            },
            cancelled.clone(),
        );
        cancelled.cancelled()
    });
    // No output is emitted while the producer is alive. Every failure first
    // cancels and attempts the exact worker join; no later phase/watch starts.
    let results: Check<Vec<Value>> = (|| {
        let mut results = Vec::new();
        for (index, (label, width, height)) in PHASES.into_iter().enumerate() {
            let mut points = vec![point(&mut samples, "before_target", started.elapsed())];
            let requested = Instant::now();
            if index != 0 {
                state
                    .lock()
                    .map_err(|_| "publication_state_unavailable")?
                    .request(
                        &control,
                        WallpaperPixelTarget::new(width, height).map_err(|_| "invalid_target")?,
                    )?;
            }
            let ready_elapsed = loop {
                let ready = {
                    let state = state.lock().map_err(|_| "publication_state_unavailable")?;
                    if let Some(error) = state.publications.error {
                        return Err(error);
                    }
                    let ready = state.publications.count != 0;
                    // Validate a fresh clock after state/lock validation. A
                    // publication observed after a delayed wake is not ready
                    // within budget, even if it arrived before we got the lock.
                    admitted_ready_elapsed(ready, requested.elapsed())?
                };
                if let Some(elapsed) = ready {
                    break elapsed;
                }
                if worker.is_finished() {
                    return Err("worker_ended_before_finite_observation");
                }
                if stop.cancelled() {
                    return Err("retarget_sequence_cancelled");
                }
                stop.wait(
                    SAMPLE_INTERVAL.min(TARGET_READY_TIMEOUT.saturating_sub(requested.elapsed())),
                );
            };
            let ready_ms = ready_elapsed.as_millis();
            points.push(point(&mut samples, "target_observed", started.elapsed()));
            stop.wait(duration);
            if worker.is_finished() {
                return Err("worker_ended_before_finite_observation");
            }
            if stop.cancelled() {
                return Err("retarget_sequence_cancelled");
            }
            points.push(point(
                &mut samples,
                "end_target_observation",
                started.elapsed(),
            ));
            let state = state.lock().map_err(|_| "publication_state_unavailable")?;
            let p = &state.publications;
            if let Some(error) = p.error {
                return Err(error);
            }
            results.push(json!({"kind":"retarget_phase","phase":label,
                "revision":state.expected.get(),"requested_size":[width,height],
                "target_ready_elapsed_ms":ready_ms,"elapsed_ms":requested.elapsed().as_millis(),
                "publications":p.count,"first_size":p.first_size,"last_size":p.last_size,
                "geometry_changes":p.geometry_changes,"maximum_published_capacity_bytes":p.max_capacity,
                "latest_capacity_bytes":p.latest.as_ref().map(|value|value.capacity),
                "memory_samples":points}));
        }
        Ok(results)
    })();
    stop.cancel();
    let join_deadline = Instant::now() + join_timeout;
    while !worker.is_finished() && Instant::now() < join_deadline {
        thread::sleep(Duration::from_millis(2));
    }
    if !worker.is_finished() {
        return Err("worker_join_timeout");
    }
    let cancellation_observed = worker.join().map_err(|_| "worker_panicked")?;
    let results = results?;
    if !cancellation_observed {
        return Err("worker_ended_before_finite_observation");
    }
    let mut state = state.lock().map_err(|_| "publication_state_unavailable")?;
    if let Some(error) = state.publications.error {
        return Err(error);
    }
    whole.push(samples.capture(Moment::JoinedLatestHeld, started.elapsed().as_millis()));
    let last_size = state.publications.latest.as_ref().map(Latest::dimensions);
    let last_capacity = state
        .publications
        .latest
        .as_ref()
        .map(|value| value.capacity);
    state.publications.latest = None;
    whole.push(samples.capture(Moment::LatestDropped, started.elapsed().as_millis()));
    for result in results {
        emit(result)?;
    }
    emit(
        json!({"kind":"retarget_finished","one_play_call":true,"same_process_sequence":true,
        "watch_joined":true,"cancellation_observed":true,"stop_requested":true,
        "consumer_held_pixel_capacity_after_drop":0,"last_size":last_size,
        "latest_capacity_bytes_before_drop":last_capacity,"stale_frames_rejected":state.stale,
        "memory_samples":whole,"renderer_used":false,"performance_accepted":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_domain::{DomainError, Result, Wallpaper};
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn readiness_admission_rejects_late_publications_and_keeps_the_validated_clock() {
        let before = TARGET_READY_TIMEOUT - Duration::from_nanos(1);
        assert_eq!(admitted_ready_elapsed(true, before), Ok(Some(before)));
        assert_eq!(admitted_ready_elapsed(false, before), Ok(None));
        assert_eq!(
            admitted_ready_elapsed(true, Duration::ZERO),
            Ok(Some(Duration::ZERO))
        );
        for elapsed in [
            TARGET_READY_TIMEOUT,
            TARGET_READY_TIMEOUT + Duration::from_nanos(1),
        ] {
            for ready in [false, true] {
                assert_eq!(
                    admitted_ready_elapsed(ready, elapsed),
                    Err("target_not_observed")
                );
            }
        }
    }

    fn request() -> Input {
        input(
            &serde_json::to_vec(&json!({"state":"/private/state","phase_seconds":1,
            "allocator_stats":true,"retarget":true,
            "asset":{"source":{"name":"Private","path":"/unavailable/original.mp4"},
                     "path":"/private/state/media-owned/source.mp4",
                     "poster":{"name":"Private","path":"/private/state/media-owned/poster.png"}}}))
            .unwrap(),
        )
        .unwrap()
    }
    fn frame(revision: WallpaperTargetRevision, pixels: Arc<Vec<u8>>) -> TargetedMediaFrame {
        TargetedMediaFrame {
            revision,
            frame: MediaFrame::rgba_shared_with_aspect(1, 1, pixels, (1, 1), Duration::ZERO)
                .unwrap(),
        }
    }
    #[derive(Default)]
    struct Fake {
        active: AtomicUsize,
        calls: AtomicUsize,
        seen: Mutex<Vec<(WallpaperTargetRevision, (u32, u32))>>,
        ignore_stop: AtomicBool,
        error: bool,
        early: bool,
    }
    impl WallpaperPlaybackPort for Fake {
        fn prepare(&self, _: &Wallpaper) -> Result<LiveWallpaperAsset> {
            panic!("never import")
        }
        fn play(
            &self,
            _: &LiveWallpaperAsset,
            _: PlaybackOptions,
            _: &mut dyn FnMut(Result<MediaFrame>) -> bool,
            _: Arc<dyn StopSignal>,
        ) {
            panic!("retargeting must never fall back to static play/restart")
        }
        fn play_retargetable(
            &self,
            _: &LiveWallpaperAsset,
            options: RetargetablePlaybackOptions,
            targets: Arc<dyn WallpaperTargetPort>,
            emit: &mut dyn FnMut(Result<TargetedMediaFrame>) -> bool,
            stop: Arc<dyn StopSignal>,
        ) {
            assert_eq!(self.active.fetch_add(1, Ordering::SeqCst), 0);
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(options.fps, WallpaperFrameRate::Source);
            assert!(options.looping);
            if self.error {
                emit(Err(DomainError::Failed("private backend error".into())));
            } else {
                while !stop.cancelled() || self.ignore_stop.load(Ordering::Acquire) {
                    if let Some(demand) = targets.try_latest().unwrap() {
                        let mut seen = self.seen.lock().unwrap();
                        if seen.last().is_none_or(|last| last.0 != demand.revision) {
                            seen.push((demand.revision, demand.target.size()));
                        }
                        drop(seen);
                        if !emit(Ok(frame(demand.revision, Arc::new(vec![10, 20, 30, 255]))))
                            && !self.ignore_stop.load(Ordering::Acquire)
                        {
                            break;
                        }
                        if self.early {
                            break;
                        }
                    }
                    if self.ignore_stop.load(Ordering::Acquire) {
                        thread::sleep(Duration::from_millis(1));
                    } else {
                        stop.wait(Duration::from_millis(1));
                    }
                }
            }
            self.active.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn one_play_owns_all_targets_and_allocator_queries_only_run_outside_that_worker() {
        for sdk in [false, true] {
            let adapter = Arc::new(Fake::default());
            let mut input = request();
            input.sdk_bridge = sdk;
            let mut queries = 0;
            let mut memory = 0;
            let mut rows = Vec::new();
            crate::observe(
                adapter.clone(),
                input,
                Duration::from_millis(10),
                Duration::from_secs(1),
                &mut || {
                    memory += 1;
                    json!({"rss_kib":4})
                },
                &mut || {
                    assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
                    queries += 1;
                    json!({"available":false})
                },
                &mut |value| {
                    assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
                    rows.push(value);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
            assert_eq!(queries, 3);
            assert_eq!(memory, 12);
            assert_eq!(
                *adapter.seen.lock().unwrap(),
                vec![
                    (WallpaperTargetRevision::new(1).unwrap(), (1920, 1080)),
                    (WallpaperTargetRevision::new(2).unwrap(), (3840, 2160)),
                    (WallpaperTargetRevision::new(3).unwrap(), (1920, 1080))
                ]
            );
            assert_eq!(rows.len(), 4);
            for row in rows.iter().take(3) {
                assert_eq!(row["kind"], "retarget_phase");
                assert!(row["publications"].as_u64().unwrap() > 0);
                assert!(
                    row["memory_samples"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|sample| sample.get("allocator").is_none())
                );
            }
            assert_eq!(rows[3]["kind"], "retarget_finished");
            assert_eq!(rows[3]["one_play_call"], true);
            assert_eq!(rows[3]["consumer_held_pixel_capacity_after_drop"], 0);
            assert_eq!(rows[3]["performance_accepted"], false);
        }
    }

    #[test]
    fn stale_results_never_relabel_pixels_and_old_latest_retires_only_on_valid_replacement() {
        let control =
            LatestWallpaperTargetAdapter::new(WallpaperPixelTarget::new(1920, 1080).unwrap());
        let first = control.try_latest().unwrap().unwrap();
        let mut state = State {
            expected: first.revision,
            publications: Publications::default(),
            stale: 0,
        };
        let old = Arc::new(vec![1, 2, 3, 255]);
        let old_weak = Arc::downgrade(&old);
        state.accept(frame(first.revision, old), None).unwrap();
        state
            .request(&control, WallpaperPixelTarget::new(3840, 2160).unwrap())
            .unwrap();
        assert_eq!(old_weak.strong_count(), 1);
        assert_eq!(state.publications.count, 0);
        let stale = Arc::new(vec![4, 5, 6, 255]);
        let stale_weak = Arc::downgrade(&stale);
        state.accept(frame(first.revision, stale), None).unwrap();
        assert_eq!(state.stale, 1);
        assert!(stale_weak.upgrade().is_none());
        assert_eq!(old_weak.strong_count(), 1);
        state
            .accept(frame(state.expected, Arc::new(vec![7, 8, 9, 255])), None)
            .unwrap();
        assert!(old_weak.upgrade().is_none());
        assert_eq!(state.publications.count, 1);
    }

    #[test]
    fn backend_error_or_early_return_never_advances_to_the_next_target() {
        for error in [true, false] {
            let adapter = Arc::new(Fake {
                error,
                early: !error,
                ..Fake::default()
            });
            let mut calls = 0;
            let result = crate::observe(
                adapter.clone(),
                request(),
                Duration::from_millis(10),
                Duration::from_secs(1),
                &mut || json!({}),
                &mut || {
                    calls += 1;
                    json!({})
                },
                &mut |_| panic!("failed sequence cannot emit success"),
            );
            assert!(result.is_err());
            assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
            assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
            assert_eq!(calls, 1);
            assert!(adapter.seen.lock().unwrap().len() <= 1);
        }
    }

    #[test]
    fn a_stop_ignoring_worker_fails_join_bound_without_another_play_or_postjoin_stats() {
        let adapter = Arc::new(Fake::default());
        adapter.ignore_stop.store(true, Ordering::Release);
        let mut calls = 0;
        let result = crate::observe(
            adapter.clone(),
            request(),
            Duration::from_millis(10),
            Duration::from_millis(2),
            &mut || json!({}),
            &mut || {
                calls += 1;
                json!({})
            },
            &mut |_| panic!("unjoined worker cannot emit success"),
        );
        assert_eq!(result, Err("worker_join_timeout"));
        assert_eq!(calls, 1);
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
        adapter.ignore_stop.store(false, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(1);
        while adapter.active.load(Ordering::SeqCst) != 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(adapter.active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unsupported_default_is_explicit_and_never_calls_static_play() {
        struct StaticOnly(AtomicUsize);
        impl WallpaperPlaybackPort for StaticOnly {
            fn prepare(&self, _: &Wallpaper) -> Result<LiveWallpaperAsset> {
                panic!("no import")
            }
            fn play(
                &self,
                _: &LiveWallpaperAsset,
                _: PlaybackOptions,
                _: &mut dyn FnMut(Result<MediaFrame>) -> bool,
                _: Arc<dyn StopSignal>,
            ) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let adapter = Arc::new(StaticOnly(AtomicUsize::new(0)));
        assert_eq!(
            crate::observe(
                adapter.clone(),
                request(),
                Duration::from_millis(10),
                Duration::from_secs(1),
                &mut || json!({}),
                &mut || json!({}),
                &mut |_| panic!("unsupported")
            ),
            Err("adapter_error")
        );
        assert_eq!(adapter.0.load(Ordering::SeqCst), 0);
    }
}
