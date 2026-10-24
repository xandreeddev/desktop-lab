//! Independent, latest-only WallpaperPlaybackPort consumer; no rendering or UI.
use lucent_api::{ImageData, ImageStream};
use lucent_domain::{
    LiveWallpaperAsset, MediaFrame, PlaybackOptions, StopSignal, WallpaperFrameRate,
    WallpaperPixelTarget, WallpaperPlaybackPort,
};
use lucent_services::video::NativeVideoWallpapers;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const INPUT_BYTES: usize = 64 * 1024;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);
const JOIN_SECONDS: u64 = 10;
const PHASES: [(&str, u32, u32); 3] = [
    ("first_fhd", 1920, 1080),
    ("four_k", 3840, 2160),
    ("restored_fhd", 1920, 1080),
];
type Check<T> = std::result::Result<T, &'static str>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    state: PathBuf,
    asset: LiveWallpaperAsset,
    phase_seconds: u64,
    #[serde(default)]
    sdk_bridge: bool,
    #[serde(default)]
    allocator_stats: bool,
}

fn local(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
}

fn input(raw: &[u8]) -> Check<Input> {
    if raw.is_empty() || raw.len() > INPUT_BYTES {
        return Err("bounded_input_required");
    }
    let value: Value = serde_json::from_slice(raw).map_err(|_| "invalid_input")?;
    // The existing domain descriptor stays unchanged; reject ignored nested
    // fields here rather than silently extending this diagnostic's input.
    let exact = |value: &Value, keys: &[&str]| {
        value.as_object().is_some_and(|map| {
            map.len() == keys.len() && keys.iter().all(|key| map.contains_key(*key))
        })
    };
    if !exact(&value["asset"], &["source", "path", "poster"])
        || !exact(&value["asset"]["source"], &["name", "path"])
        || !exact(&value["asset"]["poster"], &["name", "path"])
    {
        return Err("invalid_input");
    }
    let value: Input = serde_json::from_value(value).map_err(|_| "invalid_input")?;
    let file = Path::new(&value.asset.path);
    let poster = Path::new(&value.asset.poster.path);
    let owned = file.parent().is_some_and(|directory| {
        directory.parent() == Some(value.state.as_path())
            && directory
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("media-"))
            && poster == directory.join("poster.png")
    });
    if !(1..=30).contains(&value.phase_seconds)
        || !local(&value.state)
        || !local(file)
        || !local(poster)
        || !owned
    {
        return Err("invalid_input");
    }
    Ok(value)
}

struct Stop {
    deadline: Instant,
    requested: AtomicBool,
    lock: Mutex<()>,
    wake: Condvar,
}
impl Stop {
    fn new(duration: Duration) -> Self {
        Self {
            deadline: Instant::now() + duration,
            requested: AtomicBool::new(false),
            lock: Mutex::new(()),
            wake: Condvar::new(),
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
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            let _ = self
                .wake
                .wait_timeout_while(guard, duration.min(remaining), |_| !self.cancelled());
        }
    }
}

enum Pixels {
    Domain(MediaFrame),
    Sdk(ImageData),
}
struct Latest {
    pixels: Pixels,
    capacity: usize,
}
impl Latest {
    fn new(frame: MediaFrame, stream: Option<&ImageStream>) -> Check<Self> {
        let timestamp = frame.timestamp;
        let (width, height, pixels, aspect) = frame.into_shared_rgba_with_aspect();
        match stream {
            Some(stream) => {
                let image = stream
                    .frame_rgba_shared_with_aspect(width, height, pixels, aspect)
                    .map_err(|_| "image_bridge_failed")?;
                let capacity = image.levels()[0].pixels.capacity();
                Ok(Self {
                    pixels: Pixels::Sdk(image),
                    capacity,
                })
            }
            None => {
                let capacity = pixels.capacity();
                let frame =
                    MediaFrame::rgba_shared_with_aspect(width, height, pixels, aspect, timestamp)
                        .map_err(|_| "frame_contract_failed")?;
                Ok(Self {
                    pixels: Pixels::Domain(frame),
                    capacity,
                })
            }
        }
    }
    fn dimensions(&self) -> (u32, u32) {
        match &self.pixels {
            Pixels::Domain(frame) => frame.dimensions(),
            Pixels::Sdk(image) => (image.levels()[0].width, image.levels()[0].height),
        }
    }
}

#[derive(Default)]
struct Publications {
    latest: Option<Latest>,
    count: u64,
    geometry_changes: u64,
    first_size: Option<(u32, u32)>,
    last_size: Option<(u32, u32)>,
    max_capacity: usize,
    error: Option<&'static str>,
}
impl Publications {
    fn accept(&mut self, frame: MediaFrame, stream: Option<&ImageStream>) -> Check<()> {
        let latest = Latest::new(frame, stream)?;
        let size = latest.dimensions();
        self.count = self.count.checked_add(1).ok_or("counter_exhausted")?;
        if self.last_size.is_some_and(|old| old != size) {
            self.geometry_changes = self
                .geometry_changes
                .checked_add(1)
                .ok_or("counter_exhausted")?;
        }
        self.first_size.get_or_insert(size);
        self.last_size = Some(size);
        self.max_capacity = self.max_capacity.max(latest.capacity);
        self.latest = Some(latest);
        Ok(())
    }
}

fn memory_status(raw: &str) -> Value {
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
        let value = if rows.len() == 1 { rows[0] } else { None };
        values.insert(label.into(), json!(value));
    }
    Value::Object(values)
}

fn memory() -> Value {
    // Own process only, fixed allowlisted fields. No raw status contents,
    // command, environment, native object names or media paths leave here.
    let mut raw = String::new();
    let result = fs::File::open("/proc/self/status")
        .and_then(|file| file.take(65537).read_to_string(&mut raw));
    memory_status(if result.is_ok() && raw.len() <= 65536 {
        &raw
    } else {
        ""
    })
}

// glibc's arena accounting includes all arenas, but excludes direct-mapped
// malloc chunks. Thread-cache frees can still count as arena in-use bytes.
// None of these counters describes Rust object ownership or resident pages.
#[cfg(any(test, all(target_os = "linux", target_env = "gnu")))]
struct MallocAccounting {
    arena: u64,
    in_use: u64,
    free: u64,
    direct_mmap: u64,
    direct_mmap_regions: u64,
    free_chunks: u64,
}

fn allocator_unavailable(reason: &'static str) -> Value {
    json!({"kind":"glibc_mallinfo2", "available":false, "reason":reason})
}

#[cfg(any(test, all(target_os = "linux", target_env = "gnu")))]
fn allocator_report(version: &str, stats: MallocAccounting) -> Value {
    let parts: Vec<_> = version.split('.').collect();
    if version.len() > 32
        || parts.len() < 2
        || !parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return allocator_unavailable("invalid_runtime_version");
    }
    let major = parts[0].parse::<u32>().ok();
    let minor = parts[1].parse::<u32>().ok();
    if !matches!((major, minor), (Some(major), Some(minor)) if (major, minor) >= (2, 33)) {
        return allocator_unavailable("unsupported_glibc_version");
    }
    json!({"kind":"glibc_mallinfo2", "available":true, "glibc_version":version,
        "bytes":{"arena":stats.arena, "arena_accounted_in_use":stats.in_use,
                 "arena_free":stats.free, "direct_mmap":stats.direct_mmap,
                 "malloc_accounted_in_use_total":stats.in_use.checked_add(stats.direct_mmap)},
        "counts":{"arena_free_chunks":stats.free_chunks,
                  "direct_mmap_regions":stats.direct_mmap_regions}})
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn allocator() -> Value {
    // GNU guarantees a static NUL-terminated version string. This optional
    // diagnostic targets glibc >= 2.33 (mallinfo2's symbol version). It neither
    // tunes nor trims malloc, and runs only with the playback worker stopped.
    let version = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };
    let Ok(version) = version.to_str() else {
        return allocator_unavailable("invalid_runtime_version");
    };
    // SAFETY: mallinfo2 returns an owned scalar snapshot without arguments.
    // The example has already initialized malloc, never calls mallopt, and
    // invokes this outside signal handlers and playback-worker lifetimes.
    let stats = unsafe { libc::mallinfo2() };
    let converted = (
        u64::try_from(stats.arena),
        u64::try_from(stats.uordblks),
        u64::try_from(stats.fordblks),
        u64::try_from(stats.hblkhd),
        u64::try_from(stats.hblks),
        u64::try_from(stats.ordblks),
    );
    match converted {
        (
            Ok(arena),
            Ok(in_use),
            Ok(free),
            Ok(direct_mmap),
            Ok(direct_mmap_regions),
            Ok(free_chunks),
        ) => allocator_report(
            version,
            MallocAccounting {
                arena,
                in_use,
                free,
                direct_mmap,
                direct_mmap_regions,
                free_chunks,
            },
        ),
        _ => allocator_unavailable("counter_out_of_range"),
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn allocator() -> Value {
    allocator_unavailable("unsupported_target")
}

#[derive(Clone, Copy)]
enum Moment {
    BeforePlay,
    DuringPlay,
    JoinedLatestHeld,
    LatestDropped,
}
impl Moment {
    fn label(self) -> &'static str {
        match self {
            Self::BeforePlay => "before_play",
            Self::DuringPlay => "during_play",
            Self::JoinedLatestHeld => "after_join_latest_held",
            Self::LatestDropped => "after_latest_dropped",
        }
    }
}

struct Samples<'a> {
    memory: &'a mut dyn FnMut() -> Value,
    allocator: Option<&'a mut dyn FnMut() -> Value>,
}
impl Samples<'_> {
    fn capture(&mut self, moment: Moment, elapsed_ms: u128) -> Value {
        let mut value = json!({"moment":moment.label(), "elapsed_ms":elapsed_ms,
                               "memory":(self.memory)()});
        if !matches!(moment, Moment::DuringPlay)
            && let Some(query) = self.allocator.as_mut()
        {
            value["allocator"] = query();
        }
        value
    }
}

fn phase(
    adapter: Arc<dyn WallpaperPlaybackPort>,
    asset: LiveWallpaperAsset,
    options: PlaybackOptions,
    sdk_bridge: bool,
    duration: Duration,
    join_timeout: Duration,
    samples: &mut Samples<'_>,
) -> Check<Value> {
    let start = Instant::now();
    let mut observations = vec![samples.capture(Moment::BeforePlay, 0)];
    let stop = Arc::new(Stop::new(duration));
    let publications = Arc::new(Mutex::new(Publications::default()));
    let state = publications.clone();
    let cancelled = stop.clone();
    let worker = thread::spawn(move || {
        let stream = sdk_bridge.then(ImageStream::new);
        adapter.play(
            &asset,
            options,
            &mut |frame| {
                if cancelled.cancelled() {
                    return false;
                }
                let Ok(mut state) = state.lock() else {
                    cancelled.cancel();
                    return false;
                };
                let result = frame
                    .map_err(|_| "adapter_error")
                    .and_then(|frame| state.accept(frame, stream.as_ref()));
                if let Err(reason) = result {
                    state.error = Some(reason);
                    cancelled.cancel();
                    return false;
                }
                true
            },
            cancelled.clone(),
        );
        // Read before the coordinator requests cancellation, so an early
        // natural return cannot become a successful deadline observation.
        cancelled.cancelled()
    });
    while !stop.cancelled() && !worker.is_finished() {
        stop.wait(SAMPLE_INTERVAL);
        observations.push(samples.capture(Moment::DuringPlay, start.elapsed().as_millis()));
    }
    stop.cancel();
    let join_deadline = Instant::now() + join_timeout;
    while !worker.is_finished() && Instant::now() < join_deadline {
        thread::sleep(Duration::from_millis(2));
    }
    if !worker.is_finished() {
        // Dropping the handle cannot authorize another phase. The caller must
        // terminate this standalone process; a later outer unit bounds it too.
        return Err("worker_join_timeout");
    }
    let cancellation_observed = worker.join().map_err(|_| "worker_panicked")?;
    let mut state = publications
        .lock()
        .map_err(|_| "publication_state_unavailable")?;
    if let Some(reason) = state.error {
        return Err(reason);
    }
    if !cancellation_observed || state.count == 0 {
        return Err("worker_ended_before_finite_observation");
    }
    observations.push(samples.capture(Moment::JoinedLatestHeld, start.elapsed().as_millis()));
    let latest_capacity = state.latest.as_ref().map(|latest| latest.capacity);
    let last_size = state.latest.as_ref().map(Latest::dimensions);
    state.latest = None;
    observations.push(samples.capture(Moment::LatestDropped, start.elapsed().as_millis()));
    Ok(
        json!({"publications":state.count, "first_size":state.first_size, "last_size":last_size,
              "geometry_changes":state.geometry_changes,
              "maximum_published_capacity_bytes":state.max_capacity,
              "latest_capacity_bytes_before_drop":latest_capacity,
              "consumer_held_pixel_capacity_after_drop":0,
              "stop_requested":true, "watch_joined":true, "cancellation_observed":true,
              "elapsed_ms":start.elapsed().as_millis(), "memory_samples":observations}),
    )
}

fn observe(
    adapter: Arc<dyn WallpaperPlaybackPort>,
    request: Input,
    duration: Duration,
    join_timeout: Duration,
    sample: &mut dyn FnMut() -> Value,
    allocator_query: &mut dyn FnMut() -> Value,
    emit: &mut dyn FnMut(Value) -> Check<()>,
) -> Check<()> {
    let mut samples = Samples {
        memory: sample,
        allocator: request.allocator_stats.then_some(allocator_query),
    };
    for (label, width, height) in PHASES {
        let result = phase(
            adapter.clone(),
            request.asset.clone(),
            PlaybackOptions {
                fps: WallpaperFrameRate::Source,
                looping: true,
                target: Some(
                    WallpaperPixelTarget::new(width, height).map_err(|_| "invalid_target")?,
                ),
            },
            request.sdk_bridge,
            duration,
            join_timeout,
            &mut samples,
        )?;
        // phase returned only after its exact worker joined and latest dropped.
        emit(json!({"kind":"phase", "phase":label, "result":result}))?;
    }
    emit(json!({"kind":"finished", "same_process_sequence":true,
                "renderer_used":false, "performance_accepted":false}))
}

fn arguments(args: impl IntoIterator<Item = String>) -> Check<bool> {
    let args: Vec<_> = args.into_iter().collect();
    match args.as_slice() {
        [] => Ok(false),
        [run] if run == "--run" => Ok(true),
        _ => Err("use_run_with_private_json_stdin"),
    }
}

fn run() -> Check<()> {
    if !arguments(std::env::args().skip(1))? {
        println!(
            "{}",
            json!({"kind":"plan", "actions":false, "input":"JSON stdin only with --run",
            "sequence":["first_fhd","four_k","restored_fhd"], "renderer_used":false})
        );
        return Ok(());
    }
    let mut raw = Vec::new();
    io::stdin()
        .take((INPUT_BYTES + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|_| "input_unavailable")?;
    let request = input(&raw)?;
    for path in [
        request.state.as_path(),
        Path::new(&request.asset.path),
        Path::new(&request.asset.poster.path),
    ] {
        if fs::canonicalize(path).ok().as_deref() != Some(path) {
            return Err("canonical_existing_input_required");
        }
    }
    let duration = Duration::from_secs(request.phase_seconds);
    let adapter = Arc::new(NativeVideoWallpapers {
        state: request.state.clone(),
        library: request.state.clone(), // No discovery/import method is called.
    });
    let stdout = io::stdout();
    let mut output = stdout.lock();
    observe(
        adapter,
        request,
        duration,
        Duration::from_secs(JOIN_SECONDS),
        &mut memory,
        &mut allocator,
        &mut |value| writeln!(output, "{value}").map_err(|_| "output_unavailable"),
    )
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(reason) => {
            // Fixed labels only. Native stderr is a separate private runner log.
            println!(
                "{}",
                json!({"kind":"failed", "reason":reason, "next_phase_started":false,
                                 "performance_accepted":false})
            );
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
