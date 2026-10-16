use super::*;
#[test]
fn video_clock_caps_rate_without_dropping_regular_rounded_timestamps() {
    let mut clock = FrameClock::new(30);
    let mut count = 0;
    for n in 0..30 {
        let t = Duration::from_micros(n * 1_000_000 / 30);
        count += usize::from(clock.accept(t, t));
    }
    assert_eq!(count, 30);
    let mut clock = FrameClock::new(30);
    let mut count = 0;
    for n in 0..60 {
        let t = Duration::from_micros(n * 1_000_000 / 60);
        count += usize::from(clock.accept(t, t));
    }
    assert_eq!(count, 30);
    assert!(!clock.accept(Duration::from_secs(1), Duration::from_secs(2)));
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
    let mut retained = vec![];
    adapter.play(
        &asset,
        PlaybackOptions {
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
        let (width, height, pixels) = frame.into_rgba();
        assert_eq!((width, height), (17, 13));
        assert_eq!(
            pixels,
            expected.repeat(17 * 13),
            "later conversion must not mutate a retained frame or copy row padding"
        );
    }
}
