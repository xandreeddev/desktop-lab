//! Opt-in, test-only stage experiment. Never compiled into the production library.
//! Each process runs one timed case, then an untimed pixel/PTS validation pass.
use super::*;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    mem::MaybeUninit,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    ptr::NonNull,
};

type ProbeResult<T> = std::result::Result<T, &'static str>;

fn check(value: bool, reason: &'static str) -> ProbeResult<()> {
    if value { Ok(()) } else { Err(reason) }
}

/// Existing libavutil SHA256, not a new hashing implementation or dependency.
struct Digest(NonNull<ffmpeg::ffi::AVSHA>);
impl Digest {
    fn new() -> ProbeResult<Self> {
        // SAFETY: av_sha_alloc returns owned storage; Drop pairs it with av_free.
        let pointer =
            NonNull::new(unsafe { ffmpeg::ffi::av_sha_alloc() }).ok_or("sha_allocation")?;
        let result = Self(pointer);
        // SAFETY: owned valid context and a supported 256-bit digest size.
        check(
            unsafe { ffmpeg::ffi::av_sha_init(result.0.as_ptr(), 256) } == 0,
            "sha_initialization",
        )?;
        Ok(result)
    }
    fn update(&mut self, bytes: &[u8]) {
        // SAFETY: context is initialized; the borrowed slice outlives this call.
        unsafe {
            ffmpeg::ffi::av_sha_update(self.0.as_ptr(), bytes.as_ptr(), bytes.len());
        }
    }
    fn finish(self) -> String {
        let mut bytes = [0u8; 32];
        // SAFETY: SHA256 writes exactly 32 bytes to the live output array.
        unsafe {
            ffmpeg::ffi::av_sha_final(self.0.as_ptr(), bytes.as_mut_ptr());
        }
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
impl Drop for Digest {
    fn drop(&mut self) {
        // SAFETY: one allocation owned by this non-clone wrapper, freed once.
        unsafe {
            ffmpeg::ffi::av_free(self.0.as_ptr().cast());
        }
    }
}
fn digest(bytes: &[u8]) -> ProbeResult<String> {
    let mut value = Digest::new()?;
    value.update(bytes);
    Ok(value.finish())
}
fn identity(value: &fs::Metadata) -> [i128; 8] {
    [
        value.dev() as i128,
        value.ino() as i128,
        value.mode() as i128,
        value.uid() as i128,
        value.gid() as i128,
        value.len() as i128,
        value.mtime() as i128 * 1_000_000_000 + value.mtime_nsec() as i128,
        value.ctime() as i128 * 1_000_000_000 + value.ctime_nsec() as i128,
    ]
}
struct Source {
    file: File,
    path: PathBuf,
    stamp: [i128; 8],
    sha256: String,
}
impl Source {
    fn open(path: &Path, expected: &str) -> ProbeResult<Self> {
        check(
            path.is_absolute() && fs::canonicalize(path).map_err(|_| "media_path")? == path,
            "media_path",
        )?;
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| "media_open")?;
        let before = file.metadata().map_err(|_| "media_metadata")?;
        // SAFETY: geteuid has no parameters and observes this process only.
        check(
            before.is_file()
                && before.uid() == unsafe { libc::geteuid() }
                && before.len() > 0
                && before.len() <= SOURCE_BYTES,
            "owned_bounded_media",
        )?;
        let stamp = identity(&before);
        let sha256 = Self::hash(&mut file, before.len())?;
        check(sha256 == expected, "media_digest")?;
        let result = Self {
            file,
            path: path.into(),
            stamp,
            sha256,
        };
        result.validate()?;
        Ok(result)
    }
    fn hash(file: &mut File, size: u64) -> ProbeResult<String> {
        file.seek(SeekFrom::Start(0)).map_err(|_| "media_rewind")?;
        let mut value = Digest::new()?;
        let mut block = vec![0u8; 1024 * 1024];
        let mut count = 0u64;
        loop {
            let read = file.read(&mut block).map_err(|_| "media_read")?;
            if read == 0 {
                break;
            }
            count += read as u64;
            check(count <= size && count <= SOURCE_BYTES, "media_size_changed")?;
            value.update(&block[..read]);
        }
        check(count == size, "media_size_changed")?;
        file.seek(SeekFrom::Start(0)).map_err(|_| "media_rewind")?;
        Ok(value.finish())
    }
    fn validate(&self) -> ProbeResult<()> {
        check(
            identity(&self.file.metadata().map_err(|_| "media_metadata")?) == self.stamp
                && identity(&fs::symlink_metadata(&self.path).map_err(|_| "media_path_metadata")?)
                    == self.stamp,
            "media_changed",
        )
    }
    fn decoder_path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd()))
    }
}
#[derive(Clone, Copy)]
enum Threads {
    Serial,
    Frame2,
}
impl Threads {
    fn config(self) -> ffmpeg::codec::threading::Config {
        use ffmpeg::codec::threading::{Config, Type};
        let (count, kind) = match self {
            Self::Serial => (1, Type::None),
            Self::Frame2 => (2, Type::Frame),
        };
        let mut config = Config::count(count);
        config.kind = kind;
        config
    }
    fn label(self) -> &'static str {
        match self {
            Self::Serial => "serial",
            Self::Frame2 => "frame2",
        }
    }
}
struct Config {
    source: PathBuf,
    source_sha256: String,
    report: PathBuf,
    threads: Threads,
    convert: bool,
    frames: usize,
    seconds: u64,
    target: WallpaperPixelTarget,
}
fn text<'a>(value: &'a serde_json::Value, key: &str) -> ProbeResult<&'a str> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|v| v.len() <= 4096)
        .ok_or("config_text")
}
impl Config {
    fn parse(value: &serde_json::Value) -> ProbeResult<Self> {
        let source = PathBuf::from(text(value, "source")?);
        let report = PathBuf::from(text(value, "report")?);
        check(
            source.is_absolute() && report.is_absolute() && source != report,
            "config_paths",
        )?;
        let source_sha256 = text(value, "source_sha256")?.to_owned();
        check(
            source_sha256.len() == 64
                && source_sha256
                    .bytes()
                    .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v)),
            "config_digest",
        )?;
        let threads = match text(value, "threads")? {
            "serial" => Threads::Serial,
            "frame2" => Threads::Frame2,
            _ => return Err("config_threads"),
        };
        let convert = match text(value, "stage")? {
            "decode" => false,
            "convert" => true,
            _ => return Err("config_stage"),
        };
        let frames = value["frames"].as_u64().ok_or("config_frames")?;
        let seconds = value["seconds"].as_u64().ok_or("config_seconds")?;
        check(
            (1..=1800).contains(&frames) && (1..=120).contains(&seconds),
            "config_bounds",
        )?;
        let edge = |key: &str| {
            value[key]
                .as_u64()
                .filter(|v| *v <= 4096)
                .map(|v| v as u32)
                .ok_or("config_target")
        };
        let target = WallpaperPixelTarget::new(edge("width")?, edge("height")?)
            .map_err(|_| "config_target")?;
        Ok(Self {
            source,
            source_sha256,
            report,
            threads,
            convert,
            frames: frames as usize,
            seconds,
            target,
        })
    }
    fn load(path: &Path) -> ProbeResult<Self> {
        check(
            path.is_absolute() && fs::canonicalize(path).map_err(|_| "config_path")? == path,
            "config_path",
        )?;
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| "config_open")?;
        let before = file.metadata().map_err(|_| "config_metadata")?;
        // SAFETY: geteuid observes the caller identity only.
        check(
            before.is_file()
                && before.uid() == unsafe { libc::geteuid() }
                && before.mode() & 0o777 == 0o600
                && before.nlink() == 1
                && before.len() <= 16384,
            "private_config",
        )?;
        let mut bytes = vec![];
        (&mut file)
            .take(16385)
            .read_to_end(&mut bytes)
            .map_err(|_| "config_read")?;
        check(
            bytes.len() <= 16384
                && identity(&file.metadata().map_err(|_| "config_metadata")?) == identity(&before)
                && identity(&fs::symlink_metadata(path).map_err(|_| "config_metadata")?)
                    == identity(&before),
            "config_changed",
        )?;
        Self::parse(&serde_json::from_slice(&bytes).map_err(|_| "config_json")?)
    }
}
#[derive(Clone, Copy)]
struct Resources {
    user_ns: u64,
    system_ns: u64,
    rss_kib: u64,
    high_water_kib: u64,
    threads: u64,
}
impl Resources {
    fn read() -> ProbeResult<Self> {
        let mut data = MaybeUninit::<libc::rusage>::uninit();
        // SAFETY: getrusage initializes its output on success; no other memory is accessed.
        check(
            unsafe { libc::getrusage(libc::RUSAGE_SELF, data.as_mut_ptr()) } == 0,
            "process_usage",
        )?;
        // SAFETY: successful getrusage initialized the complete structure.
        let data = unsafe { data.assume_init() };
        let time = |v: libc::timeval| v.tv_sec as u64 * 1_000_000_000 + v.tv_usec as u64 * 1000;
        let status = fs::read_to_string("/proc/self/status").map_err(|_| "process_status")?;
        check(status.len() <= 65536, "process_status_bound")?;
        let number = |key: &str| {
            status
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .and_then(|line| line.split_ascii_whitespace().next())
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or("process_status_shape")
        };
        Ok(Self {
            user_ns: time(data.ru_utime),
            system_ns: time(data.ru_stime),
            rss_kib: number("VmRSS:")?,
            high_water_kib: number("VmHWM:")?,
            threads: number("Threads:")?,
        })
    }
    fn json(self) -> serde_json::Value {
        serde_json::json!({"user_cpu_ns":self.user_ns,"system_cpu_ns":self.system_ns,
            "rss_kib":self.rss_kib,"process_lifetime_high_water_kib":self.high_water_kib,"threads":self.threads})
    }
}
#[derive(Default)]
struct Timings {
    demux_ns: u128,
    decoder_api_ns: u128,
    converter_setup_ns: u128,
    conversion_ns: u128,
}
struct Quality {
    yuv: Digest,
    rgba: Option<Digest>,
    pts: Digest,
}
impl Quality {
    fn new(convert: bool) -> ProbeResult<Self> {
        Ok(Self {
            yuv: Digest::new()?,
            rgba: if convert { Some(Digest::new()?) } else { None },
            pts: Digest::new()?,
        })
    }
    fn header(
        digest: &mut Digest,
        frame: &ffmpeg::util::frame::video::Video,
        stamp: (Option<i64>, u64),
    ) {
        digest.update(&frame.width().to_le_bytes());
        digest.update(&frame.height().to_le_bytes());
        digest.update(&[u8::from(stamp.0.is_some())]);
        digest.update(&stamp.0.unwrap_or_default().to_le_bytes());
        digest.update(&stamp.1.to_le_bytes());
    }
    fn record_yuv(
        &mut self,
        frame: &ffmpeg::util::frame::video::Video,
        stamp: (Option<i64>, u64),
    ) -> ProbeResult<()> {
        Self::header(&mut self.yuv, frame, stamp);
        self.pts.update(&[u8::from(stamp.0.is_some())]);
        self.pts.update(&stamp.0.unwrap_or_default().to_le_bytes());
        self.pts.update(&stamp.1.to_le_bytes());
        for plane in 0..3 {
            let width = if plane == 0 {
                frame.width()
            } else {
                frame.width().div_ceil(2)
            } as usize;
            let height = if plane == 0 {
                frame.height()
            } else {
                frame.height().div_ceil(2)
            } as usize;
            let stride = frame.stride(plane);
            for row in 0..height {
                self.yuv.update(
                    frame
                        .data(plane)
                        .get(row * stride..row * stride + width)
                        .ok_or("decoded_plane_bounds")?,
                );
            }
        }
        Ok(())
    }
    fn json(self) -> serde_json::Value {
        serde_json::json!({"visible_yuv420p_sha256":self.yuv.finish(),"converted_rgba_sha256":self.rgba.map(Digest::finish),"pts_sha256":self.pts.finish()})
    }
}
struct Pass {
    stamps: Vec<(Option<i64>, u64)>,
    shape: Option<(u32, u32)>,
    converter: Option<FrameConverter>,
    output: Option<(u32, u32)>,
    timings: Timings,
    quality: Option<Quality>,
    rss_max_kib: u64,
    threads_max: u64,
    deadline: Instant,
}
impl Pass {
    fn drain(
        &mut self,
        decoder: &mut ffmpeg::decoder::Video,
        timeline: &mut PresentationTimeline,
        config: &Config,
    ) -> ProbeResult<bool> {
        loop {
            check(Instant::now() < self.deadline, "stage_deadline")?;
            let mut frame = ffmpeg::util::frame::video::Video::empty();
            let began = Instant::now();
            let result = decoder.receive_frame(&mut frame);
            self.timings.decoder_api_ns += began.elapsed().as_nanos();
            match result {
                Ok(()) => {}
                Err(ffmpeg::Error::Eof) => return Ok(true),
                Err(ffmpeg::Error::Other { errno }) if errno == libc::EAGAIN => return Ok(false),
                Err(_) => return Err("decode_receive"),
            }
            dimensions(frame.width(), frame.height()).map_err(|_| "decoded_dimensions")?;
            check(
                frame.format() == ffmpeg::format::Pixel::YUV420P,
                "benchmark_requires_yuv420p",
            )?;
            let shape = (frame.width(), frame.height());
            if self.shape.is_none() {
                self.shape = Some(shape);
            }
            check(self.shape == Some(shape), "decoded_shape_changed")?;
            let (timestamp, _) = timeline
                .frame(frame.timestamp(), frame.packet().duration)
                .map_err(|_| "decoded_timeline")?;
            let stamp = (
                frame.timestamp(),
                u64::try_from(timestamp.as_nanos()).map_err(|_| "timestamp_bound")?,
            );
            if let Some(quality) = &mut self.quality {
                quality.record_yuv(&frame, stamp)?;
            }
            if config.convert {
                if self.converter.is_none() {
                    let began = Instant::now();
                    self.converter = Some(
                        FrameConverter::new(&frame, Some(config.target))
                            .map_err(|_| "converter_setup")?,
                    );
                    self.timings.converter_setup_ns += began.elapsed().as_nanos();
                }
                let began = Instant::now();
                let converted = self
                    .converter
                    .as_mut()
                    .ok_or("converter_missing")?
                    .convert(&frame, timestamp)
                    .map_err(|_| "converter_run")?;
                self.timings.conversion_ns += began.elapsed().as_nanos();
                let (width, height, bytes, aspect) = converted.into_rgba_with_aspect();
                self.output = Some((width, height));
                if let Some(quality) = self.quality.as_mut().and_then(|q| q.rgba.as_mut()) {
                    quality.update(&width.to_le_bytes());
                    quality.update(&height.to_le_bytes());
                    quality.update(&aspect.0.to_le_bytes());
                    quality.update(&aspect.1.to_le_bytes());
                    quality.update(&stamp.1.to_le_bytes());
                    quality.update(&bytes);
                }
            }
            self.stamps.push(stamp);
            if self.stamps.len().is_multiple_of(16) {
                let resources = Resources::read()?;
                self.rss_max_kib = self.rss_max_kib.max(resources.rss_kib);
                self.threads_max = self.threads_max.max(resources.threads);
            }
            if self.stamps.len() == config.frames {
                return Ok(true);
            }
        }
    }
}
fn pass(source: &Source, config: &Config, quality: bool) -> ProbeResult<(Pass, serde_json::Value)> {
    source.validate()?;
    let before = Resources::read()?;
    let started = Instant::now();
    let limit = Duration::from_secs(config.seconds);
    let interrupt = started;
    let mut dictionary = ffmpeg::Dictionary::new();
    dictionary.set("protocol_whitelist", "file");
    dictionary.set("probesize", "1048576");
    dictionary.set("analyzeduration", "2000000");
    let mut input = ffmpeg::format::input_with_interrupt_and_dictionary(
        &source.decoder_path(),
        move || interrupt.elapsed() > limit,
        dictionary,
    )
    .map_err(|_| "media_input")?;
    check(
        matches!(
            input.format().name(),
            "mov,mp4,m4a,3gp,3g2,mj2" | "matroska,webm"
        ),
        "benchmark_container",
    )?;
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or("video_stream")?;
    let index = stream.index();
    let source_time_base = format!("{:?}", stream.time_base());
    let source_frame_rate = format!("{:?}", stream.avg_frame_rate());
    let time_base: f64 = stream.time_base().into();
    let rate: f64 = stream.avg_frame_rate().into();
    let mut context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .map_err(|_| "decoder_context")?;
    check(
        context.id() == ffmpeg::codec::Id::H264,
        "benchmark_requires_h264",
    )?;
    context.set_threading(config.threads.config());
    let mut decoder = context.decoder().video().map_err(|_| "decoder_open")?;
    dimensions(decoder.width().max(1), decoder.height().max(1))
        .map_err(|_| "declared_dimensions")?;
    let active = decoder.threading();
    let expected = config.threads.config();
    check(
        active.kind == expected.kind && active.count == expected.count,
        "requested_threading_unavailable",
    )?;
    let setup_ns = started.elapsed().as_nanos();
    let active_json = serde_json::json!({"kind":format!("{:?}",active.kind),"count":active.count});
    let mut timeline = PresentationTimeline::new(time_base, rate);
    let mut state = Pass {
        stamps: vec![],
        shape: None,
        converter: None,
        output: None,
        timings: Timings::default(),
        quality: if quality {
            Some(Quality::new(config.convert)?)
        } else {
            None
        },
        rss_max_kib: before.rss_kib,
        threads_max: before.threads,
        deadline: started + limit,
    };
    let mut eof = false;
    loop {
        check(started.elapsed() < limit, "stage_deadline")?;
        let mut packet = ffmpeg::Packet::empty();
        let began = Instant::now();
        let read = packet.read(&mut input);
        state.timings.demux_ns += began.elapsed().as_nanos();
        match read {
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => {
                eof = true;
                break;
            }
            Err(_) => return Err("demux_read"),
        }
        check(packet.size() <= 16 * 1024 * 1024, "packet_bound")?;
        if packet.stream() == index {
            let began = Instant::now();
            let sent = decoder.send_packet(&packet);
            state.timings.decoder_api_ns += began.elapsed().as_nanos();
            sent.map_err(|_| "decode_send")?;
            if state.drain(&mut decoder, &mut timeline, config)? {
                break;
            }
        }
    }
    if eof && state.stamps.len() < config.frames {
        let began = Instant::now();
        decoder.send_eof().map_err(|_| "decode_eof")?;
        state.timings.decoder_api_ns += began.elapsed().as_nanos();
        state.drain(&mut decoder, &mut timeline, config)?;
    }
    check(!state.stamps.is_empty(), "no_decoded_frames")?;
    let before_retirement = Resources::read()?;
    state.rss_max_kib = state.rss_max_kib.max(before_retirement.rss_kib);
    state.threads_max = state.threads_max.max(before_retirement.threads);
    let retirement = Instant::now();
    drop(decoder);
    drop(input);
    drop(state.converter.take());
    let retirement_ns = retirement.elapsed().as_nanos();
    let wall_ns = started.elapsed().as_nanos();
    let after = Resources::read()?;
    source.validate()?;
    let result = serde_json::json!({"frames":state.stamps.len(),"requested_frames":config.frames,"input_eof_observed":eof,
        "setup_ns":setup_ns,"wall_ns":wall_ns,"decoder_api_wall_ns":state.timings.decoder_api_ns,
        "demux_wall_ns":state.timings.demux_ns,"converter_setup_wall_ns":state.timings.converter_setup_ns,
        "conversion_and_owned_rgba_wall_ns":state.timings.conversion_ns,"retirement_wall_ns":retirement_ns,
        "unpaced_output_prefix_frames_per_second":state.stamps.len() as f64/(wall_ns as f64/1e9),
        "source_dimensions":state.shape,"converted_dimensions":state.output,"active_threading":active_json,
        "source_time_base":source_time_base,"source_average_frame_rate":source_frame_rate,
        "before":before.json(),"after":after.json(),"sampled_rss_max_kib":state.rss_max_kib,"sampled_threads_max":state.threads_max,
        "user_cpu_ns":after.user_ns-before.user_ns,"system_cpu_ns":after.system_ns-before.system_ns,
        "scope":"Unpaced adapter stage throughput, not native presentation. API wall stages may overlap asynchronous decoder work. HWM is process-lifetime through this pass, including startup/source validation; RSS samples every 16 frames and pass boundaries."});
    Ok((state, result))
}
fn output(path: &Path) -> ProbeResult<File> {
    let parent = path.parent().ok_or("report_parent")?;
    let metadata = fs::symlink_metadata(parent).map_err(|_| "report_parent")?;
    // SAFETY: geteuid observes caller identity only.
    check(
        parent.is_absolute()
            && fs::canonicalize(parent).map_err(|_| "report_parent")? == parent
            && metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o777 == 0o700,
        "private_report_parent",
    )?;
    OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "report_must_be_absent")
}
fn publish(file: &mut File, path: &Path, value: &serde_json::Value) -> ProbeResult<()> {
    let before = identity(&file.metadata().map_err(|_| "report_metadata")?);
    check(
        identity(&fs::symlink_metadata(path).map_err(|_| "report_metadata")?) == before,
        "report_changed",
    )?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "report_json")?;
    file.seek(SeekFrom::Start(0)).map_err(|_| "report_rewind")?;
    file.set_len(0).map_err(|_| "report_truncate")?;
    file.write_all(&bytes).map_err(|_| "report_write")?;
    file.sync_all().map_err(|_| "report_sync")?;
    check(
        identity(&file.metadata().map_err(|_| "report_metadata")?)
            == identity(&fs::symlink_metadata(path).map_err(|_| "report_metadata")?),
        "report_changed",
    )
}
#[test]
#[ignore = "explicit private media/config and separately scheduled bounded process required"]
fn video_decode_stage_benchmark() {
    let path = std::env::var_os("LUCENT_VIDEO_STAGE_CONFIG")
        .map(PathBuf::from)
        .expect("private benchmark config required");
    let config = Config::load(&path).expect("private benchmark config rejected");
    let mut report_file = output(&config.report).expect("fresh private report required");
    let mut report = serde_json::json!({"schema":1,"state":"prepared","threads":config.threads.label(),"stage":if config.convert {"convert"} else {"decode"},
        "benchmark_source_sha256":digest(include_bytes!("benchmark.rs")).expect("source digest"),
        "adapter_source_sha256":digest(include_bytes!("../video.rs")).expect("source digest"),
        "libavcodec_version":ffmpeg::codec::version(),"libavformat_version":ffmpeg::format::version(),"libavutil_version":ffmpeg::util::version(),"libswscale_version":ffmpeg::software::scaling::version(),
        "physical_target":config.target.size(),"production_policy_changed":false,"native_presentation_or_resource_acceptance":false});
    publish(&mut report_file, &config.report, &report).expect("private initial report");
    let result = (|| -> ProbeResult<()> {
        ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Quiet);
        ffmpeg::init().map_err(|_| "ffmpeg_init")?;
        let mut source = Source::open(&config.source, &config.source_sha256)?;
        report["source_sha256"] = source.sha256.clone().into();
        report["phase"] = "timed".into();
        publish(&mut report_file, &config.report, &report)?;
        let (timed, timings) = pass(&source, &config, false)?;
        report["timed"] = timings;
        report["phase"] = "validation".into();
        publish(&mut report_file, &config.report, &report)?;
        let (mut validation, validation_cost) = pass(&source, &config, true)?;
        check(
            timed.stamps == validation.stamps
                && timed.shape == validation.shape
                && timed.output == validation.output,
            "validation_prefix_changed",
        )?;
        source.validate()?;
        check(
            Source::hash(
                &mut source.file,
                u64::try_from(source.stamp[5]).map_err(|_| "media_size")?,
            )? == source.sha256,
            "media_digest_changed",
        )?;
        source.validate()?;
        report["validation_cost"] = validation_cost;
        report["quality"] = validation
            .quality
            .take()
            .ok_or("validation_missing")?
            .json();
        report["timed_validation_pts_and_count_match"] = true.into();
        report["source_identity_preserved"] = true.into();
        report["state"] = "completed".into();
        report["phase"] = "complete".into();
        Ok(())
    })();
    if let Err(reason) = result {
        report["state"] = "failed".into();
        report["fixed_failure"] = reason.into();
    }
    publish(&mut report_file, &config.report, &report).expect("private final report");
    assert!(
        result.is_ok(),
        "controlled stage benchmark failed; inspect the private report"
    );
}
#[test]
fn benchmark_sha256_matches_standard_vector() {
    assert_eq!(
        digest(b"abc").unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
#[test]
fn benchmark_policy_is_explicit_and_preview_is_not_inferred() {
    use ffmpeg::codec::threading::Type;
    assert_eq!(Threads::Serial.config().kind, Type::None);
    assert_eq!(Threads::Serial.config().count, 1);
    assert_eq!(Threads::Frame2.config().kind, Type::Frame);
    assert_eq!(Threads::Frame2.config().count, 2);
}
#[test]
fn benchmark_config_refuses_unbounded_or_ambiguous_cases() {
    let valid = serde_json::json!({"source":"/owned/source.mp4","source_sha256":"a".repeat(64),"report":"/owned/report.json","threads":"frame2","stage":"convert","frames":360,"seconds":60,"width":1920,"height":1080});
    assert!(Config::parse(&valid).is_ok());
    for (key, value) in [
        ("threads", serde_json::json!("auto")),
        ("stage", serde_json::json!("play")),
        ("frames", serde_json::json!(1801)),
        ("seconds", serde_json::json!(0)),
        ("width", serde_json::json!(4097)),
        ("height", serde_json::json!(0)),
        ("source_sha256", serde_json::json!("not-a-digest")),
    ] {
        let mut changed = valid.clone();
        changed[key] = value;
        assert!(Config::parse(&changed).is_err(), "{key}");
    }
}

#[test]
fn benchmark_quality_hashes_visible_pixels_and_ignores_padding_only() {
    use ffmpeg::{format::Pixel, util::frame::video::Video};
    let mut frame = Video::new(Pixel::YUV420P, 14, 10);
    for plane in 0..3 {
        frame.data_mut(plane).fill(40 + plane as u8);
    }
    let stamp = (Some(42), 16_666_667);
    let calculate = |frame: &Video| {
        let mut quality = Quality::new(false).unwrap();
        quality.record_yuv(frame, stamp).unwrap();
        quality.json()["visible_yuv420p_sha256"].clone()
    };
    let before = calculate(&frame);
    // FFmpeg's aligned row storage must not make padding into quality evidence.
    let stride = frame.stride(0);
    if stride > 14 {
        frame.data_mut(0)[14] ^= 1;
    }
    assert_eq!(calculate(&frame), before);
    frame.data_mut(0)[0] ^= 1;
    assert_ne!(calculate(&frame), before);
}

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lucent-video-stage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn benchmark_refuses_existing_report_without_replacing_it() {
    let temp = Temporary::new();
    let path = temp.0.join("report.json");
    fs::write(&path, b"foreign report").unwrap();
    let before = identity(&fs::metadata(&path).unwrap());
    assert!(output(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"foreign report");
    assert_eq!(identity(&fs::metadata(&path).unwrap()), before);
}
#[test]
fn benchmark_source_pin_ignores_read_atime_but_refuses_later_content_mutation() {
    let temp = Temporary::new();
    let path = temp.0.join("source.mp4");
    fs::write(&path, b"bounded source fixture").unwrap();
    let expected = digest(b"bounded source fixture").unwrap();
    let source = Source::open(&path, &expected).unwrap();
    assert!(source.validate().is_ok());
    assert_eq!(fs::read(&path).unwrap(), b"bounded source fixture");
    assert!(source.validate().is_ok());
    fs::write(&path, b"changed source fixture").unwrap();
    assert!(source.validate().is_err());
}
