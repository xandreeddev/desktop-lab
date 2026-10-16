//! Owned local media, decoded by Rust bindings to FFmpeg. No child player,
//! Wayland object, rendering resource or UI event loop belongs in this adapter.
use ffmpeg_next as ffmpeg;
use lucent_domain::*;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const SOURCE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ASSETS: usize = 32;
const LIBRARY_ENTRIES: usize = 1024;
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
        let mut bytes = vec![];
        fs::File::open(dir.join("asset.json"))
            .map_err(|_| storage())?
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| storage())?;
        if bytes.len() > 16 * 1024
            || serde_json::from_slice::<LiveWallpaperAsset>(&bytes).map_err(|_| storage())?
                != *asset
        {
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
        sources.truncate(MAX_ASSETS);
        Ok(sources)
    }
    fn prepare(&self, source: &Wallpaper) -> Result<LiveWallpaperAsset> {
        let file = Path::new(&source.path);
        valid_file(file)?;
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
        fs::create_dir_all(&self.state).map_err(|_| storage())?;
        if fs::read_dir(&self.state)
            .map_err(|_| storage())?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("media-"))
            .count()
            >= MAX_ASSETS
        {
            return Err(DomainError::Unavailable(
                "Live wallpaper asset store is full; remove unused media before importing".into(),
            ));
        }
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
            let input = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(file)
                .map_err(|_| storage())?;
            if !input.metadata().map_err(|_| storage())?.is_file() {
                return Err(storage());
            }
            let mut out = fs::File::create(&target).map_err(|_| storage())?;
            if std::io::copy(&mut input.take(SOURCE_BYTES + 1), &mut out).map_err(|_| storage())?
                > SOURCE_BYTES
            {
                return Err(DomainError::Invalid(
                    "Live wallpaper source exceeds 512 MiB".into(),
                ));
            }
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
fn decode(
    path: &Path,
    options: PlaybackOptions,
    emit: &mut dyn FnMut(MediaFrame) -> bool,
    stop: Arc<dyn StopSignal>,
) -> Result<()> {
    if !*INIT.get_or_init(|| {
        ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Quiet);
        ffmpeg::init().is_ok()
    }) {
        return Err(failed());
    }
    valid_file(path)?;
    loop {
        if stop.cancelled() {
            return Ok(());
        }
        let origin = Instant::now();
        let deadline = Arc::new(AtomicU64::new(5000));
        let interrupt_stop = stop.clone();
        let interrupt_deadline = deadline.clone();
        let mut dictionary = ffmpeg::Dictionary::new();
        dictionary.set("protocol_whitelist", "file");
        dictionary.set("probesize", "1048576");
        dictionary.set("analyzeduration", "2000000");
        let mut input = ffmpeg::format::input_with_interrupt_and_dictionary(
            path,
            move || {
                interrupt_stop.cancelled()
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
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or_else(failed)?;
        let index = stream.index();
        let time_base: f64 = stream.time_base().into();
        let rate: f64 = stream.avg_frame_rate().into();
        let mut context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|_| failed())?;
        context.set_threading(ffmpeg::codec::threading::Config::count(1));
        let mut decoder = context.decoder().video().map_err(|_| failed())?;
        dimensions(decoder.width(), decoder.height())?;
        let expected = (decoder.width(), decoder.height(), decoder.format());
        let mut scaler = ffmpeg::software::scaling::context::Context::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            ffmpeg::format::Pixel::RGBA,
            decoder.width(),
            decoder.height(),
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )
        .map_err(|_| failed())?;
        // swscale writes into one private conversion buffer for this decoder.
        // Published frames still own separate immutable bytes: consumers can
        // retain an old frame while the worker converts the next one.
        let mut rgba = ffmpeg::util::frame::video::Video::empty();
        let mut clock = FrameClock::new(options.fps.value());
        let mut count = 0u64;
        let mut first = None;
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
                dimensions(frame.width(), frame.height())?;
                if (frame.width(), frame.height(), frame.format()) != expected {
                    return Err(DomainError::Invalid(
                        "Video changes dimensions or pixel format during playback".into(),
                    ));
                }
                let raw = frame
                    .timestamp()
                    .map(|pts| pts as f64 * time_base)
                    .filter(|v| v.is_finite())
                    .unwrap_or_else(|| {
                        count as f64
                            / if rate.is_finite() && rate > 0. {
                                rate
                            } else {
                                30.
                            }
                    });
                let base = *first.get_or_insert(raw);
                let seconds = (raw - base).max(0.);
                count += 1;
                if !seconds.is_finite() || seconds > 24. * 60. * 60. {
                    return Err(failed());
                }
                let timestamp = Duration::from_secs_f64(seconds);
                let duration = frame.packet().duration as f64 * time_base;
                let hold = if duration.is_finite() && duration > 0. && duration < 60. {
                    Duration::from_secs_f64(duration)
                } else {
                    Duration::from_secs_f64(
                        1. / if rate.is_finite() && rate > 0. {
                            rate
                        } else {
                            f64::from(options.fps.value())
                        },
                    )
                };
                end_time = timestamp.saturating_add(hold);
                if !clock.accept(timestamp, started.elapsed()) {
                    continue;
                }
                let wait = timestamp.saturating_sub(started.elapsed());
                if wait > Duration::ZERO {
                    stop.wait(wait);
                }
                if stop.cancelled() {
                    return Ok(false);
                }
                scaler.run(&frame, &mut rgba).map_err(|_| failed())?;
                let row = frame.width() as usize * 4;
                let stride = rgba.stride(0);
                let plane = rgba.data(0);
                let mut bytes = Vec::with_capacity(row * frame.height() as usize);
                for y in 0..frame.height() as usize {
                    let start = y.checked_mul(stride).ok_or_else(failed)?;
                    bytes.extend_from_slice(plane.get(start..start + row).ok_or_else(failed)?);
                }
                delivered = true;
                if !emit(MediaFrame::rgba(
                    frame.width(),
                    frame.height(),
                    bytes,
                    timestamp,
                )?) {
                    return Ok(false);
                }
            }
        };
        loop {
            if stop.cancelled() {
                return Ok(());
            }
            deadline.store(
                origin.elapsed().as_millis() as u64 + 5000,
                Ordering::Relaxed,
            );
            let mut packet = ffmpeg::Packet::empty();
            match packet.read(&mut input) {
                Ok(()) => {}
                Err(ffmpeg::Error::Eof) => break,
                Err(_) => return Err(failed()),
            }
            if packet.size() > 16 * 1024 * 1024 {
                return Err(DomainError::Invalid(
                    "Video packet exceeds the media envelope".into(),
                ));
            }
            if packet.stream() == index {
                decoder.send_packet(&packet).map_err(|_| failed())?;
                if !frames(&mut decoder)? {
                    return Ok(());
                }
            }
        }
        decoder.send_eof().map_err(|_| failed())?;
        if !frames(&mut decoder)? || stop.cancelled() {
            return Ok(());
        }
        drop(frames);
        if !delivered {
            return Err(failed());
        }
        stop.wait(end_time.saturating_sub(started.elapsed()));
        if !options.looping {
            return Ok(());
        }
    }
}
struct FrameClock {
    interval: Duration,
    next: Duration,
    first: bool,
}
impl FrameClock {
    fn new(fps: u8) -> Self {
        Self {
            interval: Duration::from_secs_f64(1. / f64::from(fps)),
            next: Duration::ZERO,
            first: true,
        }
    }
    fn accept(&mut self, timestamp: Duration, elapsed: Duration) -> bool {
        if !self.first
            && (timestamp.saturating_add(Duration::from_millis(1)) < self.next
                || elapsed.saturating_sub(timestamp) > self.interval)
        {
            return false;
        }
        self.first = false;
        self.next = timestamp.saturating_add(self.interval);
        true
    }
}
#[cfg(test)]
mod tests;
