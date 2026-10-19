//! Read-only source previews. A pinned descriptor outlives every decoder loop;
//! neither discovery nor preview creates an imported asset or a poster.
use super::{SOURCE_BYTES, decode_preview, failed, ffmpeg};
use lucent_domain::*;
use std::{
    fs::{File, Metadata, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl From<Metadata> for Identity {
    fn from(value: Metadata) -> Self {
        Self {
            device: value.dev(),
            inode: value.ino(),
            bytes: value.len(),
            modified: (value.mtime(), value.mtime_nsec()),
            changed: (value.ctime(), value.ctime_nsec()),
        }
    }
}
struct Source {
    file: File,
    identity: Identity,
    decoder_path: PathBuf,
}
fn changed() -> DomainError {
    DomainError::Failed("Live wallpaper preview source is unavailable or changed".into())
}
/// Proven opaque formats can be reduced directly by FFmpeg. Alpha/palette
/// conversion retains its source pixels until the premultiplied fitter below.
pub(super) fn conversion_dimensions(
    format: ffmpeg::format::Pixel,
    width: u32,
    height: u32,
    options: WallpaperPreviewOptions,
) -> Result<(u32, u32)> {
    super::dimensions(width, height)?;
    let descriptor = format.descriptor().ok_or_else(failed)?;
    // SAFETY: FFmpeg owns the validated non-null immutable descriptor.
    let flags = unsafe { (*descriptor.as_ptr()).flags };
    let alpha = (ffmpeg::ffi::AV_PIX_FMT_FLAG_ALPHA | ffmpeg::ffi::AV_PIX_FMT_FLAG_PAL) as u64;
    if flags & alpha == 0 {
        options.fit_size(width, height)
    } else {
        Ok((width, height))
    }
}
impl Source {
    fn open(source: &Wallpaper) -> Result<Self> {
        let path = Path::new(&source.path);
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !path.is_absolute()
            || source.path.len() > 4096
            || source.path.chars().any(char::is_control)
            || !matches!(extension.as_str(), "mp4" | "webm" | "gif")
        {
            return Err(DomainError::Invalid(
                "Choose an absolute local MP4, WebM or animated GIF preview source".into(),
            ));
        }
        // A FIFO/device/symlink replacement cannot turn the regular-file check
        // into a blocking pathname open. FFmpeg subsequently opens this exact
        // live descriptor through procfs, never the original source pathname.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| changed())?;
        let metadata = file.metadata().map_err(|_| changed())?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > SOURCE_BYTES {
            return Err(DomainError::Invalid(
                "Preview media must be a nonempty regular file up to 512 MiB".into(),
            ));
        }
        let decoder_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
        Ok(Self {
            file,
            identity: metadata.into(),
            decoder_path,
        })
    }
    fn validate(&self) -> Result<()> {
        let metadata = self.file.metadata().map_err(|_| changed())?;
        if !metadata.is_file() || Identity::from(metadata) != self.identity {
            return Err(changed());
        }
        Ok(())
    }
}

/// The caller serializes preview bodies and owns source-generation authority.
/// This operation returns only after its decoder and source descriptor retire.
pub(super) fn run(
    source: &Wallpaper,
    options: WallpaperPreviewOptions,
    emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
    stop: Arc<dyn StopSignal>,
) {
    let result = (|| {
        if stop.cancelled() {
            return Ok(());
        }
        let source = Source::open(source)?;
        if stop.cancelled() {
            return Ok(());
        }
        let mut rejected = None;
        let result = decode_preview(
            &source.decoder_path,
            options,
            &mut |frame| {
                if stop.cancelled() {
                    return false;
                }
                let result = source
                    .validate()
                    .and_then(|()| fit(frame, options, stop.as_ref()))
                    .and_then(|frame| {
                        source.validate()?;
                        Ok(frame)
                    });
                match result {
                    Ok(Some(frame)) if !stop.cancelled() => emit(Ok(frame)),
                    Ok(_) => false,
                    Err(error) => {
                        rejected = Some(error);
                        false
                    }
                }
            },
            stop.clone(),
        );
        result?;
        if let Some(error) = rejected {
            return Err(error);
        }
        Ok(())
    })();
    if let Err(error) = result
        && !stop.cancelled()
    {
        emit(Err(error));
    }
}

fn fit(
    frame: MediaFrame,
    options: WallpaperPreviewOptions,
    stop: &dyn StopSignal,
) -> Result<Option<MediaFrame>> {
    if stop.cancelled() {
        return Ok(None);
    }
    let (width, height) = frame.dimensions();
    let size = options.fit_size(width, height)?;
    if size == (width, height) {
        return Ok(Some(frame));
    }
    let timestamp = frame.timestamp;
    let (_, _, mut pixels, aspect) = frame.into_rgba_with_aspect();
    // Resample premultiplied color and alpha together. Straight-alpha scaling
    // would leak invisible palette colors into the visible edge of a GIF.
    // Reuse the decoder-owned publication allocation rather than copying a
    // second full-size RGBA image. Codec/converter scratch remains separate.
    for row in pixels.chunks_exact_mut(width as usize * 4) {
        if stop.cancelled() {
            return Ok(None);
        }
        for pixel in row.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
    }
    let image = image::RgbaImage::from_raw(width, height, pixels).ok_or_else(failed)?;
    let mut pixels = image::imageops::resize(
        &image,
        size.0,
        size.1,
        image::imageops::FilterType::Triangle,
    )
    .into_raw();
    drop(image);
    for row in pixels.chunks_exact_mut(size.0 as usize * 4) {
        if stop.cancelled() {
            return Ok(None);
        }
        for pixel in row.as_chunks_mut::<4>().0 {
            let alpha = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = if alpha == 0 {
                    0
                } else {
                    ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8
                };
            }
        }
    }
    if stop.cancelled() {
        return Ok(None);
    }
    MediaFrame::rgba_with_aspect(size.0, size.1, pixels, aspect, timestamp).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, symlink},
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };
    struct Never;
    impl StopSignal for Never {
        fn cancelled(&self) -> bool {
            false
        }
        fn wait(&self, duration: Duration) {
            std::thread::sleep(duration);
        }
    }
    struct Cancelled;
    impl StopSignal for Cancelled {
        fn cancelled(&self) -> bool {
            true
        }
        fn wait(&self, _: Duration) {}
    }
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            for _ in 0..128 {
                let path = std::env::temp_dir().join(format!(
                    "lucent-video-preview-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("preview fixture allocation failed: {error}"),
                }
            }
            panic!("preview fixture allocation exhausted");
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn source(path: &Path) -> Wallpaper {
        Wallpaper {
            path: path.to_string_lossy().into(),
            name: "Preview fixture".into(),
        }
    }
    #[test]
    fn preview_fit_preserves_timestamp_aspect_and_premultiplied_edge_color() {
        let frame = MediaFrame::rgba_with_aspect(
            2,
            1,
            vec![0, 0, 255, 255, 255, 0, 0, 0],
            (65, 37),
            Duration::from_millis(125),
        )
        .unwrap();
        let fitted = fit(frame, WallpaperPreviewOptions::new(1, 1).unwrap(), &Never)
            .unwrap()
            .unwrap();
        assert_eq!(fitted.timestamp, Duration::from_millis(125));
        let (width, height, pixels, aspect) = fitted.into_rgba_with_aspect();
        assert_eq!((width, height), (1, 1));
        assert_eq!(aspect, (65, 37));
        assert_eq!(&pixels[..3], &[0, 0, 255]);
        assert!((126..=129).contains(&pixels[3]));
    }
    #[test]
    fn preview_fit_does_not_enlarge_or_copy_small_published_frames() {
        let pixels = vec![200, 40, 80, 0];
        let allocation = pixels.as_ptr();
        let frame = MediaFrame::rgba(1, 1, pixels, Duration::ZERO).unwrap();
        let fitted = fit(
            frame,
            WallpaperPreviewOptions::new(1024, 1024).unwrap(),
            &Never,
        )
        .unwrap()
        .unwrap();
        let (_, _, pixels) = fitted.into_rgba();
        assert_eq!(pixels.as_ptr(), allocation);
        assert_eq!(pixels, [200, 40, 80, 0]);
    }
    #[test]
    fn preview_fit_bounds_large_alpha_frames_and_cancellation_discards_them() {
        let frame = || {
            MediaFrame::rgba(2048, 8, [255, 0, 0, 128].repeat(2048 * 8), Duration::ZERO).unwrap()
        };
        let options = WallpaperPreviewOptions::new(1024, 1024).unwrap();
        let fitted = fit(frame(), options, &Never).unwrap().unwrap();
        assert_eq!(fitted.dimensions(), (1024, 4));
        assert_eq!(fitted.aspect_size(), (2048, 8));
        assert!(fit(frame(), options, &Cancelled).unwrap().is_none());
    }
    #[test]
    fn preview_conversion_contains_opaque_formats_and_retains_alpha_before_fitting() {
        use ffmpeg::format::Pixel;
        let options = WallpaperPreviewOptions::new(320, 180).unwrap();
        for format in [Pixel::RGB24, Pixel::YUV420P, Pixel::YUVJ420P, Pixel::GRAY8] {
            assert_eq!(
                conversion_dimensions(format, 3840, 2160, options).unwrap(),
                (320, 180)
            );
        }
        for format in [Pixel::RGBA, Pixel::BGRA, Pixel::YUVA420P, Pixel::PAL8] {
            assert_eq!(
                conversion_dimensions(format, 3840, 2160, options).unwrap(),
                (3840, 2160)
            );
        }
        assert!(conversion_dimensions(Pixel::None, 14, 10, options).is_err());
        assert!(conversion_dimensions(Pixel::RGB24, 4097, 1, options).is_err());
    }
    #[test]
    fn opaque_preview_converter_reduces_before_publication_and_keeps_natural_aspect() {
        use ffmpeg::{format::Pixel, util::frame::video::Video};
        let mut frame = Video::new(Pixel::YUV420P, 14, 10);
        frame.data_mut(0).fill(81);
        frame.data_mut(1).fill(90);
        frame.data_mut(2).fill(240);
        let mut converter = super::super::FrameConverter::new_sized(
            &frame,
            None,
            Some(WallpaperPreviewOptions::new(9, 5).unwrap()),
        )
        .unwrap();
        assert_eq!(
            (
                converter.scaler.output().width,
                converter.scaler.output().height
            ),
            (7, 5)
        );
        let converted = converter
            .convert(&frame, Duration::from_millis(75))
            .unwrap();
        assert_eq!(converted.dimensions(), (7, 5));
        assert_eq!(converted.aspect_size(), (14, 10));
        assert_eq!(converted.timestamp, Duration::from_millis(75));
    }
    #[test]
    fn preview_source_pins_the_opened_file_and_rejects_in_place_changes() {
        let root = Directory::new();
        let path = root.0.join("clip.gif");
        fs::write(&path, b"original").unwrap();
        let pinned = Source::open(&source(&path)).unwrap();
        pinned.validate().unwrap();
        fs::rename(&path, root.0.join("old.gif")).unwrap();
        fs::write(&path, b"replacement").unwrap();
        assert_eq!(fs::read(&pinned.decoder_path).unwrap(), b"original");
        fs::write(root.0.join("old.gif"), b"modified source").unwrap();
        assert!(pinned.validate().is_err());
    }
    #[test]
    fn preview_source_rejects_links_empty_oversized_and_nonregular_files_without_imports() {
        let root = Directory::new();
        let path = root.0.join("clip.gif");
        fs::write(&path, b"source").unwrap();
        let link = root.0.join("link.gif");
        symlink(&path, &link).unwrap();
        assert!(Source::open(&source(&link)).is_err());
        let empty = root.0.join("empty.gif");
        fs::write(&empty, []).unwrap();
        assert!(Source::open(&source(&empty)).is_err());
        let huge = root.0.join("huge.mp4");
        File::create(&huge)
            .unwrap()
            .set_len(SOURCE_BYTES + 1)
            .unwrap();
        assert!(Source::open(&source(&huge)).is_err());
        let directory = root.0.join("directory.gif");
        fs::create_dir(&directory).unwrap();
        assert!(Source::open(&source(&directory)).is_err());
        let fifo = root.0.join("pipe.gif");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: name is a live NUL-terminated path in our owned fixture directory.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(Source::open(&source(&fifo)).is_err());
        assert!(!root.0.join("store").exists());
    }
    #[test]
    fn already_cancelled_preview_never_opens_or_emits() {
        let root = Directory::new();
        run(
            &source(&root.0.join("absent.mp4")),
            WallpaperPreviewOptions::new(320, 180).unwrap(),
            &mut |_| panic!("cancelled preview emitted"),
            Arc::new(Cancelled),
        );
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
    }
    fn transparent_loop() -> Vec<u8> {
        // Two100-ms4×2 frames, with opaque red beside transparent green.
        // Frequent clear codes make the LZW code width unambiguous.
        let mut data = b"GIF89a\x04\0\x02\0\x80\x01\0\xff\0\0\0\xff\0".to_vec();
        let mut compressed = Vec::new();
        let (mut bits, mut used) = (0u32, 0u32);
        for code in [0u32, 0, 1, 1, 0, 0, 1, 1]
            .into_iter()
            .flat_map(|index| [4, index])
            .chain([5])
        {
            bits |= code << used;
            used += 3;
            while used >= 8 {
                compressed.push(bits as u8);
                bits >>= 8;
                used -= 8;
            }
        }
        if used > 0 {
            compressed.push(bits as u8);
        }
        for _ in 0..2 {
            data.extend_from_slice(b"\x21\xf9\x04\x01\x0a\0\x01\0");
            data.extend_from_slice(b"\x2c\0\0\0\0\x04\0\x02\0\0\x02");
            data.push(u8::try_from(compressed.len()).unwrap());
            data.extend_from_slice(&compressed);
            data.push(0);
        }
        data.push(0x3b);
        data
    }
    #[test]
    fn real_unimported_gif_preview_loops_bounded_frames_without_storage_writes() {
        let root = Directory::new();
        let path = root.0.join("clip.gif");
        let bytes = transparent_loop();
        fs::write(&path, &bytes).unwrap();
        let original: Identity = fs::metadata(&path).unwrap().into();
        let adapter = super::super::NativeVideoWallpapers {
            state: root.0.join("store"),
            library: root.0.clone(),
        };
        let mut timestamps = Vec::new();
        adapter.preview(
            &source(&path),
            WallpaperPreviewOptions::new(3, 1).unwrap(),
            &mut |frame| {
                let frame = frame.unwrap();
                timestamps.push(frame.timestamp);
                assert_eq!(frame.dimensions(), (2, 1));
                assert_eq!(frame.aspect_size(), (4, 2));
                let (_, _, pixels) = frame.into_rgba();
                assert_eq!(pixels.len(), 8);
                assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] < 255));
                assert!(
                    pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|pixel| pixel[1] == 0 && pixel[2] == 0)
                );
                timestamps.len() < 3
            },
            Arc::new(Never),
        );
        assert_eq!(
            timestamps.len(),
            3,
            "callback retirement must stop a looping preview"
        );
        assert!(timestamps[1] > timestamps[0]);
        assert_eq!(
            timestamps[2], timestamps[0],
            "source loop restarts its timeline"
        );
        assert!(Identity::from(fs::metadata(&path).unwrap()) == original);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
        assert!(!adapter.state.exists());
    }
    #[test]
    fn failed_preview_preserves_existing_store_entries_and_source_bytes() {
        let root = Directory::new();
        let path = root.0.join("bad.mp4");
        fs::write(&path, b"not media").unwrap();
        let store = root.0.join("store");
        fs::create_dir(&store).unwrap();
        let marker = store.join("existing");
        fs::write(&marker, b"keep").unwrap();
        let identity: Identity = fs::metadata(&marker).unwrap().into();
        let adapter = super::super::NativeVideoWallpapers {
            state: store.clone(),
            library: root.0.clone(),
        };
        let mut errors = 0;
        adapter.preview(
            &source(&path),
            WallpaperPreviewOptions::new(320, 180).unwrap(),
            &mut |frame| {
                assert!(frame.is_err());
                errors += 1;
                false
            },
            Arc::new(Never),
        );
        assert_eq!(errors, 1);
        assert_eq!(fs::read(&path).unwrap(), b"not media");
        assert_eq!(fs::read(&marker).unwrap(), b"keep");
        assert!(Identity::from(fs::metadata(&marker).unwrap()) == identity);
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 2);
        assert_eq!(fs::read_dir(&store).unwrap().count(), 1);
    }
}
