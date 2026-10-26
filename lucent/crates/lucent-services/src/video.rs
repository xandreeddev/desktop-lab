//! Owned local media, decoded by Rust bindings to FFmpeg. No child player,
//! Wayland object, rendering resource or UI event loop belongs in this adapter.
use ffmpeg_next as ffmpeg;
use lucent_domain::*;
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const SOURCE_BYTES: u64 = 512 * 1024 * 1024;
const LIBRARY_RESULTS: usize = 32;
const LIBRARY_ENTRIES: usize = 1024;
mod pixels;
mod preview;
mod store;
mod targets;
pub use targets::LatestWallpaperTargetAdapter;
static NEXT: AtomicU64 = AtomicU64::new(1);
static INIT: OnceLock<bool> = OnceLock::new();
#[derive(Clone)]
pub struct NativeVideoWallpapers {
    pub state: PathBuf,
    /// Non-recursive user library; discovery never imports or decodes a file.
    pub library: PathBuf,
}
impl Default for NativeVideoWallpapers {
    fn default() -> Self {
        Self {
            state: crate::state_directory().join("live-wallpapers"),
            library: std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("Videos/Wallpapers"),
        }
    }
}
fn failed() -> DomainError {
    DomainError::Failed("Live wallpaper media could not be decoded".into())
}
fn storage() -> DomainError {
    DomainError::Failed("Live wallpaper storage is unavailable".into())
}
fn valid_file(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(DomainError::Invalid(
            "Use an absolute local media path".into(),
        ));
    }
    let metadata = fs::metadata(path).map_err(|_| storage())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > SOURCE_BYTES {
        return Err(DomainError::Invalid(
            "Local media must be a nonempty regular file up to 512 MiB".into(),
        ));
    }
    Ok(())
}
impl NativeVideoWallpapers {
    fn owned(&self, asset: &LiveWallpaperAsset) -> Result<()> {
        let root = fs::canonicalize(&self.state).map_err(|_| storage())?;
        let file = Path::new(&asset.path);
        let dir = file.parent().ok_or_else(storage)?;
        if dir.parent() != Some(root.as_path())
            || !dir
                .file_name()
                .is_some_and(|s| s.to_string_lossy().starts_with("media-"))
            || fs::canonicalize(file).map_err(|_| storage())? != file
            || Path::new(&asset.poster.path) != dir.join("poster.png")
        {
            return Err(DomainError::Invalid(
                "Live wallpaper is not an owned asset".into(),
            ));
        }
        if store::metadata(&dir.join("asset.json"))? != *asset {
            return Err(DomainError::Invalid(
                "Live wallpaper asset metadata is invalid".into(),
            ));
        }
        valid_file(file)
    }
}
struct Never;
impl StopSignal for Never {
    fn cancelled(&self) -> bool {
        false
    }
    fn wait(&self, d: Duration) {
        std::thread::sleep(d)
    }
}
impl WallpaperPlaybackPort for NativeVideoWallpapers {
    fn preview(
        &self,
        source: &Wallpaper,
        options: WallpaperPreviewOptions,
        emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
        stop: Arc<dyn StopSignal>,
    ) {
        preview::run(source, options, emit, stop);
    }
    fn library(&self) -> Result<Vec<Wallpaper>> {
        if !self.library.is_absolute() {
            return Err(DomainError::Unavailable(
                "Live wallpaper library is unavailable".into(),
            ));
        }
        let entries = match fs::read_dir(&self.library) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(_) => {
                return Err(DomainError::Unavailable(
                    "Live wallpaper library could not be read".into(),
                ));
            }
        };
        let mut sources = Vec::new();
        for entry in entries.take(LIBRARY_ENTRIES).flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.path();
            let extension = path
                .extension()
                .and_then(|part| part.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(extension.as_str(), "mp4" | "webm" | "gif")
                || !entry
                    .metadata()
                    .is_ok_and(|metadata| metadata.len() > 0 && metadata.len() <= SOURCE_BYTES)
            {
                continue;
            }
            let Some(path_text) = path.to_str() else {
                continue;
            };
            let name = path
                .file_stem()
                .and_then(|part| part.to_str())
                .unwrap_or("Live wallpaper")
                .replace(['_', '-'], " ")
                .chars()
                .filter(|character| !character.is_control())
                .take(64)
                .collect();
            sources.push(Wallpaper {
                path: path_text.into(),
                name,
            });
        }
        sources.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
        sources.truncate(LIBRARY_RESULTS);
        Ok(sources)
    }
    fn prepare(&self, source: &Wallpaper) -> Result<LiveWallpaperAsset> {
        let file = Path::new(&source.path);
        let mut input = store::Source::open(file)?;
        let extension = file
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "mp4" | "webm" | "gif") {
            return Err(DomainError::Invalid(
                "Use a local MP4, WebM or animated GIF".into(),
            ));
        }
        if let Some(asset) = self.reusable(source, &mut input)? {
            return Ok(asset);
        }
        fs::create_dir_all(&self.state).map_err(|_| storage())?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| storage())?
            .as_nanos();
        let dir = fs::canonicalize(&self.state)
            .map_err(|_| storage())?
            .join(format!(
                "media-{stamp}-{}",
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|_| storage())?;
        let result = (|| {
            let target = dir.join(format!("source.{extension}"));
            let mut out = fs::File::create(&target).map_err(|_| storage())?;
            input.copy_to(&mut out)?;
            out.flush().map_err(|_| storage())?;
            out.sync_all().map_err(|_| storage())?;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o400))
                .map_err(|_| storage())?;
            let mut poster = None;
            decode(
                &target,
                PlaybackOptions {
                    looping: false,
                    ..Default::default()
                },
                DecodePurpose::Poster,
                &mut |frame| {
                    poster = Some(frame);
                    false
                },
                Arc::new(Never),
            )?;
            let (width, height, rgba) = poster.ok_or_else(failed)?.into_rgba();
            let poster = dir.join("poster.png");
            image::save_buffer_with_format(
                &poster,
                &rgba,
                width,
                height,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )
            .map_err(|_| storage())?;
            fs::set_permissions(&poster, fs::Permissions::from_mode(0o400))
                .map_err(|_| storage())?;
            let asset = LiveWallpaperAsset {
                source: source.clone(),
                path: target.to_string_lossy().into(),
                poster: Wallpaper {
                    path: poster.to_string_lossy().into(),
                    name: source.name.clone(),
                },
            };
            fs::write(
                dir.join("asset.json"),
                serde_json::to_vec(&asset).map_err(|_| storage())?,
            )
            .map_err(|_| storage())?;
            Ok(asset)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&dir);
        }
        result
    }
    fn play(
        &self,
        asset: &LiveWallpaperAsset,
        options: PlaybackOptions,
        emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
        stop: Arc<dyn StopSignal>,
    ) {
        let result = self.owned(asset).and_then(|()| {
            decode(
                Path::new(&asset.path),
                options,
                DecodePurpose::LiveWallpaper,
                &mut |frame| emit(Ok(frame)),
                stop.clone(),
            )
        });
        if let Err(error) = result
            && !stop.cancelled()
        {
            emit(Err(error));
        }
    }
    fn play_retargetable(
        &self,
        asset: &LiveWallpaperAsset,
        options: RetargetablePlaybackOptions,
        targets: Arc<dyn WallpaperTargetPort>,
        emit: &mut dyn FnMut(Result<TargetedMediaFrame>) -> bool,
        stop: Arc<dyn StopSignal>,
    ) {
        let result = self.owned(asset).and_then(|()| {
            decode_retargetable(
                Path::new(&asset.path),
                options,
                targets,
                &mut |frame| emit(Ok(frame)),
                stop.clone(),
            )
        });
        if let Err(error) = result
            && !stop.cancelled()
        {
            emit(Err(error));
        }
    }
}
fn dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || u64::from(width) * u64::from(height) * 4 > 64 * 1024 * 1024
    {
        Err(DomainError::Invalid(
            "Video dimensions exceed the 4096-pixel/64-MiB envelope".into(),
        ))
    } else {
        Ok(())
    }
}
fn conversion_dimensions(
    format: ffmpeg::format::Pixel,
    width: u32,
    height: u32,
    target: Option<WallpaperPixelTarget>,
) -> Result<(u32, u32)> {
    dimensions(width, height)?;
    if opaque_format(format)?
        && let Some(target) = target
    {
        target.cover_size(width, height)
    } else {
        Ok((width, height))
    }
}
fn opaque_format(format: ffmpeg::format::Pixel) -> Result<bool> {
    let descriptor = format.descriptor().ok_or_else(failed)?;
    Ok({
        // SAFETY: Pixel::descriptor returns a non-null pointer obtained from
        // av_pix_fmt_desc_get. FFmpeg owns this immutable format descriptor for
        // the process lifetime; this reads only its public flags field.
        let flags = unsafe { (*descriptor.as_ptr()).flags };
        // pixdesc.h documents alpha-capable formats (including PAL8) as
        // straight alpha. Resampling before the SDK premultiplies can mix
        // invisible RGB into an edge, so only proven opaque formats fit here.
        // The C macros are nonnegative signed constants (1<<7 and 1<<1).
        let alpha_flags =
            (ffmpeg::ffi::AV_PIX_FMT_FLAG_ALPHA | ffmpeg::ffi::AV_PIX_FMT_FLAG_PAL) as u64;
        flags & alpha_flags == 0
    })
}

/// The first decoded frame, not the codec's initial metadata, establishes the
/// conversion contract. Some decoders only discover their pixel format while
/// parsing packets. FFmpeg 9 can assert inside sws_getContext for Pixel::None
/// before it reaches its unsupported-format check.
struct FrameConverter {
    source: ffmpeg::software::scaling::context::Definition,
    scaler: ffmpeg::software::scaling::context::Context,
    rgba: ffmpeg::util::frame::video::Video,
    pixels: Option<pixels::Pool>,
}
impl FrameConverter {
    fn new(
        frame: &ffmpeg::util::frame::video::Video,
        target: Option<WallpaperPixelTarget>,
    ) -> Result<Self> {
        Self::new_sized(frame, target, None)
    }
    fn new_live(
        frame: &ffmpeg::util::frame::video::Video,
        target: Option<WallpaperPixelTarget>,
    ) -> Result<Self> {
        let mut converter = Self::new(frame, target)?;
        if opaque_format(frame.format())? {
            converter.pixels = Some(pixels::Pool::default());
        }
        Ok(converter)
    }
    fn new_sized(
        frame: &ffmpeg::util::frame::video::Video,
        target: Option<WallpaperPixelTarget>,
        preview: Option<WallpaperPreviewOptions>,
    ) -> Result<Self> {
        dimensions(frame.width(), frame.height())?;
        let format = frame.format();
        if format.descriptor().is_none()
            || !ffmpeg::software::scaling::support::input(format)
            || !ffmpeg::software::scaling::support::output(ffmpeg::format::Pixel::RGBA)
        {
            return Err(failed());
        }
        let (width, height) = match preview {
            Some(options) => {
                preview::conversion_dimensions(format, frame.width(), frame.height(), options)?
            }
            None => conversion_dimensions(format, frame.width(), frame.height(), target)?,
        };
        let scaler = ffmpeg::software::scaling::context::Context::get(
            format,
            frame.width(),
            frame.height(),
            ffmpeg::format::Pixel::RGBA,
            width,
            height,
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )
        .map_err(|_| failed())?;
        Ok(Self {
            source: *scaler.input(),
            scaler,
            // FFmpeg owns the aligned, padded conversion allocation. Published
            // frames own separate immutable bytes and may outlive this worker.
            rgba: ffmpeg::util::frame::video::Video::empty(),
            pixels: None,
        })
    }

    fn validate(&self, frame: &ffmpeg::util::frame::video::Video) -> Result<()> {
        dimensions(frame.width(), frame.height())?;
        if (frame.width(), frame.height(), frame.format())
            != (self.source.width, self.source.height, self.source.format)
        {
            return Err(DomainError::Invalid(
                "Video changes dimensions or pixel format during playback".into(),
            ));
        }
        Ok(())
    }

    fn convert(
        &mut self,
        frame: &ffmpeg::util::frame::video::Video,
        timestamp: Duration,
    ) -> Result<MediaFrame> {
        self.validate(frame)?;
        self.scaler
            .run(frame, &mut self.rgba)
            .map_err(|_| failed())?;
        let (width, height) = (self.scaler.output().width, self.scaler.output().height);
        if (self.rgba.width(), self.rgba.height()) != (width, height) {
            return Err(failed());
        }
        let row = width as usize * 4;
        let stride = self.rgba.stride(0);
        let plane = self.rgba.data(0);
        let pack = |bytes: &mut Vec<u8>| {
            for y in 0..height as usize {
                let start = y.checked_mul(stride).ok_or_else(failed)?;
                bytes.extend_from_slice(plane.get(start..start + row).ok_or_else(failed)?);
            }
            Ok(())
        };
        let length = row * height as usize;
        let bytes = match &mut self.pixels {
            Some(pool) => pool.frame(length, pack)?,
            None => pixels::packed(length, pack)?,
        };
        MediaFrame::rgba_shared_with_aspect(
            width,
            height,
            bytes,
            (self.source.width, self.source.height),
            timestamp,
        )
    }
}

fn admit_decoded_source(
    shape: (u32, u32, ffmpeg::format::Pixel),
    timestamp: Option<i64>,
    admitted: &mut Option<(u32, u32, ffmpeg::format::Pixel)>,
    rewind_at: &mut Option<i64>,
) -> Result<()> {
    dimensions(shape.0, shape.1)?;
    if let Some(original) = admitted {
        if *original != shape {
            return Err(DomainError::Invalid(
                "Video changes dimensions or pixel format during playback".into(),
            ));
        }
    } else {
        // A missing first decoded PTS can be an unseekable prefix. This fact
        // belongs to the source admission, never to a replaceable converter.
        if timestamp.is_none() {
            *rewind_at = None;
        }
        *admitted = Some(shape);
    }
    Ok(())
}

fn live_converter_for_target(
    converter: &mut Option<FrameConverter>,
    frame: &ffmpeg::util::frame::video::Video,
    target: Option<WallpaperPixelTarget>,
) -> Result<()> {
    if let Some(old) = converter.as_ref() {
        old.validate(frame)?;
    }
    let size = conversion_dimensions(frame.format(), frame.width(), frame.height(), target)?;
    if converter
        .as_ref()
        .is_some_and(|old| (old.scaler.output().width, old.scaler.output().height) != size)
    {
        // Drop only size-dependent scratch/pool ownership before replacement.
        // Shared published pixels remain valid until their final owner drops.
        *converter = None;
    }
    if converter.is_none() {
        *converter = Some(FrameConverter::new_live(frame, target)?);
    }
    Ok(())
}
fn convert_live_target(
    converter: &mut Option<FrameConverter>,
    frame: &ffmpeg::util::frame::video::Video,
    demand: WallpaperTargetDemand,
    timestamp: Duration,
) -> Result<TargetedMediaFrame> {
    live_converter_for_target(converter, frame, demand.target)?;
    Ok(TargetedMediaFrame {
        revision: demand.revision,
        frame: converter
            .as_mut()
            .ok_or_else(failed)?
            .convert(frame, timestamp)?,
    })
}
/// Adapter-local decoder policy. Poster extraction and read-only previews keep
/// their serial decoder even when the underlying source uses H.264.
#[derive(Clone, Copy)]
enum DecodePurpose {
    Poster,
    LiveWallpaper,
    Preview(WallpaperPreviewOptions),
}
impl DecodePurpose {
    fn threading(self, codec: ffmpeg::codec::Id) -> ffmpeg::codec::threading::Config {
        use ffmpeg::codec::threading::{Config, Type};
        let parallel = matches!(self, Self::LiveWallpaper) && codec == ffmpeg::codec::Id::H264;
        let mut config = Config::count(if parallel { 2 } else { 1 });
        // A count alone leaves FFmpeg's threading kind at None. Restrict the
        // measured frame-threading trial to live H.264; other codecs stay serial.
        config.kind = if parallel { Type::Frame } else { Type::None };
        config
    }
}
fn decode(
    path: &Path,
    options: PlaybackOptions,
    purpose: DecodePurpose,
    emit: &mut dyn FnMut(MediaFrame) -> bool,
    stop: Arc<dyn StopSignal>,
) -> Result<()> {
    decode_inner(path, options, purpose, emit, stop)
}
fn decode_preview(
    path: &Path,
    preview: WallpaperPreviewOptions,
    emit: &mut dyn FnMut(MediaFrame) -> bool,
    stop: Arc<dyn StopSignal>,
) -> Result<()> {
    decode_inner(
        path,
        PlaybackOptions {
            fps: WallpaperFrameRate::Source,
            looping: true,
            target: None,
        },
        DecodePurpose::Preview(preview),
        emit,
        stop,
    )
}
fn decode_inner(
    path: &Path,
    options: PlaybackOptions,
    purpose: DecodePurpose,
    emit: &mut dyn FnMut(MediaFrame) -> bool,
    stop: Arc<dyn StopSignal>,
) -> Result<()> {
    decode_controlled(
        path,
        options,
        purpose,
        &mut |frame, _| emit(frame),
        stop,
        None,
    )
}

struct PlaybackTargets {
    port: Arc<dyn WallpaperTargetPort>,
    current: WallpaperTargetDemand,
}
impl PlaybackTargets {
    fn new(port: Arc<dyn WallpaperTargetPort>) -> Result<Self> {
        let current = port.try_latest()?.ok_or_else(|| {
            DomainError::Unavailable("Initial wallpaper target is unavailable".into())
        })?;
        Ok(Self { port, current })
    }
    fn refresh(&mut self) -> Result<()> {
        if let Some(next) = self.port.try_latest()? {
            if next.revision < self.current.revision
                || (next.revision == self.current.revision && next.target != self.current.target)
            {
                return Err(DomainError::Invalid(
                    "Wallpaper target revision regressed or was reused".into(),
                ));
            }
            self.current = next;
        }
        Ok(())
    }
}

fn decode_retargetable(
    path: &Path,
    options: RetargetablePlaybackOptions,
    targets: Arc<dyn WallpaperTargetPort>,
    emit: &mut dyn FnMut(TargetedMediaFrame) -> bool,
    stop: Arc<dyn StopSignal>,
) -> Result<()> {
    let targets = PlaybackTargets::new(targets)?;
    let mut invalid_identity = false;
    decode_controlled(
        path,
        PlaybackOptions {
            fps: options.fps,
            looping: options.looping,
            target: None,
        },
        DecodePurpose::LiveWallpaper,
        &mut |frame, revision| {
            if let Some(revision) = revision {
                emit(TargetedMediaFrame { revision, frame })
            } else {
                invalid_identity = true;
                false
            }
        },
        stop,
        Some(targets),
    )?;
    if invalid_identity {
        return Err(failed());
    }
    Ok(())
}

fn decode_controlled(
    path: &Path,
    options: PlaybackOptions,
    purpose: DecodePurpose,
    emit: &mut dyn FnMut(MediaFrame, Option<WallpaperTargetRevision>) -> bool,
    stop: Arc<dyn StopSignal>,
    mut targets: Option<PlaybackTargets>,
) -> Result<()> {
    if !*INIT.get_or_init(|| {
        ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Quiet);
        ffmpeg::init().is_ok()
    }) {
        return Err(failed());
    }
    valid_file(path)?;
    let mut session = None;
    loop {
        if stop.cancelled() {
            return Ok(());
        }
        if session.is_none() {
            session = Some(DecoderSession::open(path, purpose, stop.clone())?);
        }
        let active = session.as_mut().ok_or_else(failed)?;
        let keep = match targets.as_mut() {
            Some(targets) => {
                active.cycle_controlled(options, purpose, emit, stop.as_ref(), Some(targets))?
            }
            None => active.cycle(
                options,
                purpose,
                &mut |frame| emit(frame, None),
                stop.as_ref(),
            )?,
        };
        if !keep || stop.cancelled() || !options.looping {
            return Ok(());
        }
        if active.rewind(stop.as_ref()) {
            continue;
        }
        // Other purposes/codecs and unsupported/failed seeks preserve the old
        // reopen path. Retire this owner before allocating its replacement.
        session = None;
    }
}

/// One admitted local input and its codec/conversion allocations. Only live
/// H.264 retains this owner across complete, drained playback cycles. Seeking
/// keeps the originally opened file; it never resolves a new source pathname.
struct DecoderSession {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    converter: Option<FrameConverter>,
    // Independent from converter lifetime: retargeting must not reinterpret a
    // later missing timestamp as the first decoded prefix or admit a new source.
    decoded_source: Option<(u32, u32, ffmpeg::format::Pixel)>,
    index: usize,
    time_base: ffmpeg::Rational,
    rate: f64,
    origin: Instant,
    deadline: Arc<AtomicU64>,
    rewind_at: Option<i64>,
}

/// Input::seek uses global AV_TIME_BASE units and passes the range end as the
/// inclusive native max_ts. Round up so sub-microsecond stream starts remain
/// inside that bound, including negative starts. Invalid/unknown metadata keeps
/// the existing reopen path rather than guessing a start of zero.
fn rewind_timestamp(start: i64, time_base: ffmpeg::Rational) -> Option<i64> {
    if start == ffmpeg::ffi::AV_NOPTS_VALUE || time_base.0 <= 0 || time_base.1 <= 0 {
        return None;
    }
    let numerator = i128::from(start) * i128::from(time_base.0) * 1_000_000;
    let denominator = i128::from(time_base.1);
    let ticks =
        numerator.div_euclid(denominator) + i128::from(numerator.rem_euclid(denominator) != 0);
    i64::try_from(ticks).ok()
}

impl DecoderSession {
    fn open(path: &Path, purpose: DecodePurpose, stop: Arc<dyn StopSignal>) -> Result<Self> {
        let origin = Instant::now();
        let deadline = Arc::new(AtomicU64::new(5000));
        let interrupt_deadline = deadline.clone();
        let mut dictionary = ffmpeg::Dictionary::new();
        dictionary.set("protocol_whitelist", "file");
        dictionary.set("probesize", "1048576");
        dictionary.set("analyzeduration", "2000000");
        let input = ffmpeg::format::input_with_interrupt_and_dictionary(
            path,
            move || {
                stop.cancelled()
                    || origin.elapsed().as_millis() as u64
                        > interrupt_deadline.load(Ordering::Relaxed)
            },
            dictionary,
        )
        .map_err(|_| failed())?;
        if !matches!(
            input.format().name(),
            "mov,mp4,m4a,3gp,3g2,mj2" | "matroska,webm" | "gif"
        ) {
            return Err(DomainError::Invalid(
                "Media container is not MP4, WebM or GIF".into(),
            ));
        }
        let (index, time_base, rate, rewind_at, mut context) = {
            let stream = input
                .streams()
                .best(ffmpeg::media::Type::Video)
                .ok_or_else(failed)?;
            let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .map_err(|_| failed())?;
            let reuse = matches!(purpose, DecodePurpose::LiveWallpaper)
                && context.id() == ffmpeg::codec::Id::H264;
            (
                stream.index(),
                stream.time_base(),
                stream.avg_frame_rate().into(),
                if reuse {
                    rewind_timestamp(stream.start_time(), stream.time_base())
                } else {
                    None
                },
                context,
            )
        };
        context.set_threading(purpose.threading(context.id()));
        let decoder = context.decoder().video().map_err(|_| failed())?;
        // Reject known oversized declarations before allocating decoded frames,
        // but permit unknown (zero) edges until the first frame establishes them.
        dimensions(decoder.width().max(1), decoder.height().max(1))?;
        Ok(Self {
            input,
            decoder,
            converter: None,
            decoded_source: None,
            index,
            time_base,
            rate,
            origin,
            deadline,
            rewind_at,
        })
    }

    fn rewind(&mut self, stop: &dyn StopSignal) -> bool {
        let Some(target) = self.rewind_at else {
            return false;
        };
        if stop.cancelled() {
            return false;
        }
        self.deadline.store(
            self.origin.elapsed().as_millis() as u64 + 5000,
            Ordering::Relaxed,
        );
        // This is one bounded attempt after EOF drain/final hold. A failed seek
        // discards the whole session; no partially repositioned decoder is used.
        if self.input.seek(target, ..target).is_err() || stop.cancelled() {
            return false;
        }
        self.decoder.flush();
        true
    }

    /// Returns false only when delivery/cancellation retires the body. EOF is
    /// fully drained and the final source hold completes before a loop rewind.
    fn cycle(
        &mut self,
        options: PlaybackOptions,
        purpose: DecodePurpose,
        emit: &mut dyn FnMut(MediaFrame) -> bool,
        stop: &dyn StopSignal,
    ) -> Result<bool> {
        self.cycle_controlled(options, purpose, &mut |frame, _| emit(frame), stop, None)
    }

    fn cycle_controlled(
        &mut self,
        options: PlaybackOptions,
        purpose: DecodePurpose,
        emit: &mut dyn FnMut(MediaFrame, Option<WallpaperTargetRevision>) -> bool,
        stop: &dyn StopSignal,
        mut targets: Option<&mut PlaybackTargets>,
    ) -> Result<bool> {
        let Self {
            input,
            decoder,
            converter,
            decoded_source,
            index,
            time_base,
            rate,
            origin,
            deadline,
            rewind_at,
        } = self;
        let mut timeline = PresentationTimeline::new((*time_base).into(), *rate);
        let mut clock = FrameClock::new(options.fps);
        let started = Instant::now();
        let mut delivered = false;
        let mut end_time = Duration::ZERO;
        let mut frames = |decoder: &mut ffmpeg::decoder::Video| -> Result<bool> {
            loop {
                let mut frame = ffmpeg::util::frame::video::Video::empty();
                match decoder.receive_frame(&mut frame) {
                    Ok(()) => {}
                    Err(ffmpeg::Error::Eof) => return Ok(true),
                    Err(ffmpeg::Error::Other { errno }) if errno == libc::EAGAIN => {
                        return Ok(true);
                    }
                    Err(_) => return Err(failed()),
                }
                if stop.cancelled() {
                    return Ok(false);
                }
                admit_decoded_source(
                    (frame.width(), frame.height(), frame.format()),
                    frame.timestamp(),
                    decoded_source,
                    rewind_at,
                )?;
                if converter.is_none() && targets.is_none() {
                    *converter = Some(match purpose {
                        DecodePurpose::Preview(preview) => {
                            FrameConverter::new_sized(&frame, None, Some(preview))?
                        }
                        DecodePurpose::Poster => FrameConverter::new(&frame, options.target)?,
                        DecodePurpose::LiveWallpaper => {
                            FrameConverter::new_live(&frame, options.target)?
                        }
                    });
                }
                if let Some(converter) = converter.as_ref() {
                    converter.validate(&frame)?;
                }
                let (timestamp, hold) =
                    timeline.frame(frame.timestamp(), frame.packet().duration)?;
                end_time = end_time.max(timestamp.saturating_add(hold));
                if !clock.accept(timestamp, started.elapsed(), hold) {
                    continue;
                }
                let wait = timestamp.saturating_sub(started.elapsed());
                if wait > Duration::ZERO {
                    stop.wait(wait);
                }
                if stop.cancelled() {
                    return Ok(false);
                }
                let (converted, revision) = if let Some(targets) = targets.as_deref_mut() {
                    targets.refresh()?;
                    let value = convert_live_target(converter, &frame, targets.current, timestamp)?;
                    (value.frame, Some(value.revision))
                } else {
                    (
                        converter
                            .as_mut()
                            .ok_or_else(failed)?
                            .convert(&frame, timestamp)?,
                        None,
                    )
                };
                delivered = true;
                if !emit(converted, revision) {
                    return Ok(false);
                }
            }
        };
        loop {
            if stop.cancelled() {
                return Ok(false);
            }
            deadline.store(
                origin.elapsed().as_millis() as u64 + 5000,
                Ordering::Relaxed,
            );
            let mut packet = ffmpeg::Packet::empty();
            match packet.read(input) {
                Ok(()) => {}
                Err(ffmpeg::Error::Eof) => break,
                Err(_) => return Err(failed()),
            }
            if packet.size() > 16 * 1024 * 1024 {
                return Err(DomainError::Invalid(
                    "Video packet exceeds the media envelope".into(),
                ));
            }
            if packet.stream() == *index {
                decoder.send_packet(&packet).map_err(|_| failed())?;
                if !frames(decoder)? {
                    return Ok(false);
                }
            }
        }
        decoder.send_eof().map_err(|_| failed())?;
        if !frames(decoder)? || stop.cancelled() {
            return Ok(false);
        }
        drop(frames);
        if !delivered {
            return Err(failed());
        }
        stop.wait(end_time.saturating_sub(started.elapsed()));
        Ok(true)
    }
}
/// Normalize decoder timestamps without rounding the source rate to an integer.
/// Missing timestamps continue from the preceding frame's duration, including
/// variable-rate media; only missing/invalid metadata uses the 30 Hz fallback.
struct PresentationTimeline {
    time_base: f64,
    fallback_period: Duration,
    origin: Option<f64>,
    next: Duration,
}
impl PresentationTimeline {
    fn new(time_base: f64, rate: f64) -> Self {
        let fallback_period = Duration::try_from_secs_f64(1. / rate)
            .ok()
            .filter(|duration| !duration.is_zero() && *duration < Duration::from_secs(60))
            .unwrap_or_else(|| Duration::from_secs_f64(1. / 30.));
        Self {
            time_base,
            fallback_period,
            origin: None,
            next: Duration::ZERO,
        }
    }
    fn frame(&mut self, pts: Option<i64>, packet_duration: i64) -> Result<(Duration, Duration)> {
        let valid_time_base = self.time_base.is_finite() && self.time_base > 0.;
        let raw = pts
            .filter(|_| valid_time_base)
            .map(|pts| pts as f64 * self.time_base)
            .filter(|seconds| seconds.is_finite());
        let timestamp = if let Some(raw) = raw {
            // A first PTS may follow frames without timestamps. Align that PTS
            // with their continuation rather than returning to media time zero.
            let origin = *self.origin.get_or_insert(raw - self.next.as_secs_f64());
            let seconds = (raw - origin).max(0.);
            if !seconds.is_finite() || seconds > 24. * 60. * 60. {
                return Err(failed());
            }
            Duration::from_secs_f64(seconds)
        } else {
            self.next
        };
        if timestamp > Duration::from_secs(24 * 60 * 60) {
            return Err(failed());
        }
        let duration = packet_duration as f64 * self.time_base;
        let hold = if valid_time_base && duration.is_finite() && duration > 0. && duration < 60. {
            Duration::try_from_secs_f64(duration)
                .ok()
                .filter(|duration| !duration.is_zero())
                .unwrap_or(self.fallback_period)
        } else {
            self.fallback_period
        };
        self.next = self.next.max(timestamp.saturating_add(hold));
        Ok((timestamp, hold))
    }
}
struct FrameClock {
    interval: Option<Duration>,
    next: Duration,
    previous: Option<Duration>,
}
impl FrameClock {
    fn new(rate: WallpaperFrameRate) -> Self {
        Self {
            interval: rate
                .cap()
                .map(|fps| Duration::from_secs_f64(1. / f64::from(fps.value()))),
            next: Duration::ZERO,
            previous: None,
        }
    }
    fn accept(&mut self, timestamp: Duration, elapsed: Duration, hold: Duration) -> bool {
        if self.previous.is_some_and(|previous| timestamp <= previous)
            || (self.previous.is_some() && elapsed.saturating_sub(timestamp) > hold)
        {
            return false;
        }
        if let Some(interval) = self.interval {
            // Containers such as WebM round presentation timestamps to ms.
            // Keep that tolerance without resetting the cap phase on every
            // accepted frame, which would turn a 30 -> 24 cap into 15 FPS.
            if timestamp.saturating_add(Duration::from_millis(1)) < self.next {
                return false;
            }
            let slots = timestamp.saturating_sub(self.next).as_nanos() / interval.as_nanos() + 1;
            let advance = interval.as_nanos().saturating_mul(slots);
            self.next = self.next.saturating_add(Duration::from_nanos(
                u64::try_from(advance).unwrap_or(u64::MAX),
            ));
        }
        self.previous = Some(timestamp);
        true
    }
}
#[cfg(test)]
mod tests;
