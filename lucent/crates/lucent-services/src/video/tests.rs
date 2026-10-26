use super::*;

struct TargetSequence(std::sync::Mutex<std::collections::VecDeque<Option<WallpaperTargetDemand>>>);
impl WallpaperTargetPort for TargetSequence {
    fn try_latest(&self) -> Result<Option<WallpaperTargetDemand>> {
        Ok(self.0.lock().unwrap().pop_front().unwrap_or(None))
    }
}

#[test]
fn dynamic_target_admission_refuses_initial_absence_reused_or_regressing_revisions() {
    let target = WallpaperPixelTarget::new(32, 24).unwrap();
    let initial = WallpaperTargetDemand {
        revision: WallpaperTargetRevision::new(2).unwrap(),
        target,
    };
    let port = |values: Vec<Option<WallpaperTargetDemand>>| {
        Arc::new(TargetSequence(std::sync::Mutex::new(values.into())))
            as Arc<dyn WallpaperTargetPort>
    };
    assert!(PlaybackTargets::new(port(vec![None])).is_err());
    for invalid in [
        WallpaperTargetDemand {
            revision: WallpaperTargetRevision::initial(),
            target,
        },
        WallpaperTargetDemand {
            revision: initial.revision,
            target: WallpaperPixelTarget::new(64, 48).unwrap(),
        },
    ] {
        let mut state = PlaybackTargets::new(port(vec![Some(initial), Some(invalid)])).unwrap();
        assert!(state.refresh().is_err());
        assert_eq!(state.current, initial);
    }
    let mut state = PlaybackTargets::new(port(vec![Some(initial), None, Some(initial)])).unwrap();
    state.refresh().unwrap();
    assert_eq!(state.current, initial);
    state.refresh().unwrap();
    assert_eq!(state.current, initial);
}

#[test]
fn dynamic_target_port_errors_preserve_the_last_admitted_demand() {
    struct FailsAfterAdmission(std::sync::Mutex<bool>);
    impl WallpaperTargetPort for FailsAfterAdmission {
        fn try_latest(&self) -> Result<Option<WallpaperTargetDemand>> {
            let mut called = self.0.lock().unwrap();
            if *called {
                return Err(DomainError::Unavailable("Target source retired".into()));
            }
            *called = true;
            Ok(Some(WallpaperTargetDemand {
                revision: WallpaperTargetRevision::initial(),
                target: WallpaperPixelTarget::new(32, 24).unwrap(),
            }))
        }
    }
    let mut targets =
        PlaybackTargets::new(Arc::new(FailsAfterAdmission(std::sync::Mutex::new(false)))).unwrap();
    let admitted = targets.current;
    assert!(matches!(
        targets.refresh(),
        Err(DomainError::Unavailable(_))
    ));
    assert_eq!(targets.current, admitted);
}

#[test]
fn absent_initial_target_fails_before_opening_a_decoder_or_delivering_pixels() {
    let absent = Arc::new(TargetSequence(std::sync::Mutex::new(
        [None].into_iter().collect(),
    )));
    assert!(matches!(
        decode_retargetable(
            Path::new("/unavailable/retarget-source.mp4"),
            RetargetablePlaybackOptions::default(),
            absent,
            &mut |_| panic!("No frame can precede target admission"),
            Arc::new(Never),
        ),
        Err(DomainError::Unavailable(_))
    ));
}

#[test]
fn converter_retirement_does_not_reclassify_a_later_missing_pts_or_new_source() {
    let source = (64, 48, ffmpeg::format::Pixel::YUV420P);
    let mut admitted = None;
    let mut rewind = Some(0);
    admit_decoded_source(source, Some(0), &mut admitted, &mut rewind).unwrap();
    // Converter replacement never changes the source-admission fact.
    admit_decoded_source(source, None, &mut admitted, &mut rewind).unwrap();
    assert_eq!(rewind, Some(0));
    assert!(admit_decoded_source((32, 24, source.2), Some(1), &mut admitted, &mut rewind).is_err());
    assert_eq!(admitted, Some(source));
    let mut prefix = None;
    admit_decoded_source(source, None, &mut prefix, &mut rewind).unwrap();
    assert_eq!(rewind, None);
}

#[test]
fn dynamic_alpha_and_paletted_demand_preserves_source_size_and_captured_revision() {
    use ffmpeg::{format::Pixel, util::frame::video::Video};
    let mut source = Video::new(Pixel::RGBA, 14, 10);
    source.data_mut(0).fill(127);
    let mut converter = None;
    let first = WallpaperTargetDemand {
        revision: WallpaperTargetRevision::initial(),
        target: WallpaperPixelTarget::new(7, 5).unwrap(),
    };
    let old = convert_live_target(&mut converter, &source, first, Duration::ZERO).unwrap();
    let original = old.frame.into_shared_rgba_with_aspect().2;
    let copy = original.as_slice().to_vec();
    let native = unsafe { converter.as_ref().unwrap().scaler.as_ptr() as usize };
    let second = WallpaperTargetDemand {
        revision: first.revision.checked_next().unwrap(),
        target: WallpaperPixelTarget::new(14, 10).unwrap(),
    };
    let next =
        convert_live_target(&mut converter, &source, second, Duration::from_millis(17)).unwrap();
    assert_eq!(next.revision, second.revision);
    assert_eq!(next.frame.dimensions(), (14, 10));
    assert_eq!(next.frame.aspect_size(), (14, 10));
    assert_eq!(
        unsafe { converter.as_ref().unwrap().scaler.as_ptr() as usize },
        native
    );
    assert!(converter.as_ref().unwrap().pixels.is_none());
    assert_eq!(original.as_slice(), copy);
    assert_eq!(
        conversion_dimensions(Pixel::PAL8, 14, 10, Some(first.target)).unwrap(),
        (14, 10)
    );
}

#[test]
fn one_h264_cycle_retargets_conversion_without_replacing_decoder_source_or_timing() {
    let root = Directory::new();
    let file = h264_fixture(&root.0);
    ffmpeg::init().unwrap();
    let small = WallpaperPixelTarget::new(32, 24).unwrap();
    let large = WallpaperPixelTarget::new(64, 48).unwrap();
    let mut references = Vec::new();
    for target in [small, large] {
        let mut frames = Vec::new();
        decode(
            &file,
            PlaybackOptions {
                fps: WallpaperFrameRate::Source,
                looping: false,
                target: Some(target),
            },
            DecodePurpose::Poster,
            &mut |frame| {
                frames.push(frame_snapshot(frame));
                true
            },
            Arc::new(Never),
        )
        .unwrap();
        references.push(frames);
    }
    let control = Arc::new(LatestWallpaperTargetAdapter::new(small));
    let mut targets = PlaybackTargets::new(control.clone()).unwrap();
    let mut session =
        DecoderSession::open(&file, DecodePurpose::LiveWallpaper, Arc::new(Never)).unwrap();
    // SAFETY: Read addresses of still-owned native handles; never dereference.
    let native = unsafe {
        (
            session.input.as_ptr() as usize,
            session.decoder.as_ptr() as usize,
        )
    };
    let origin = session.origin;
    let deadline = Arc::as_ptr(&session.deadline);
    let mut frames = Vec::new();
    let mut revisions = Vec::new();
    let mut old = None;
    let mut revision = WallpaperTargetRevision::initial();
    let policy = PlaybackOptions {
        fps: WallpaperFrameRate::Source,
        looping: true,
        target: None,
    };
    assert!(
        session
            .cycle_controlled(
                policy,
                DecodePurpose::LiveWallpaper,
                &mut |frame, captured| {
                    let timestamp = frame.timestamp;
                    let (width, height, pixels, aspect) = frame.into_shared_rgba_with_aspect();
                    if old.is_none() {
                        old = Some((pixels.clone(), pixels.as_slice().to_vec()));
                    }
                    let frame = MediaFrame::rgba_shared_with_aspect(
                        width, height, pixels, aspect, timestamp,
                    )
                    .unwrap();
                    frames.push(frame_snapshot(frame));
                    revisions.push(captured.unwrap());
                    if frames.len() == 2 {
                        revision = control.set(revision, large).unwrap().revision;
                    }
                    if frames.len() == 4 {
                        revision = control.set(revision, small).unwrap().revision;
                    }
                    true
                },
                &Never,
                Some(&mut targets)
            )
            .unwrap()
    );
    assert_eq!(
        frames.len(),
        8,
        "retargeting must preserve drained/reordered EOF frames"
    );
    for (index, actual) in frames.iter().enumerate() {
        assert_eq!(
            actual,
            &references[usize::from((2..4).contains(&index))][index]
        );
        assert_eq!(
            revisions[index].get(),
            if index < 2 {
                1
            } else if index < 4 {
                2
            } else {
                3
            }
        );
    }
    assert_eq!(
        unsafe {
            (
                session.input.as_ptr() as usize,
                session.decoder.as_ptr() as usize,
            )
        },
        native
    );
    assert_eq!(session.origin, origin);
    assert_eq!(Arc::as_ptr(&session.deadline), deadline);
    assert_eq!(session_allocations(&session).0, native.0);
    assert!(session.rewind(&Never));
    let mut calls = 0;
    assert!(
        !session
            .cycle_controlled(
                policy,
                DecodePurpose::LiveWallpaper,
                &mut |_, captured| {
                    calls += 1;
                    assert_eq!(captured, Some(revision));
                    false
                },
                &Never,
                Some(&mut targets)
            )
            .unwrap()
    );
    assert_eq!(calls, 1);
    let (held, copy) = old.unwrap();
    drop(session);
    assert_eq!(held.as_slice(), copy);
}

#[test]
fn dynamic_callback_cancellation_retires_pool_without_invalidating_held_pixels() {
    let root = Directory::new();
    let file = h264_fixture(&root.0);
    let control = Arc::new(LatestWallpaperTargetAdapter::new(
        WallpaperPixelTarget::new(32, 24).unwrap(),
    ));
    let mut held = None;
    let mut calls = 0;
    decode_retargetable(
        &file,
        RetargetablePlaybackOptions {
            fps: WallpaperFrameRate::Source,
            looping: true,
        },
        control,
        &mut |value| {
            calls += 1;
            held = Some(value.frame.into_shared_rgba_with_aspect().2);
            false
        },
        Arc::new(Never),
    )
    .unwrap();
    assert_eq!(calls, 1);
    let held = held.unwrap();
    assert_eq!(Arc::strong_count(&held), 1);
    assert_eq!(held.len(), 32 * 24 * 4);
}

#[test]
fn video_publication_pool_is_exclusive_to_proven_opaque_live_converters() {
    use ffmpeg::{format::Pixel, util::frame::video::Video};
    let opaque = Video::new(Pixel::YUV420P, 14, 10);
    assert!(
        FrameConverter::new_live(&opaque, None)
            .unwrap()
            .pixels
            .is_some()
    );
    assert!(FrameConverter::new(&opaque, None).unwrap().pixels.is_none());
    assert!(
        FrameConverter::new_sized(
            &opaque,
            None,
            Some(WallpaperPreviewOptions::new(9, 7).unwrap()),
        )
        .unwrap()
        .pixels
        .is_none()
    );
    let alpha = Video::new(Pixel::RGBA, 14, 10);
    assert!(
        FrameConverter::new_live(&alpha, None)
            .unwrap()
            .pixels
            .is_none()
    );
    assert!(!opaque_format(Pixel::PAL8).unwrap());
    assert!(opaque_format(Pixel::None).is_err());
}

#[test]
fn video_pool_reuses_released_pixels_but_preserves_sdk_images_and_fallbacks() {
    use ffmpeg::{format::Pixel, util::frame::video::Video};
    let mut source = Video::new(Pixel::YUV420P, 14, 10);
    source.data_mut(0).fill(81);
    source.data_mut(1).fill(90);
    source.data_mut(2).fill(240);
    let mut converter =
        FrameConverter::new_live(&source, Some(WallpaperPixelTarget::new(9, 5).unwrap())).unwrap();
    let (width, height, first, aspect) = converter
        .convert(&source, Duration::ZERO)
        .unwrap()
        .into_shared_rgba_with_aspect();
    let first_pointer = first.as_ptr();
    let original = (*first).clone();
    let stream = lucent_api::ImageStream::new();
    let image = stream
        .frame_rgba_shared_with_aspect(width, height, first, aspect)
        .unwrap();
    assert_eq!(image.pixels().as_ptr(), first_pointer);
    source.data_mut(0).fill(145);
    source.data_mut(1).fill(54);
    source.data_mut(2).fill(34);
    let second = converter
        .convert(&source, Duration::from_millis(17))
        .unwrap()
        .into_shared_rgba_with_aspect()
        .2;
    let second_pointer = second.as_ptr();
    assert_ne!(second_pointer, first_pointer);
    let fallback = converter
        .convert(&source, Duration::from_millis(34))
        .unwrap()
        .into_shared_rgba_with_aspect()
        .2;
    assert_ne!(fallback.as_ptr(), first_pointer);
    assert_ne!(fallback.as_ptr(), second_pointer);
    assert_eq!(image.pixels(), original);
    drop(second);
    let reused = converter
        .convert(&source, Duration::from_millis(51))
        .unwrap()
        .into_shared_rgba_with_aspect()
        .2;
    assert_eq!(reused.as_ptr(), second_pointer);
    assert_eq!(image.pixels(), original);
    drop(reused);
    drop(image);
    let reused = converter
        .convert(&source, Duration::from_millis(68))
        .unwrap()
        .into_shared_rgba_with_aspect()
        .2;
    assert_eq!(reused.as_ptr(), first_pointer);
    let weak = Arc::downgrade(&reused);
    drop(converter);
    assert_eq!(Arc::strong_count(&reused), 1);
    assert_eq!(reused.len(), 9 * 7 * 4);
    drop(reused);
    assert!(weak.upgrade().is_none());
}

#[test]
fn video_live_h264_publication_pool_survives_drained_eof_cycles() {
    let root = Directory::new();
    let file = h264_fixture(&root.0);
    ffmpeg::init().unwrap();
    let mut session =
        DecoderSession::open(&file, DecodePurpose::LiveWallpaper, Arc::new(Never)).unwrap();
    let options = PlaybackOptions {
        fps: WallpaperFrameRate::Source,
        looping: true,
        target: Some(WallpaperPixelTarget::new(32, 24).unwrap()),
    };
    let stream = lucent_api::ImageStream::new();
    let mut current = None;
    let mut pointers = std::collections::BTreeSet::new();
    let mut delivered = 0;
    for _ in 0..3 {
        assert!(
            session
                .cycle(
                    options,
                    DecodePurpose::LiveWallpaper,
                    &mut |frame| {
                        let (width, height, pixels, aspect) = frame.into_shared_rgba_with_aspect();
                        let pointer = pixels.as_ptr() as usize;
                        let image = stream
                            .frame_rgba_shared_with_aspect(width, height, pixels, aspect)
                            .unwrap();
                        assert_eq!(image.pixels().as_ptr() as usize, pointer);
                        pointers.insert(pointer);
                        delivered += 1;
                        current = Some(image);
                        true
                    },
                    &Never
                )
                .unwrap()
        );
        assert!(session.rewind(&Never));
    }
    assert_eq!(delivered, 24, "every cycle includes reordered EOF frames");
    assert_eq!(
        pointers.len(),
        2,
        "one held image permits a bounded pair of slots"
    );
    let last = current.unwrap();
    let original = last.pixels().to_vec();
    drop(session);
    assert_eq!(last.pixels(), original);
}

#[test]
fn video_retarget_retires_old_pool_without_mutating_held_pixels() {
    use ffmpeg::{format::Pixel, util::frame::video::Video};
    let mut source = Video::new(Pixel::YUV420P, 14, 10);
    source.data_mut(0).fill(81);
    source.data_mut(1).fill(90);
    source.data_mut(2).fill(240);
    let mut old = FrameConverter::new_live(&source, None).unwrap();
    let (width, height, pixels, aspect) = old
        .convert(&source, Duration::ZERO)
        .unwrap()
        .into_shared_rgba_with_aspect();
    let pointer = pixels.as_ptr();
    let stream = lucent_api::ImageStream::new();
    let held = stream
        .frame_rgba_shared_with_aspect(width, height, pixels, aspect)
        .unwrap();
    let original = held.pixels().to_vec();
    drop(old);
    assert_eq!(Arc::strong_count(&held.levels()[0].pixels), 1);
    let mut replacement =
        FrameConverter::new_live(&source, Some(WallpaperPixelTarget::new(7, 5).unwrap())).unwrap();
    let next = replacement
        .convert(&source, Duration::from_millis(17))
        .unwrap();
    assert_eq!(next.dimensions(), (7, 5));
    let pixels = next.into_shared_rgba_with_aspect().2;
    assert_ne!(pixels.as_ptr(), pointer);
    assert_eq!(held.pixels(), original);
}

#[test]
fn video_cancelled_live_owner_drops_pool_while_its_sdk_frame_stays_valid() {
    let root = Directory::new();
    let file = h264_fixture(&root.0);
    let stop = Arc::new(Stop(AtomicBool::new(false)));
    let stream = lucent_api::ImageStream::new();
    let mut held = None;
    let mut weak = None;
    decode(
        &file,
        PlaybackOptions::default(),
        DecodePurpose::LiveWallpaper,
        &mut |frame| {
            let (width, height, pixels, aspect) = frame.into_shared_rgba_with_aspect();
            weak = Some(Arc::downgrade(&pixels));
            held = Some(
                stream
                    .frame_rgba_shared_with_aspect(width, height, pixels, aspect)
                    .unwrap(),
            );
            stop.0.store(true, Ordering::Relaxed);
            true
        },
        stop.clone(),
    )
    .unwrap();
    assert!(stop.cancelled());
    let held = held.unwrap();
    assert_eq!(Arc::strong_count(&held.levels()[0].pixels), 1);
    assert!(held.is_opaque());
    assert!(!held.pixels().is_empty());
    drop(held);
    assert!(weak.unwrap().upgrade().is_none());
}

#[test]
fn video_frame_threading_is_exclusive_to_live_h264() {
    use ffmpeg::codec::{Id, threading::Type};
    for purpose in [
        DecodePurpose::Poster,
        DecodePurpose::Preview(WallpaperPreviewOptions::new(64, 48).unwrap()),
        DecodePurpose::LiveWallpaper,
    ] {
        for codec in [
            Id::H264,
            Id::HEVC,
            Id::MPEG4,
            Id::VP8,
            Id::VP9,
            Id::AV1,
            Id::GIF,
        ] {
            let config = purpose.threading(codec);
            let parallel = matches!(purpose, DecodePurpose::LiveWallpaper) && codec == Id::H264;
            assert_eq!(config.kind, if parallel { Type::Frame } else { Type::None });
            assert_eq!(config.count, if parallel { 2 } else { 1 });
        }
    }
}

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
    assert!(conversion_dimensions(Pixel::None, 4, 2, target).is_err());
}

#[test]
fn video_conversion_rejects_missing_decoded_format_before_native_context_creation() {
    let mut frame = ffmpeg::util::frame::video::Video::empty();
    frame.set_width(14);
    frame.set_height(10);
    frame.set_format(ffmpeg::format::Pixel::None);
    assert!(FrameConverter::new(&frame, None).is_err());
    assert!(FrameConverter::new(&ffmpeg::util::frame::video::Video::empty(), None).is_err());
}

#[test]
fn video_conversion_uses_decoded_frame_contract_and_preserves_owned_odd_rows() {
    use ffmpeg::{format::Pixel, util::frame::video::Video};
    let mut frame = Video::new(Pixel::YUV420P, 14, 10);
    frame.data_mut(0).fill(81);
    frame.data_mut(1).fill(90);
    frame.data_mut(2).fill(240);
    let mut converter =
        FrameConverter::new(&frame, Some(WallpaperPixelTarget::new(9, 5).unwrap())).unwrap();
    let first = converter.convert(&frame, Duration::ZERO).unwrap();
    assert_eq!(first.dimensions(), (9, 7));
    assert_eq!(first.aspect_size(), (14, 10));
    let first_bytes = first.into_rgba().2;
    let first_snapshot = first_bytes.clone();
    assert_eq!(first_bytes.len(), 9 * 7 * 4);
    assert!(
        first_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[0] > 200 && pixel[1] < 35 && pixel[2] < 35 && pixel[3] == 255)
    );
    // Reusing FFmpeg's private buffer cannot mutate a published older frame.
    frame.data_mut(0).fill(145);
    frame.data_mut(1).fill(54);
    frame.data_mut(2).fill(34);
    let second = converter
        .convert(&frame, Duration::from_millis(17))
        .unwrap();
    assert_ne!(second.into_rgba().2, first_bytes);
    assert_eq!(first_bytes, first_snapshot);
    // A new producer gets a fresh contract and conversion allocation. Changing
    // format or dimensions inside one producer still fails before sws_scale.
    for target in [None, Some(WallpaperPixelTarget::new(9, 5).unwrap()), None] {
        let mut replacement = FrameConverter::new(&frame, target).unwrap();
        assert_eq!(
            replacement
                .convert(&frame, Duration::ZERO)
                .unwrap()
                .dimensions(),
            if target.is_some() { (9, 7) } else { (14, 10) }
        );
    }
    for changed in [
        Video::new(Pixel::YUV420P, 16, 10),
        Video::new(Pixel::RGBA, 14, 10),
    ] {
        assert!(converter.convert(&changed, Duration::ZERO).is_err());
    }
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
    assert_eq!(sources.len(), LIBRARY_RESULTS);
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

fn h264_fixture(root: &Path) -> PathBuf {
    h264_fixture_with_options(root, "h264-b-frames.mp4", &[])
}
fn h264_fixture_with_options(root: &Path, name: &str, extra: &[&str]) -> PathBuf {
    use std::process::Stdio;
    let file = root.join(name);
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x48:rate=10",
            "-frames:v",
            "8",
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-threads",
            "1",
            "-bf",
            "2",
            "-g",
            "12",
            "-x264-params",
            "b-adapt=0",
            "-pix_fmt",
            "yuv420p",
        ])
        .args(extra)
        .arg(&file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("ffmpeg with H.264 encoding is a media fixture dependency");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "H.264 fixture encoding failed");
            return file;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("H.264 fixture encoding exceeded its bounded deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn frame_snapshot(frame: MediaFrame) -> (Duration, u32, u32, Vec<u8>, (u32, u32)) {
    let timestamp = frame.timestamp;
    let (width, height, rgba, aspect) = frame.into_rgba_with_aspect();
    (timestamp, width, height, rgba, aspect)
}

#[test]
fn video_live_h264_preserves_reordered_pixels_timestamps_and_eof_drain() {
    use ffmpeg::codec::threading::Type;
    let root = Directory::new();
    let file = h264_fixture(&root.0);
    let original = storage_snapshot(&file);
    ffmpeg::init().unwrap();
    let input = ffmpeg::format::input(&file).unwrap();
    let parameters = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .unwrap()
        .parameters();
    for (purpose, kind, count) in [
        (DecodePurpose::Poster, Type::None, 1),
        (
            DecodePurpose::Preview(WallpaperPreviewOptions::new(64, 48).unwrap()),
            Type::None,
            1,
        ),
        (DecodePurpose::LiveWallpaper, Type::Frame, 2),
    ] {
        let mut context =
            ffmpeg::codec::context::Context::from_parameters(parameters.clone()).unwrap();
        assert_eq!(context.id(), ffmpeg::codec::Id::H264);
        context.set_threading(purpose.threading(context.id()));
        let decoder = context.decoder().video().unwrap();
        assert_eq!(decoder.threading().kind, kind);
        assert_eq!(decoder.threading().count, count);
        assert!(
            decoder.has_b_frames(),
            "the fixture must exercise decoder reorder delay"
        );
    }
    drop(input);
    let source = Wallpaper {
        path: file.to_string_lossy().into(),
        name: "H.264 fixture".into(),
    };
    let adapter = NativeVideoWallpapers {
        state: root.0.join("store"),
        library: root.0.clone(),
    };
    let asset = adapter.prepare(&source).unwrap();
    let stored = storage_snapshot(&adapter.state);
    let mut full_reference = vec![];
    for target in [None, Some(WallpaperPixelTarget::new(32, 24).unwrap())] {
        let options = PlaybackOptions {
            fps: WallpaperFrameRate::Source,
            looping: false,
            target,
        };
        let mut reference = vec![];
        decode(
            &file,
            options,
            DecodePurpose::Poster,
            &mut |frame| {
                reference.push(frame_snapshot(frame));
                true
            },
            Arc::new(Never),
        )
        .unwrap();
        assert_eq!(
            reference.len(),
            8,
            "serial decoding must drain delayed EOF frames"
        );
        assert_eq!(
            reference.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            (0..8)
                .map(|index| Duration::from_millis(index * 100))
                .collect::<Vec<_>>()
        );
        let mut actual = vec![];
        adapter.play(
            &asset,
            options,
            &mut |frame| {
                actual.push(frame_snapshot(frame.unwrap()));
                true
            },
            Arc::new(Never),
        );
        assert_eq!(
            actual, reference,
            "Frame2 must preserve every reordered frame and fitted pixel"
        );
        if target.is_none() {
            full_reference = reference;
        }
    }
    let poster = image::open(&asset.poster.path).unwrap().into_rgba8();
    assert_eq!(poster.dimensions(), (64, 48));
    assert_eq!(poster.as_raw(), &full_reference[0].3);
    let mut preview = vec![];
    adapter.preview(
        &source,
        WallpaperPreviewOptions::new(64, 48).unwrap(),
        &mut |frame| {
            preview.push(frame_snapshot(frame.unwrap()));
            preview.len() < 8
        },
        Arc::new(Never),
    );
    assert_eq!(
        preview, full_reference,
        "serial preview must retain the same source pixels and PTS"
    );
    assert_eq!(storage_snapshot(&adapter.state), stored);
    assert_eq!(storage_snapshot(&file), original);
}

#[test]
fn video_live_h264_callback_and_loop_cancellation_retire_without_storage_changes() {
    let root = Directory::new();
    let file = h264_fixture(&root.0);
    let source = Wallpaper {
        path: file.to_string_lossy().into(),
        name: "H.264 retirement".into(),
    };
    let adapter = NativeVideoWallpapers {
        state: root.0.join("store"),
        library: root.0.clone(),
    };
    let asset = adapter.prepare(&source).unwrap();
    let stored = storage_snapshot(&adapter.state);
    let original = storage_snapshot(&file);
    let mut first = vec![];
    adapter.play(
        &asset,
        PlaybackOptions {
            fps: WallpaperFrameRate::Source,
            ..Default::default()
        },
        &mut |frame| {
            first.push(frame.unwrap().timestamp);
            first.len() < 3
        },
        Arc::new(Never),
    );
    assert_eq!(
        first.len(),
        3,
        "callback false must return with delayed frames still queued"
    );
    let stop = Arc::new(Stop(AtomicBool::new(false)));
    let mut looped = vec![];
    adapter.play(
        &asset,
        PlaybackOptions {
            fps: WallpaperFrameRate::Source,
            ..Default::default()
        },
        &mut |frame| {
            looped.push(frame.unwrap().timestamp);
            if looped.len() == 10 {
                stop.0.store(true, Ordering::Relaxed);
            }
            true
        },
        stop.clone(),
    );
    assert_eq!(looped.len(), 10);
    assert!(stop.cancelled());
    assert_eq!(
        looped[8],
        Duration::ZERO,
        "looping must reset the source timeline"
    );
    assert_eq!(&looped[8..], &first[..2]);
    adapter.play(
        &asset,
        PlaybackOptions::default(),
        &mut |_| panic!("an already-cancelled live decoder emitted"),
        stop,
    );
    assert_eq!(storage_snapshot(&adapter.state), stored);
    assert_eq!(storage_snapshot(&file), original);
}

#[test]
fn video_live_rewind_target_preserves_signed_starts_and_rejects_unknown_or_overflow() {
    use ffmpeg::Rational;
    for (start, time_base, expected) in [
        (0, Rational(1, 10), 0),
        (20, Rational(1, 10), 2_000_000),
        (-3, Rational(1, 10), -300_000),
        (1, Rational(1, 90_000), 12),
        (-1, Rational(1, 90_000), -11),
    ] {
        assert_eq!(rewind_timestamp(start, time_base), Some(expected));
    }
    for (start, time_base) in [
        (ffmpeg::ffi::AV_NOPTS_VALUE, Rational(1, 10)),
        (1, Rational(0, 10)),
        (1, Rational(-1, 10)),
        (1, Rational(1, 0)),
        (1, Rational(1, -10)),
        (i64::MAX, Rational(i32::MAX, 1)),
        (i64::MIN + 1, Rational(i32::MAX, 1)),
    ] {
        assert_eq!(rewind_timestamp(start, time_base), None);
    }
}

/// Observe allocations through binding-provided pointers without dereferencing
/// them. The owned session stays live across every comparison.
fn session_allocations(session: &DecoderSession) -> (usize, usize, usize, usize) {
    let converter = session.converter.as_ref().unwrap();
    // SAFETY: these methods expose addresses of the live owned FFmpeg handles;
    // the test never dereferences them or passes them to a native function.
    unsafe {
        (
            session.input.as_ptr() as usize,
            session.decoder.as_ptr() as usize,
            converter.scaler.as_ptr() as usize,
            converter.rgba.data(0).as_ptr() as usize,
        )
    }
}

#[test]
fn video_live_h264_reuses_actual_contexts_and_conversion_storage_after_eof() {
    let root = Directory::new();
    let file = h264_fixture(&root.0);
    let original = storage_snapshot(&file);
    ffmpeg::init().unwrap();
    for target in [None, Some(WallpaperPixelTarget::new(32, 24).unwrap())] {
        let options = PlaybackOptions {
            fps: WallpaperFrameRate::Source,
            looping: false,
            target,
        };
        let mut reference = vec![];
        decode(
            &file,
            options,
            DecodePurpose::Poster,
            &mut |frame| {
                reference.push(frame_snapshot(frame));
                true
            },
            Arc::new(Never),
        )
        .unwrap();
        assert_eq!(
            reference.len(),
            8,
            "reference must include reordered EOF frames"
        );
        let mut session =
            DecoderSession::open(&file, DecodePurpose::LiveWallpaper, Arc::new(Never)).unwrap();
        let mut allocations = None;
        for cycle in 0..3 {
            let mut actual = vec![];
            assert!(
                session
                    .cycle(
                        options,
                        DecodePurpose::LiveWallpaper,
                        &mut |frame| {
                            actual.push(frame_snapshot(frame));
                            true
                        },
                        &Never
                    )
                    .unwrap()
            );
            assert_eq!(actual, reference, "changed decoded output in cycle {cycle}");
            let current = session_allocations(&session);
            if let Some(first) = allocations {
                assert_eq!(current, first, "EOF loop replaced a live native allocation");
            } else {
                allocations = Some(current);
            }
            assert!(session.rewind(&Never));
            assert_eq!(session_allocations(&session), current);
            assert_eq!(
                session.decoder.threading().kind,
                ffmpeg::codec::threading::Type::Frame
            );
            assert_eq!(session.decoder.threading().count, 2);
        }
        // Callback retirement also works on the newly rewound decoder before
        // its remaining delayed frames have been emitted.
        let mut delivered = 0;
        assert!(
            !session
                .cycle(
                    options,
                    DecodePurpose::LiveWallpaper,
                    &mut |_| {
                        delivered += 1;
                        false
                    },
                    &Never
                )
                .unwrap()
        );
        assert_eq!(delivered, 1);
        let stopped = Stop(AtomicBool::new(true));
        assert!(!session.rewind(&stopped));
    }
    assert_eq!(storage_snapshot(&file), original);
}

/// Observe actual decoded timestamps, including unknown values. Encoded
/// negative timestamps can become missing PTS during demux; negative DTS alone
/// is not evidence of a negative presentation start.
fn decoded_timestamps(path: &Path) -> Vec<Option<i64>> {
    let mut input = ffmpeg::format::input(path).unwrap();
    let stream = input.streams().best(ffmpeg::media::Type::Video).unwrap();
    let index = stream.index();
    let mut context =
        ffmpeg::codec::context::Context::from_parameters(stream.parameters()).unwrap();
    context.set_threading(DecodePurpose::LiveWallpaper.threading(context.id()));
    let mut decoder = context.decoder().video().unwrap();
    let mut result = vec![];
    let mut receive = |decoder: &mut ffmpeg::decoder::Video| {
        loop {
            let mut frame = ffmpeg::util::frame::video::Video::empty();
            match decoder.receive_frame(&mut frame) {
                Ok(()) => result.push(frame.timestamp()),
                Err(ffmpeg::Error::Other { errno }) if errno == libc::EAGAIN => break,
                Err(ffmpeg::Error::Eof) => break,
                Err(error) => panic!("could not decode bounded timestamp fixture: {error}"),
            }
        }
    };
    loop {
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(&mut input) {
            Ok(()) if packet.stream() == index => {
                decoder.send_packet(&packet).unwrap();
                receive(&mut decoder);
            }
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => break,
            Err(error) => panic!("could not read bounded timestamp fixture: {error}"),
        }
    }
    decoder.send_eof().unwrap();
    receive(&mut decoder);
    result
}

#[test]
fn video_live_h264_positive_and_missing_initial_pts_preserve_complete_loop_output() {
    let root = Directory::new();
    ffmpeg::init().unwrap();
    for (name, extra, sign, missing_prefix) in [
        ("positive.mp4", vec!["-output_ts_offset", "2"], 1, 0),
        (
            "missing-initial-pts.mkv",
            vec![
                "-output_ts_offset",
                "-0.3",
                "-avoid_negative_ts",
                "disabled",
                "-f",
                "matroska",
            ],
            0,
            3,
        ),
    ] {
        let file = h264_fixture_with_options(&root.0, name, &extra);
        let original = storage_snapshot(&file);
        let options = PlaybackOptions {
            fps: WallpaperFrameRate::Source,
            looping: false,
            target: None,
        };
        let mut reference = vec![];
        decode(
            &file,
            options,
            DecodePurpose::Poster,
            &mut |frame| {
                reference.push(frame_snapshot(frame));
                true
            },
            Arc::new(Never),
        )
        .unwrap();
        assert_eq!(
            reference.len(),
            8,
            "offset fixture must preserve all source frames"
        );
        let mut session =
            DecoderSession::open(&file, DecodePurpose::LiveWallpaper, Arc::new(Never)).unwrap();
        let start = session.input.stream(session.index).unwrap().start_time();
        assert_eq!(
            start.signum(),
            sign,
            "muxer normalized the requested presentation start"
        );
        let raw_pts = decoded_timestamps(&file);
        assert_eq!(raw_pts.len(), 8, "timestamp fixture must fully drain");
        assert_eq!(
            raw_pts.iter().take_while(|pts| pts.is_none()).count(),
            missing_prefix
        );
        assert_eq!(raw_pts[missing_prefix].unwrap().signum(), sign);
        // The negative output offset is not exposed as negative presentation
        // timestamps by this Matroska demuxer: metadata is zero and the first
        // three actual frames have unknown PTS. Do not claim negative coverage
        // or use metadata zero to discard/reposition past that initial prefix.
        assert!(session.rewind_at.is_some());
        for cycle in 0..2 {
            let mut actual = vec![];
            assert!(
                session
                    .cycle(
                        options,
                        DecodePurpose::LiveWallpaper,
                        &mut |frame| {
                            actual.push(frame_snapshot(frame));
                            true
                        },
                        &Never
                    )
                    .unwrap()
            );
            assert_eq!(
                actual, reference,
                "offset source changed during cycle {cycle}"
            );
            if missing_prefix > 0 {
                assert!(
                    session.rewind_at.is_none(),
                    "unknown initial PTS must forbid reuse"
                );
            }
            if cycle == 0 && !session.rewind(&Never) {
                // Unknown initial PTS or an unsupported seek keeps the
                // same one-attempt/drop-before-reopen fallback as the caller;
                // the next cycle still has to match the complete source.
                drop(session);
                session =
                    DecoderSession::open(&file, DecodePurpose::LiveWallpaper, Arc::new(Never))
                        .unwrap();
            }
        }
        // Exercise the real entry as well as the owned-session boundary. The
        // first frame of cycle three proves both preceding EOFs fully drained
        // and progressed through either supported seeks or bounded reopens.
        let mut looped = vec![];
        decode(
            &file,
            PlaybackOptions {
                looping: true,
                ..options
            },
            DecodePurpose::LiveWallpaper,
            &mut |frame| {
                looped.push(frame_snapshot(frame));
                looped.len() < 17
            },
            Arc::new(Never),
        )
        .unwrap();
        assert_eq!(looped.len(), 17);
        assert_eq!(&looped[..8], reference.as_slice());
        assert_eq!(&looped[8..16], reference.as_slice());
        assert_eq!(looped[16], reference[0]);
        assert_eq!(storage_snapshot(&file), original);
    }
}

#[test]
fn video_live_session_keeps_serial_purposes_and_other_codecs_on_reopen_path() {
    let root = Directory::new();
    let h264 = h264_fixture(&root.0);
    let mpeg4 = fixture(&root.0, "mp4");
    ffmpeg::init().unwrap();
    for (path, purpose) in [
        (&h264, DecodePurpose::Poster),
        (
            &h264,
            DecodePurpose::Preview(WallpaperPreviewOptions::new(64, 48).unwrap()),
        ),
        (&mpeg4, DecodePurpose::LiveWallpaper),
    ] {
        let mut session = DecoderSession::open(path, purpose, Arc::new(Never)).unwrap();
        assert!(session.rewind_at.is_none());
        assert!(!session.rewind(&Never));
        assert_eq!(
            session.decoder.threading().kind,
            ffmpeg::codec::threading::Type::None
        );
        assert_eq!(session.decoder.threading().count, 1);
    }
}

#[test]
fn video_live_h264_final_hold_occurs_after_delayed_eof_frames() {
    struct HoldObserver {
        delivered: std::sync::atomic::AtomicUsize,
        waits: std::sync::Mutex<Vec<usize>>,
    }
    impl StopSignal for HoldObserver {
        fn cancelled(&self) -> bool {
            false
        }
        fn wait(&self, duration: Duration) {
            self.waits
                .lock()
                .unwrap()
                .push(self.delivered.load(Ordering::Relaxed));
            std::thread::sleep(duration);
        }
    }
    let root = Directory::new();
    let file = h264_fixture(&root.0);
    ffmpeg::init().unwrap();
    let stop = Arc::new(HoldObserver {
        delivered: std::sync::atomic::AtomicUsize::new(0),
        waits: std::sync::Mutex::new(vec![]),
    });
    let mut session =
        DecoderSession::open(&file, DecodePurpose::LiveWallpaper, stop.clone()).unwrap();
    assert!(
        session
            .cycle(
                PlaybackOptions {
                    fps: WallpaperFrameRate::Source,
                    looping: false,
                    target: None
                },
                DecodePurpose::LiveWallpaper,
                &mut |_| {
                    stop.delivered.fetch_add(1, Ordering::Relaxed);
                    true
                },
                stop.as_ref()
            )
            .unwrap()
    );
    assert_eq!(stop.delivered.load(Ordering::Relaxed), 8);
    assert_eq!(
        stop.waits.lock().unwrap().last(),
        Some(&8),
        "final hold was omitted or occurred before delayed EOF output"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct StoredFile {
    path: PathBuf,
    identity: (u64, u64, u32, u64),
    modified: (i64, i64),
    changed: (i64, i64),
    bytes: Vec<u8>,
    link: Option<PathBuf>,
}
fn storage_snapshot(root: &Path) -> Vec<StoredFile> {
    use std::os::unix::fs::MetadataExt;
    fn visit(path: &Path, root: &Path, files: &mut Vec<StoredFile>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        files.push(StoredFile {
            path: path.strip_prefix(root).unwrap().to_owned(),
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.mode(),
                metadata.len(),
            ),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            bytes: if metadata.is_file() {
                fs::read(path).unwrap()
            } else {
                vec![]
            },
            link: metadata.is_symlink().then(|| fs::read_link(path).unwrap()),
        });
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(&entry.unwrap().path(), root, files);
            }
        }
    }
    let mut files = vec![];
    visit(root, root, &mut files);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

#[test]
fn video_native_repeated_import_reuses_exact_owned_bytes_beyond_old_import_quota() {
    let root = Directory::new();
    let file = fixture(&root.0, "mp4");
    let source = Wallpaper {
        path: file.to_string_lossy().into(),
        name: "Current fixture".into(),
    };
    let adapter = NativeVideoWallpapers {
        state: root.0.join("store"),
        library: root.0.clone(),
    };
    let asset = adapter.prepare(&source).unwrap();
    // Historical assets remain valid and immutable, with distinct descriptors.
    // Lookup must not confuse the library's bounded result count with a quota.
    for number in 0..40 {
        let dir = adapter.state.join(format!("media-history-{number}"));
        fs::create_dir(&dir).unwrap();
        let mut historical = asset.clone();
        historical.source.name = format!("Historical {number}");
        historical.path = dir.join("source.mp4").to_string_lossy().into();
        historical.poster.path = dir.join("poster.png").to_string_lossy().into();
        fs::copy(&asset.path, &historical.path).unwrap();
        fs::copy(&asset.poster.path, &historical.poster.path).unwrap();
        fs::write(
            dir.join("asset.json"),
            serde_json::to_vec(&historical).unwrap(),
        )
        .unwrap();
        adapter.owned(&historical).unwrap();
    }
    let before = storage_snapshot(&adapter.state);
    let original_source = storage_snapshot(&file);
    for _ in 0..3 {
        assert_eq!(adapter.prepare(&source).unwrap(), asset);
        assert_eq!(storage_snapshot(&adapter.state), before);
        assert_eq!(storage_snapshot(&file), original_source);
    }
    assert_eq!(fs::read_dir(&adapter.state).unwrap().count(), 41);
}

#[test]
fn video_native_changed_source_bytes_create_new_immutable_asset_without_old_writes() {
    let root = Directory::new();
    let file = fixture(&root.0, "mp4");
    // A legal ignored ISO-BMFF free box makes comparison span multiple chunks.
    // Its last byte changes without changing path, name, size or video pixels.
    let mut original = fs::read(&file).unwrap();
    let padding = 2 * 64 * 1024 + 8;
    original.extend_from_slice(&(padding as u32).to_be_bytes());
    original.extend_from_slice(b"free");
    original.resize(original.len() + padding - 8, 0);
    *original.last_mut().unwrap() = b'A';
    fs::write(&file, &original).unwrap();
    let source = Wallpaper {
        path: file.to_string_lossy().into(),
        name: "Same descriptor".into(),
    };
    let adapter = NativeVideoWallpapers {
        state: root.0.join("store"),
        library: root.0.clone(),
    };
    let first = adapter.prepare(&source).unwrap();
    let first_dir = Path::new(&first.path).parent().unwrap();
    let first_snapshot = storage_snapshot(first_dir);
    let mut changed = original.clone();
    *changed.last_mut().unwrap() = b'B';
    fs::write(&file, &changed).unwrap();
    let second = adapter.prepare(&source).unwrap();
    assert_ne!(second.path, first.path);
    assert_eq!(fs::read(&second.path).unwrap(), changed);
    assert_eq!(fs::read(&first.path).unwrap(), original);
    assert_eq!(storage_snapshot(first_dir), first_snapshot);
    let stable = storage_snapshot(&adapter.state);
    assert_eq!(adapter.prepare(&source).unwrap(), second);
    assert_eq!(storage_snapshot(&adapter.state), stable);
    // Equal bytes alone are insufficient when the source descriptor changes.
    let renamed = Wallpaper {
        name: "Different descriptor".into(),
        ..source
    };
    let third = adapter.prepare(&renamed).unwrap();
    assert_ne!(third.path, second.path);
    assert_eq!(third.source, renamed);
    assert_eq!(storage_snapshot(first_dir), first_snapshot);
}

#[test]
fn video_native_invalid_foreign_and_symlink_records_are_not_reused_or_rewritten() {
    let root = Directory::new();
    let file = fixture(&root.0, "mp4");
    let source = Wallpaper {
        path: file.to_string_lossy().into(),
        name: "Fixture".into(),
    };
    for case in ["invalid", "foreign", "symlink", "poster", "writable"] {
        let adapter = NativeVideoWallpapers {
            state: root.0.join(case),
            library: root.0.clone(),
        };
        let first = adapter.prepare(&source).unwrap();
        let dir = Path::new(&first.path).parent().unwrap();
        let metadata = dir.join("asset.json");
        let outside = root.0.join(format!("{case}-outside.json"));
        match case {
            "invalid" => fs::write(&metadata, b"invalid metadata").unwrap(),
            "foreign" => {
                let mut foreign = first.clone();
                foreign.path = source.path.clone();
                fs::write(&metadata, serde_json::to_vec(&foreign).unwrap()).unwrap();
            }
            "symlink" => {
                fs::rename(&metadata, &outside).unwrap();
                std::os::unix::fs::symlink(&outside, &metadata).unwrap();
                assert!(adapter.owned(&first).is_err());
            }
            "poster" => {
                fs::rename(&first.poster.path, &outside).unwrap();
                std::os::unix::fs::symlink(&outside, &first.poster.path).unwrap();
            }
            "writable" => {
                fs::set_permissions(&first.path, fs::Permissions::from_mode(0o600)).unwrap()
            }
            _ => unreachable!(),
        }
        let before = storage_snapshot(dir);
        let outside_before = outside.exists().then(|| storage_snapshot(&outside));
        let second = adapter.prepare(&source).unwrap();
        assert_ne!(second.path, first.path, "reused {case} record");
        adapter.owned(&second).unwrap();
        assert_eq!(storage_snapshot(dir), before, "rewrote {case} record");
        assert_eq!(
            outside.exists().then(|| storage_snapshot(&outside)),
            outside_before
        );
    }
}

#[test]
fn video_native_import_still_succeeds_after_bounded_historical_lookup_misses() {
    let root = Directory::new();
    let file = fixture(&root.0, "mp4");
    let adapter = NativeVideoWallpapers {
        state: root.0.join("store"),
        library: root.0.clone(),
    };
    fs::create_dir(&adapter.state).unwrap();
    for number in 0..1025 {
        let dir = adapter.state.join(format!("media-history-{number}"));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("keep"), b"unrelated historical data").unwrap();
    }
    let historical = fs::read_dir(&adapter.state)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let snapshot = storage_snapshot(&path);
            (path, snapshot)
        })
        .collect::<Vec<_>>();
    let asset = adapter
        .prepare(&Wallpaper {
            path: file.to_string_lossy().into(),
            name: "Fresh import".into(),
        })
        .unwrap();
    adapter.owned(&asset).unwrap();
    assert_eq!(fs::read_dir(&adapter.state).unwrap().count(), 1026);
    for (path, before) in historical {
        assert_eq!(storage_snapshot(&path), before);
    }
}

#[test]
fn video_import_rejects_symlink_fifo_and_replaced_sources_before_storage_mutation() {
    use std::os::unix::ffi::OsStrExt;
    let root = Directory::new();
    let original = root.0.join("original.mp4");
    fs::write(&original, b"regular source").unwrap();
    let link = root.0.join("linked.mp4");
    std::os::unix::fs::symlink(&original, &link).unwrap();
    let fifo = root.0.join("fifo.mp4");
    let fifo_name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: the CString is valid for this call; the FIFO is in our owned
    // temporary directory and has no readers or writers.
    assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);
    let adapter = NativeVideoWallpapers {
        state: root.0.join("store"),
        library: root.0.clone(),
    };
    for path in [&link, &fifo] {
        assert!(
            adapter
                .prepare(&Wallpaper {
                    path: path.to_string_lossy().into(),
                    name: "Rejected source".into(),
                })
                .is_err()
        );
        assert!(!adapter.state.exists());
    }
    let mut source = store::Source::open(&original).unwrap();
    fs::rename(&original, root.0.join("old.mp4")).unwrap();
    fs::write(&original, b"replacement bytes").unwrap();
    let mut out = fs::File::create(root.0.join("copy")).unwrap();
    assert!(source.copy_to(&mut out).is_err());
    assert_eq!(out.metadata().unwrap().len(), 0);
    assert_eq!(fs::read(&original).unwrap(), b"replacement bytes");
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
fn video_native_conversion_waits_for_decoded_format_when_codec_metadata_is_unknown() {
    let root = Directory::new();
    let file = fixture(&root.0, "mp4");
    let mut input = ffmpeg::format::input(&file).unwrap();
    let stream = input.streams().best(ffmpeg::media::Type::Video).unwrap();
    let index = stream.index();
    let mut parameters = stream.parameters().clone();
    // SAFETY: This is an owned copy, changed before creating/opening a decoder.
    // AVCodecParameters::format explicitly permits -1 for unknown pixel format.
    unsafe {
        (*parameters.as_mut_ptr()).format = -1;
    }
    let context = ffmpeg::codec::context::Context::from_parameters(parameters).unwrap();
    let mut decoder = context.decoder().video().unwrap();
    assert_eq!(decoder.format(), ffmpeg::format::Pixel::None);
    let mut converted = false;
    for (stream, packet) in input.packets() {
        if stream.index() != index {
            continue;
        }
        decoder.send_packet(&packet).unwrap();
        let mut frame = ffmpeg::util::frame::video::Video::empty();
        if decoder.receive_frame(&mut frame).is_ok() {
            assert_ne!(frame.format(), ffmpeg::format::Pixel::None);
            let mut converter =
                FrameConverter::new(&frame, Some(WallpaperPixelTarget::new(9, 5).unwrap()))
                    .unwrap();
            let output = converter.convert(&frame, Duration::ZERO).unwrap();
            assert_eq!(output.dimensions(), (9, 7));
            assert_eq!(output.aspect_size(), (16, 12));
            converted = true;
            break;
        }
    }
    assert!(
        converted,
        "unknown metadata must not prevent decoding the first frame"
    );
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
