//! Isolated diagnostic consumer. Backend library bytes are pinned separately.
//! No renderer, compositor, session changes, allocator tuning or FPS claim.
use lucent_domain::{
    LiveWallpaperAsset, PlaybackOptions, RetargetablePlaybackOptions, StopSignal,
    TargetedMediaFrame, WallpaperFrameRate, WallpaperPixelTarget, WallpaperPlaybackPort,
    WallpaperTargetPort, WallpaperTargetRevision,
};
use lucent_services::video::{LatestWallpaperTargetAdapter, NativeVideoWallpapers};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

mod accounting;
type Check<T> = Result<T, &'static str>;
const READY: Duration = Duration::from_secs(5);
const JOIN: Duration = Duration::from_secs(10);
type Phase = (&'static str, Option<(u32, u32)>);
const KNOWN_PHASES: [Phase; 3] = [
    ("first_fhd", Some((1920, 1080))),
    ("four_k", Some((3840, 2160))),
    ("restored_fhd", Some((1920, 1080))),
];
const SOURCE_PHASES: [Phase; 5] = [
    ("initial_source", None),
    ("first_fhd", Some((1920, 1080))),
    ("four_k", Some((3840, 2160))),
    ("restored_fhd", Some((1920, 1080))),
    ("restored_source", None),
];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Reopen,
    Retarget,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Sequence {
    KnownTargets,
    SourceAndTargets,
}
impl Sequence {
    fn phases(self) -> &'static [Phase] {
        match self {
            Self::KnownTargets => &KNOWN_PHASES,
            Self::SourceAndTargets => &SOURCE_PHASES,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    state: PathBuf,
    asset: LiveWallpaperAsset,
    mode: Mode,
    sequence: Sequence,
    phase_seconds: u64,
    /// Actual ffprobe best-effort PTS rescaled to the backend's Duration clock.
    expected_pts_ns: Vec<u64>,
}
fn input(raw: &[u8]) -> Check<Input> {
    if raw.is_empty() || raw.len() > 64 * 1024 {
        return Err("bounded_input_required");
    }
    let value: Value = serde_json::from_slice(raw).map_err(|_| "invalid_input")?;
    for (nested, keys) in [
        (&value["asset"], &["source", "path", "poster"][..]),
        (&value["asset"]["source"], &["name", "path"][..]),
        (&value["asset"]["poster"], &["name", "path"][..]),
    ] {
        if !nested.as_object().is_some_and(|map| {
            map.len() == keys.len() && keys.iter().all(|key| map.contains_key(*key))
        }) {
            return Err("invalid_input");
        }
    }
    let value: Input = serde_json::from_value(value).map_err(|_| "invalid_input")?;
    let file = Path::new(&value.asset.path);
    let poster = Path::new(&value.asset.poster.path);
    let local = |path: &Path| {
        path.is_absolute()
            && !path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
    };
    if value.phase_seconds != 12
        || !local(&value.state)
        || !local(file)
        || !local(poster)
        || !file.parent().is_some_and(|directory| {
            directory.parent() == Some(value.state.as_path())
                && directory
                    .file_name()
                    .is_some_and(|name| name == "media-generated")
                && poster == directory.join("poster.png")
        })
        || value.expected_pts_ns.len() != 240
        || value.expected_pts_ns.first() != Some(&0)
        || !value
            .expected_pts_ns
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        || value
            .expected_pts_ns
            .last()
            .is_none_or(|last| *last >= 4_000_000_000)
    {
        return Err("invalid_input");
    }
    Ok(value)
}
fn target(value: Option<(u32, u32)>) -> Check<Option<WallpaperPixelTarget>> {
    value
        .map(|(w, h)| WallpaperPixelTarget::new(w, h).map_err(|_| "invalid_target"))
        .transpose()
}
fn ready_elapsed(ready: bool, elapsed: Duration) -> Check<Option<Duration>> {
    if elapsed >= READY {
        return Err("target_not_observed");
    }
    Ok(ready.then_some(elapsed))
}

struct Stop {
    requested: AtomicBool,
    lock: Mutex<()>,
    wake: Condvar,
    deadline: Instant,
}
impl Stop {
    fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            lock: Mutex::new(()),
            wake: Condvar::new(),
            deadline: Instant::now() + Duration::from_secs(100),
        }
    }
    fn cancel(&self) {
        let _guard = self.lock.lock().ok();
        self.requested.store(true, Ordering::Release);
        self.wake.notify_all();
    }
}
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.requested.load(Ordering::Acquire) || Instant::now() >= self.deadline
    }
    fn wait(&self, duration: Duration) {
        if let Ok(guard) = self.lock.lock() {
            let _ = self.wake.wait_timeout_while(
                guard,
                duration.min(self.deadline.saturating_duration_since(Instant::now())),
                |_| !self.cancelled(),
            );
        }
    }
}

#[derive(Default)]
struct Metrics {
    count: u64,
    first_pts_ns: Option<u128>,
    last_pts_ns: Option<u128>,
    first_size: Option<(u32, u32)>,
    last_size: Option<(u32, u32)>,
    rgba_bytes: Option<usize>,
    max_capacity: usize,
}
struct Latest {
    _pixels: Arc<Vec<u8>>,
    capacity: usize,
}
struct State {
    expected: WallpaperTargetRevision,
    size: (u32, u32),
    latest: Option<Latest>,
    metrics: Metrics,
    stale: u64,
    error: Option<&'static str>,
    expected_pts_ns: Arc<Vec<u64>>,
    previous_pts_ns: Option<u128>,
    observed_loop_wraps: u64,
}
impl State {
    fn new(expected_pts_ns: Arc<Vec<u64>>, size: (u32, u32)) -> Self {
        Self {
            expected: WallpaperTargetRevision::initial(),
            size,
            latest: None,
            metrics: Metrics::default(),
            stale: 0,
            error: None,
            expected_pts_ns,
            previous_pts_ns: None,
            observed_loop_wraps: 0,
        }
    }
    fn request(&mut self, revision: WallpaperTargetRevision, size: (u32, u32)) -> Check<()> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if revision <= self.expected {
            return Err("non_advancing_revision");
        }
        self.expected = revision;
        self.size = size;
        self.metrics = Metrics::default();
        // Last-good storage and previous source timestamp survive the demand.
        Ok(())
    }
    fn accept(&mut self, value: TargetedMediaFrame) -> Check<()> {
        if value.revision != self.expected {
            self.stale = self.stale.checked_add(1).ok_or("counter_exhausted")?;
            return Ok(());
        }
        let frame = value.frame;
        let timestamp = frame.timestamp.as_nanos();
        let (width, height, pixels, aspect) = frame.into_shared_rgba_with_aspect();
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4));
        if (width, height) != self.size
            || aspect != (3840, 2160)
            || bytes != Some(pixels.len())
            || !self
                .expected_pts_ns
                .iter()
                .any(|pts| u128::from(*pts).abs_diff(timestamp) <= 1)
        {
            return Err("frame_contract_failed");
        }
        if self.previous_pts_ns.is_some_and(|old| timestamp < old) {
            self.observed_loop_wraps = self
                .observed_loop_wraps
                .checked_add(1)
                .ok_or("counter_exhausted")?;
        }
        self.previous_pts_ns = Some(timestamp);
        let m = &mut self.metrics;
        m.count = m.count.checked_add(1).ok_or("counter_exhausted")?;
        m.first_pts_ns.get_or_insert(timestamp);
        m.last_pts_ns = Some(timestamp);
        m.first_size.get_or_insert((width, height));
        m.last_size = Some((width, height));
        m.rgba_bytes = bytes;
        m.max_capacity = m.max_capacity.max(pixels.capacity());
        self.latest = Some(Latest {
            capacity: pixels.capacity(),
            _pixels: pixels,
        });
        Ok(())
    }
}

struct Worker {
    stop: Arc<Stop>,
    handle: JoinHandle<bool>,
    ordinal: u64,
}
fn spawn(
    adapter: Arc<dyn WallpaperPlaybackPort>,
    asset: LiveWallpaperAsset,
    mode: Mode,
    control: Arc<LatestWallpaperTargetAdapter>,
    state: Arc<Mutex<State>>,
    ordinal: u64,
) -> Check<Worker> {
    let fixed = control
        .try_latest()
        .map_err(|_| "target_control_failed")?
        .ok_or("initial_target_unavailable")?;
    let stop = Arc::new(Stop::new());
    let cancelled = stop.clone();
    let handle = thread::spawn(move || {
        let mut receive = |value: lucent_domain::Result<TargetedMediaFrame>| {
            if cancelled.cancelled() {
                return false;
            }
            let Ok(mut state) = state.lock() else {
                cancelled.cancel();
                return false;
            };
            let result = value
                .map_err(|_| "adapter_error")
                .and_then(|value| state.accept(value));
            if let Err(error) = result {
                state.error = Some(error);
                cancelled.cancel();
                return false;
            }
            true
        };
        match mode {
            Mode::Retarget => adapter.play_retargetable(
                &asset,
                RetargetablePlaybackOptions {
                    fps: WallpaperFrameRate::Source,
                    looping: true,
                },
                control,
                &mut receive,
                cancelled.clone(),
            ),
            Mode::Reopen => adapter.play(
                &asset,
                PlaybackOptions {
                    fps: WallpaperFrameRate::Source,
                    looping: true,
                    target: fixed.target,
                },
                &mut |value| {
                    receive(value.map(|frame| TargetedMediaFrame {
                        revision: fixed.revision,
                        frame,
                    }))
                },
                cancelled.clone(),
            ),
        }
        // Sample before the coordinator can turn an early return into a pass.
        cancelled.cancelled()
    });
    Ok(Worker {
        stop,
        handle,
        ordinal,
    })
}
fn join(worker: Worker) -> Check<Value> {
    worker.stop.cancel();
    let requested = Instant::now();
    while !worker.handle.is_finished() && requested.elapsed() < JOIN {
        thread::sleep(Duration::from_millis(2));
    }
    if !worker.handle.is_finished() {
        return Err("worker_join_timeout");
    }
    let cancelled = worker.handle.join().map_err(|_| "worker_panicked")?;
    if !cancelled {
        return Err("worker_ended_early");
    }
    Ok(
        json!({"worker_ordinal":worker.ordinal,"stop_requested":true,
        "watch_joined":true,"cancellation_observed":true,"join_elapsed_ms":requested.elapsed().as_millis()}),
    )
}
fn memory() -> Value {
    let mut raw = String::new();
    let result = fs::File::open("/proc/self/status")
        .and_then(|file| file.take(65537).read_to_string(&mut raw));
    if result.is_err() || raw.len() > 65536 {
        raw.clear();
    }
    let mut values = serde_json::Map::new();
    for (field, label) in [
        ("VmRSS:", "rss_kib"),
        ("RssAnon:", "anonymous_kib"),
        ("VmHWM:", "rss_high_water_kib"),
        ("VmSwap:", "swap_kib"),
    ] {
        let rows: Vec<_> = raw
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                (parts.next() == Some(field)).then(|| {
                    let count = parts.next()?.parse::<u64>().ok()?;
                    (parts.next() == Some("kB") && parts.next().is_none()).then_some(count)
                })
            })
            .collect();
        values.insert(
            label.into(),
            json!(if rows.len() == 1 { rows[0] } else { None }),
        );
    }
    Value::Object(values)
}
fn point(label: &str, started: Instant, stopped: bool) -> Value {
    let mut point =
        json!({"moment":label,"elapsed_ms":started.elapsed().as_millis(),"memory":memory()});
    if stopped {
        point["allocator"] = accounting::allocator();
    }
    point
}

fn observe(adapter: Arc<dyn WallpaperPlaybackPort>, request: Input) -> Check<Value> {
    let started = Instant::now();
    let mut stopped = vec![point("before_play", started, true)];
    let planned = request.sequence.phases();
    let initial = planned[0].1;
    let control = Arc::new(LatestWallpaperTargetAdapter::new(target(initial)?));
    let state = Arc::new(Mutex::new(State::new(
        Arc::new(request.expected_pts_ns),
        initial.unwrap_or((3840, 2160)),
    )));
    let mut owner: Option<Worker> = None;
    let mut starts = 0u64;
    let mut joins = Vec::new();
    // All errors pass through exact-current-worker cancellation and join.
    let result: Check<Vec<Value>> = (|| {
        let mut phases = Vec::new();
        for (index, &(label, demand)) in planned.iter().enumerate() {
            let before = point("before_target", started, false);
            if request.mode == Mode::Reopen
                && let Some(worker) = owner.take()
            {
                joins.push(join(worker)?);
                // Both consumers retain last-good pixels until replacement.
                // Reset only the control's per-worker timestamp observation;
                // its new decoder starts a new source clock, not an EOF wrap.
                state
                    .lock()
                    .map_err(|_| "state_unavailable")?
                    .previous_pts_ns = None;
            }
            let requested = Instant::now();
            let request_clock_ms = started.elapsed().as_millis();
            if index != 0 {
                let mut state = state.lock().map_err(|_| "state_unavailable")?;
                let next = control
                    .set(state.expected, target(demand)?)
                    .map_err(|_| "target_control_failed")?;
                state.request(next.revision, demand.unwrap_or((3840, 2160)))?;
            }
            if owner.is_none() {
                starts += 1;
                owner = Some(spawn(
                    adapter.clone(),
                    request.asset.clone(),
                    request.mode,
                    control.clone(),
                    state.clone(),
                    starts,
                )?);
            }
            let worker = owner.as_ref().ok_or("worker_owner_missing")?;
            let elapsed = loop {
                let ready = {
                    let state = state.lock().map_err(|_| "state_unavailable")?;
                    if let Some(error) = state.error {
                        return Err(error);
                    }
                    ready_elapsed(state.metrics.count != 0, requested.elapsed())?
                };
                if let Some(elapsed) = ready {
                    break elapsed;
                }
                if worker.handle.is_finished() || worker.stop.cancelled() {
                    return Err("worker_ended_early");
                }
                worker.stop.wait(Duration::from_millis(10));
            };
            let observed = point("target_observed", started, false);
            let phase_started = Instant::now();
            let mut samples = vec![before, observed];
            while phase_started.elapsed() < Duration::from_secs(request.phase_seconds) {
                if worker.handle.is_finished() || worker.stop.cancelled() {
                    return Err("worker_ended_early");
                }
                worker.stop.wait(Duration::from_millis(250));
                samples.push(point("during_target", started, false));
            }
            let state = state.lock().map_err(|_| "state_unavailable")?;
            if let Some(error) = state.error {
                return Err(error);
            }
            let m = &state.metrics;
            phases.push(json!({"phase":label,"revision":state.expected.get(),"requested_size":demand,
                "worker_ordinal":worker.ordinal,"target_ready_elapsed_ms":elapsed.as_millis(),
                "phase_observed_ms":phase_started.elapsed().as_millis(),"phase_request_clock_ms":request_clock_ms,
                "publications":m.count,"first_size":m.first_size,"last_size":m.last_size,
                "rgba_bytes":m.rgba_bytes,"aspect_size":[3840,2160],"maximum_capacity_bytes":m.max_capacity,
                "latest_capacity_bytes":state.latest.as_ref().map(|latest| latest.capacity),
                "first_pts_ns":m.first_pts_ns,"last_pts_ns":m.last_pts_ns,
                "observed_loop_wraps_total":state.observed_loop_wraps,"stale_publications_total":state.stale,
                "all_admitted_pts_match_ffprobe":true,"memory_samples":samples}));
        }
        Ok(phases)
    })();
    if let Some(worker) = owner.take() {
        joins.push(join(worker)?);
    }
    let phases = result?;
    let mut state = state.lock().map_err(|_| "state_unavailable")?;
    if let Some(error) = state.error {
        return Err(error);
    }
    stopped.push(point("after_join_latest_held", started, true));
    let held = state.latest.as_ref().map(|latest| latest.capacity);
    state.latest = None;
    stopped.push(point("after_latest_dropped", started, true));
    let expected = if request.mode == Mode::Retarget {
        1
    } else {
        planned.len() as u64
    };
    if starts != expected || joins.len() != expected as usize || state.observed_loop_wraps == 0 {
        return Err("lifecycle_or_loop_observation_failed");
    }
    Ok(
        json!({"kind":"full_demand_observation","mode":request.mode,"sequence":request.sequence,
        "pid":std::process::id(),"phases":phases,"play_calls":starts,"exact_worker_joins":joins,
        "stopped_allocator_observations":stopped,"final_latest_capacity_before_drop":held,
        "consumer_held_pixel_capacity_after_drop":0,"renderer_used":false,"fps_measured":false,
        "whole_shell_memory_accepted":false,"native_presentation_accepted":false}),
    )
}
fn run() -> Check<()> {
    match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => {
            println!("{}", json!({"prepared_only":true,"actions":false}));
            return Ok(());
        }
        [arg] if arg == "--run" => (),
        _ => return Err("explicit_run_required"),
    }
    let mut raw = Vec::new();
    io::stdin()
        .take(65537)
        .read_to_end(&mut raw)
        .map_err(|_| "input_unavailable")?;
    let request = input(&raw)?;
    for path in [
        request.state.as_path(),
        Path::new(&request.asset.path),
        Path::new(&request.asset.poster.path),
    ] {
        if fs::canonicalize(path).ok().as_deref() != Some(path) {
            return Err("canonical_input_required");
        }
    }
    let adapter = Arc::new(NativeVideoWallpapers {
        state: request.state.clone(),
        library: request.state.clone(),
    });
    let value = observe(adapter, request)?;
    writeln!(io::stdout().lock(), "{value}").map_err(|_| "output_unavailable")
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(reason) => {
            println!(
                "{}",
                json!({"kind":"failed","reason":reason,"next_case_allowed":false})
            );
            std::process::ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod observer_tests;
