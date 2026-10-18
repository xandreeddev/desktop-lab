use super::*;
#[test]
fn video_source_clock_preserves_actual_integer_and_fractional_cadences() {
    for fps in [15u32, 24, 30, 60, 120] {
        let mut clock = FrameClock::new(WallpaperFrameRate::Source);
        let hold = Duration::from_secs_f64(1. / f64::from(fps));
        let mut count = 0;
        for n in 0..fps {
            let timestamp = Duration::from_micros(u64::from(n) * 1_000_000 / u64::from(fps));
            count += usize::from(clock.accept(timestamp, timestamp, hold));
        }
        assert_eq!(count, fps as usize);
    }
    let mut clock = FrameClock::new(WallpaperFrameRate::Source);
    let hold = Duration::from_secs_f64(1001. / 60_000.);
    for n in 0..600 {
        let timestamp = Duration::from_secs_f64(f64::from(n) * 1001. / 60_000.);
        assert!(
            clock.accept(timestamp, timestamp, hold),
            "dropped frame {n}"
        );
    }
}
#[test]
fn video_fit_uses_ffmpeg_alpha_and_palette_descriptors_conservatively() {
    use ffmpeg::format::Pixel;
    let target = Some(WallpaperPixelTarget::new(3, 1).unwrap());
    for format in [
        Pixel::RGBA,
        Pixel::BGRA,
        Pixel::ARGB,
        Pixel::ABGR,
        Pixel::YA8,
        Pixel::YUVA420P,
        Pixel::GBRAP,
        Pixel::PAL8,
        Pixel::None,
    ] {
        assert_eq!(conversion_dimensions(format, 4, 2, target).unwrap(), (4, 2));
    }
    for format in [
        Pixel::YUV420P,
        Pixel::YUVJ420P,
        Pixel::YUV444P,
        Pixel::RGB24,
        Pixel::BGR24,
        Pixel::GRAY8,
    ] {
        assert_eq!(conversion_dimensions(format, 4, 2, target).unwrap(), (3, 2));
        assert_eq!(conversion_dimensions(format, 4, 2, None).unwrap(), (4, 2));
    }
    assert!(conversion_dimensions(Pixel::RGBA, 4097, 1, target).is_err());
    assert!(conversion_dimensions(Pixel::YUV420P, 4097, 1, target).is_err());
}
#[test]
fn video_cap_is_anchored_without_rounding_down_to_a_divisor_or_upsampling() {
    for (source, cap, expected) in [
        (30u32, 30u8, 30usize),
        (60, 30, 30),
        (30, 24, 24),
        (15, 60, 15),
        (120, 60, 60),
    ] {
        let mut clock = FrameClock::new(WallpaperFps::new(cap).unwrap().into());
        let hold = Duration::from_secs_f64(1. / f64::from(source));
        let mut count = 0;
        for n in 0..source {
            let timestamp = Duration::from_micros(u64::from(n) * 1_000_000 / u64::from(source));
            count += usize::from(clock.accept(timestamp, timestamp, hold));
        }
        assert_eq!(count, expected, "{source} source capped at {cap}");
    }
    let mut clock = FrameClock::new(WallpaperFps::new(30).unwrap().into());
    let hold = Duration::from_secs_f64(1001. / 60_000.);
    let count = (0..600)
        .filter(|n| {
            let timestamp = Duration::from_secs_f64(f64::from(*n) * 1001. / 60_000.);
            clock.accept(timestamp, timestamp, hold)
        })
        .count();
    assert_eq!(count, 300, "59.94 FPS must retain the cap's phase");
}
#[test]
fn video_source_clock_preserves_variable_intervals_and_drops_stale_or_repeated_frames() {
    let mut clock = FrameClock::new(WallpaperFrameRate::Source);
    for millis in [0, 10, 90, 600, 601] {
        let timestamp = Duration::from_millis(millis);
        assert!(clock.accept(timestamp, timestamp, Duration::from_millis(10)));
    }
    assert!(!clock.accept(
        Duration::from_millis(601),
        Duration::from_millis(601),
        Duration::from_millis(10)
    ));
    assert!(!clock.accept(
        Duration::from_millis(600),
        Duration::from_millis(601),
        Duration::from_millis(10)
    ));
    assert!(!clock.accept(
        Duration::from_millis(620),
        Duration::from_millis(700),
        Duration::from_millis(16)
    ));
    assert!(clock.accept(
        Duration::from_millis(710),
        Duration::from_millis(710),
        Duration::from_millis(16)
    ));

    let mut clock = FrameClock::new(WallpaperFps::new(30).unwrap().into());
    assert!(clock.accept(Duration::ZERO, Duration::ZERO, Duration::from_secs(1)));
    assert!(
        clock.accept(
            Duration::from_secs(1),
            Duration::from_millis(1500),
            Duration::from_secs(1)
        ),
        "a VFR frame that still covers media time must not use the cap interval as its expiry"
    );
    assert!(!clock.accept(
        Duration::from_secs(2),
        Duration::from_secs(4),
        Duration::from_secs(1)
    ));
    assert!(
        clock.accept(
            Duration::from_secs(4),
            Duration::from_secs(4),
            Duration::from_secs(1)
        ),
        "late frames must be skipped without a burst of obsolete deliveries"
    );
}
#[test]
fn video_timeline_uses_pts_and_frame_duration_for_variable_rate_and_missing_timestamps() {
    let mut timeline = PresentationTimeline::new(0.001, 30.);
    for (pts, duration, expected) in [
        (100_000, 20, 0),
        (100_020, 200, 20),
        (100_220, 10, 220),
        (100_230, 50, 230),
    ] {
        let (timestamp, hold) = timeline.frame(Some(pts), duration).unwrap();
        assert_eq!(timestamp, Duration::from_millis(expected));
        assert_eq!(hold, Duration::from_millis(duration as u64));
    }
    let (timestamp, _) = timeline.frame(None, 10).unwrap();
    assert_eq!(
        timestamp,
        Duration::from_millis(280),
        "missing PTS must continue from the previous source duration, even with a nonzero starting PTS"
    );

    let mut timeline = PresentationTimeline::new(0.001, 30.);
    assert_eq!(timeline.frame(None, 50).unwrap().0, Duration::ZERO);
    assert_eq!(
        timeline.frame(Some(100_000), 25).unwrap().0,
        Duration::from_millis(50)
    );
    assert_eq!(
        timeline.frame(Some(100_025), 25).unwrap().0,
        Duration::from_millis(75)
    );
}
#[test]
fn video_timeline_fallback_preserves_fractional_rate_and_rejects_unbounded_time() {
    let mut timeline = PresentationTimeline::new(f64::NAN, 60_000. / 1001.);
    for n in 0..600 {
        let (timestamp, _) = timeline.frame(None, 0).unwrap();
        let expected = Duration::from_secs_f64(f64::from(n) * 1001. / 60_000.);
        assert!(timestamp.abs_diff(expected) < Duration::from_micros(1));
    }
    for invalid_rate in [0., -1., f64::NAN, f64::INFINITY] {
        let mut timeline = PresentationTimeline::new(0., invalid_rate);
        assert_eq!(timeline.frame(None, 0).unwrap().0, Duration::ZERO);
        assert_eq!(
            timeline.frame(None, 0).unwrap().0,
            Duration::from_secs_f64(1. / 30.)
        );
    }
    let mut timeline = PresentationTimeline::new(1., 30.);
    assert_eq!(timeline.frame(Some(0), 0).unwrap().0, Duration::ZERO);
    assert!(timeline.frame(Some(86_401), 0).is_err());
}

use std::{process::Command, sync::atomic::AtomicBool};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "lucent-video-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn local_library_lists_bounded_regular_media_without_decoding_or_importing() {
    let root = Directory::new();
    let library = root.0.join("library");
    fs::create_dir(&library).unwrap();
    for name in ["Moon_lake.MP4", "Animated.gif", "Zebra.webm"] {
        fs::write(library.join(name), b"not decoded by discovery").unwrap();
    }
    for name in ["still.png", "notes.txt", "empty.mp4"] {
        fs::write(library.join(name), b"").unwrap();
    }
    fs::create_dir(library.join("nested.mp4")).unwrap();
    fs::write(library.join("nested.mp4/hidden.mp4"), b"nested").unwrap();
    std::os::unix::fs::symlink(library.join("Moon_lake.MP4"), library.join("alias.mp4")).unwrap();
    fs::File::create(library.join("too-big.mp4"))
        .unwrap()
        .set_len(SOURCE_BYTES + 1)
        .unwrap();
    let adapter = NativeVideoWallpapers {
        state: root.0.join("store"),
        library: library.clone(),
    };
    let sources = adapter.library().unwrap();
    assert_eq!(
        sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>(),
        ["Animated", "Moon lake", "Zebra"]
    );
    assert!(
        sources
            .iter()
            .all(|source| Path::new(&source.path).parent() == Some(library.as_path()))
    );
    assert!(
        !adapter.state.exists(),
        "discovery must not import or create storage"
    );
    for number in 0..64 {
        fs::write(library.join(format!("loop-{number:02}.mp4")), b"candidate").unwrap();
    }
    let sources = adapter.library().unwrap();
    assert_eq!(sources.len(), MAX_ASSETS);
    assert!(sources.windows(2).all(|pair| pair[0].name <= pair[1].name));
}

#[test]
fn missing_library_is_empty_but_invalid_or_unreadable_library_is_an_error() {
    let root = Directory::new();
    let mut adapter = NativeVideoWallpapers {
        state: root.0.join("store"),
        library: root.0.join("missing"),
    };
    assert!(adapter.library().unwrap().is_empty());
    adapter.library = PathBuf::from("relative-library");
    assert!(adapter.library().is_err());
    adapter.library = root.0.join("not-a-directory");
    fs::write(&adapter.library, b"file").unwrap();
    assert!(adapter.library().is_err());
}

struct Stop(AtomicBool);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    fn wait(&self, d: Duration) {
        let deadline = Instant::now() + d;
        while !self.cancelled() && Instant::now() < deadline {
            std::thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(5)),
            );
        }
    }
}
fn fixture(root: &Path, extension: &str) -> PathBuf {
    let file = root.join(format!("fixture.{extension}"));
    let mut command = Command::new("ffmpeg");
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-threads",
        "1",
        "-f",
        "lavfi",
        "-i",
        "color=c=red:s=16x12:r=10",
        "-t",
        "0.3",
    ]);
    match extension {
        "mp4" => {
            command.args(["-c:v", "mpeg4"]);
        }
        "webm" => {
            command.args(["-c:v", "libvpx", "-threads", "1"]);
        }
        _ => {}
    }
    let result = command
        .arg(&file)
        .output()
        .expect("ffmpeg is a required media runtime/fixture dependency");
    assert!(result.status.success(), "synthetic fixture encoding failed");
    file
}
#[test]
fn video_native_mp4_webm_and_gif_import_and_decode_through_the_real_port() {
    for extension in ["mp4", "webm", "gif"] {
        let root = Directory::new();
        let file = fixture(&root.0, extension);
        let adapter = NativeVideoWallpapers {
            state: root.0.join("store"),
            library: root.0.clone(),
        };
        let asset = adapter
            .prepare(&Wallpaper {
                path: file.to_string_lossy().into(),
                name: "Synthetic fixture".into(),
            })
            .unwrap();
        assert!(Path::new(&asset.poster.path).is_file());
        let stop = Arc::new(Stop(AtomicBool::new(false)));
        let mut count = 0;
        let mut timestamps = vec![];
        adapter.play(
            &asset,
            PlaybackOptions {
                looping: false,
                ..Default::default()
            },
            &mut |frame| {
                let frame = frame.unwrap();
                assert_eq!(frame.dimensions(), (16, 12));
                timestamps.push(frame.timestamp);
                let (_, _, rgba) = frame.into_rgba();
                assert!(rgba[0] > 200 && rgba[1] < 35 && rgba[2] < 35);
                count += 1;
                true
            },
            stop,
        );
        assert_eq!(count, 3);
        assert!(timestamps.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
#[test]
fn video_native_display_fit_preserves_source_poster_pixels_and_timestamps() {
    for extension in ["mp4", "webm", "gif"] {
        let root = Directory::new();
        let file = fixture(&root.0, extension);
        let source = fs::read(&file).unwrap();
        let adapter = NativeVideoWallpapers {
            state: root.0.join("store"),
            library: root.0.clone(),
        };
        let asset = adapter
            .prepare(&Wallpaper {
                path: file.to_string_lossy().into(),
                name: "Display fit fixture".into(),
            })
            .unwrap();
        let poster = fs::read(&asset.poster.path).unwrap();
        assert_eq!(
            image::image_dimensions(&asset.poster.path).unwrap(),
            (16, 12)
        );
        let mut reference_timestamps = vec![];
        for target in [
            None,
            Some(WallpaperPixelTarget::new(8, 5).unwrap()),
            Some(WallpaperPixelTarget::new(32, 24).unwrap()),
        ] {
            let mut timestamps = vec![];
            adapter.play(
                &asset,
                PlaybackOptions {
                    target,
                    looping: false,
                    ..Default::default()
                },
                &mut |frame| {
                    let frame = frame.unwrap();
                    timestamps.push(frame.timestamp);
                    assert_eq!(frame.aspect_size(), (16, 12));
                    let (width, height, rgba) = frame.into_rgba();
                    assert_eq!(
                        (width, height),
                        if extension != "gif" && target.is_some_and(|t| t.size() == (8, 5)) {
                            (8, 6)
                        } else {
                            (16, 12)
                        }
                    );
                    assert_eq!(rgba.len(), width as usize * height as usize * 4);
                    assert!(rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[0] > 200
                        && pixel[1] < 35
                        && pixel[2] < 35
                        && pixel[3] == 255));
                    true
                },
                Arc::new(Never),
            );
            assert_eq!(timestamps.len(), 3);
            if target.is_none() {
                reference_timestamps = timestamps;
            } else {
                assert_eq!(
                    timestamps, reference_timestamps,
                    "display size must not change media time"
                );
            }
        }
        assert_eq!(fs::read(&file).unwrap(), source);
        assert_eq!(fs::read(&asset.path).unwrap(), source);
        assert_eq!(fs::read(&asset.poster.path).unwrap(), poster);
        assert_eq!(fs::read_dir(&adapter.state).unwrap().count(), 1);
    }
}

fn transparent_edge_gif(path: &Path) {
    // Two 4×2 frames: opaque red beside transparent contrasting green. A clear
    // code before each palette index keeps every LZW code three bits wide.
    let mut data = b"GIF89a\x04\0\x02\0\x80\x01\0\xff\0\0\0\xff\0".to_vec();
    let mut compressed = vec![];
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
        // Transparent palette index 1, a 100-ms duration, then a full frame.
        data.extend_from_slice(b"\x21\xf9\x04\x01\x0a\0\x01\0");
        data.extend_from_slice(b"\x2c\0\0\0\0\x04\0\x02\0\0\x02");
        data.push(u8::try_from(compressed.len()).unwrap());
        data.extend_from_slice(&compressed);
        data.push(0);
    }
    data.push(0x3b);
    fs::write(path, data).unwrap();
}

#[test]
fn video_native_transparent_gif_fit_keeps_premultiplied_coverage_without_color_fringes() {
    let root = Directory::new();
    let file = root.0.join("transparent-edge.gif");
    transparent_edge_gif(&file);
    let original = fs::read(&file).unwrap();
    let adapter = NativeVideoWallpapers {
        state: root.0.join("store"),
        library: root.0.clone(),
    };
    let asset = adapter
        .prepare(&Wallpaper {
            path: file.to_string_lossy().into(),
            name: "Transparent edge".into(),
        })
        .unwrap();
    assert_eq!(image::image_dimensions(&asset.poster.path).unwrap(), (4, 2));
    let mut reference = vec![];
    for target in [None, Some(WallpaperPixelTarget::new(3, 1).unwrap())] {
        let mut frames = vec![];
        adapter.play(&asset, PlaybackOptions {
            target, looping: false, ..Default::default()
        }, &mut |frame| {
            let frame = frame.unwrap();
            let timestamp = frame.timestamp;
            let (width, height, rgba, aspect) = frame.into_rgba_with_aspect();
            assert_eq!((width, height), (4, 2),
                "alpha-capable conversion must preserve pixels before premultiplied GPU filtering");
            assert_eq!(aspect, (4, 2));
            for row in rgba.as_chunks::<{ 4 * 4 }>().0 {
                assert_eq!(&row[..8], &[255, 0, 0, 255, 255, 0, 0, 255]);
                assert_eq!(row[11], 0);
                assert_eq!(row[15], 0);
            }
            let prepared = lucent_api::ImageStream::new()
                .frame_rgba_with_aspect(width, height, rgba, aspect).unwrap();
            assert!(!prepared.is_opaque());
            // The centre output pixel at 3×1 samples exactly between source
            // columns 1 and 2. Interpolate the premultiplied render inputs:
            // 50% red coverage, with no invisible green/white RGB contribution.
            let pixels = prepared.pixels();
            let middle: Vec<f32> = (0..4).map(|channel|
                (f32::from(pixels[4 + channel]) + f32::from(pixels[8 + channel])) / 510.
            ).collect();
            assert_eq!(middle, [0.5, 0., 0., 0.5]);
            frames.push((timestamp, pixels.to_vec()));
            true
        }, Arc::new(Never));
        assert_eq!(frames.len(), 2);
        assert!(frames[1].0 > frames[0].0);
        if target.is_none() {
            reference = frames;
        } else {
            assert_eq!(
                frames, reference,
                "target cannot alter alpha pixels or timestamps"
            );
        }
    }
    assert_eq!(fs::read(&file).unwrap(), original);
    assert_eq!(fs::read(&asset.path).unwrap(), original);
}
#[test]
fn video_native_loop_cancellation_and_metadata_rejection_preserve_storage() {
    let root = Directory::new();
    let file = fixture(&root.0, "mp4");
    let adapter = NativeVideoWallpapers {
        state: root.0.join("store"),
        library: root.0.clone(),
    };
    let asset = adapter
        .prepare(&Wallpaper {
            path: file.to_string_lossy().into(),
            name: "Fixture".into(),
        })
        .unwrap();
    let stop = Arc::new(Stop(AtomicBool::new(false)));
    let mut count = 0;
    adapter.play(
        &asset,
        PlaybackOptions::default(),
        &mut |frame| {
            frame.unwrap();
            count += 1;
            if count == 5 {
                stop.0.store(true, Ordering::Relaxed);
            }
            true
        },
        stop.clone(),
    );
    assert_eq!(count, 5);
    assert!(stop.cancelled());
    let mut invalid = asset.clone();
    invalid.path = file.to_string_lossy().into();
    let mut error = false;
    adapter.play(
        &invalid,
        PlaybackOptions::default(),
        &mut |frame| {
            error = frame.is_err();
            false
        },
        Arc::new(Stop(AtomicBool::new(false))),
    );
    assert!(error);
    fs::write(root.0.join("broken.mp4"), b"invalid media").unwrap();
    assert!(
        adapter
            .prepare(&Wallpaper {
                path: root.0.join("broken.mp4").to_string_lossy().into(),
                name: "Broken".into()
            })
            .is_err()
    );
    assert_eq!(
        fs::read_dir(&adapter.state).unwrap().count(),
        1,
        "failed preparation removes only its candidate"
    );
}

#[test]
fn video_native_reused_conversion_preserves_retained_frame_bytes_and_odd_row_stride() {
    let root = Directory::new();
    let file = root.0.join("colors.mp4");
    let mut encoder = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-threads",
            "1",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            "17x13",
            "-framerate",
            "10",
            "-i",
            "pipe:0",
            "-c:v",
            "libx264rgb",
            "-crf",
            "0",
            "-threads",
            "1",
        ])
        .arg(&file)
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let colors = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
    {
        let mut input = encoder.stdin.take().unwrap();
        for color in colors {
            input.write_all(&color.repeat(17 * 13)).unwrap();
        }
    }
    assert!(encoder.wait_with_output().unwrap().status.success());
    let adapter = NativeVideoWallpapers {
        state: root.0.join("store"),
        library: root.0.clone(),
    };
    let asset = adapter
        .prepare(&Wallpaper {
            path: file.to_string_lossy().into(),
            name: "Colors".into(),
        })
        .unwrap();
    for target in [None, Some(WallpaperPixelTarget::new(9, 5).unwrap())] {
        let mut retained = vec![];
        adapter.play(
            &asset,
            PlaybackOptions {
                target,
                looping: false,
                ..Default::default()
            },
            &mut |frame| {
                retained.push(frame.unwrap());
                true
            },
            Arc::new(Never),
        );
        assert_eq!(retained.len(), colors.len());
        for (frame, expected) in retained.into_iter().zip(colors) {
            assert_eq!(frame.aspect_size(), (17, 13));
            let (width, height, pixels) = frame.into_rgba();
            assert_eq!(
                (width, height),
                if target.is_some() { (9, 7) } else { (17, 13) }
            );
            assert_eq!(
                pixels,
                expected.repeat(width as usize * height as usize),
                "later conversion must not mutate a retained frame or copy row padding"
            );
        }
    }
}
