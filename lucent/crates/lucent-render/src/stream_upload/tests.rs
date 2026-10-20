use super::*;
use crate::{Canvas, Density, Paint, Rect, ResourceProfile};
use lucent_api::{Color, ImageStream};
use std::{sync::Arc, time::Duration};

#[test]
fn upload_layout_accounts_for_odd_rows_limits_and_host_slices() {
    for (width, height, stride, bytes) in [
        (64, 1, 256, 256),
        (65, 3, 512, 1536),
        (1920, 1080, 7680, 8294400),
    ] {
        let pixels = width as usize * height as usize * 4;
        let layout = Layout::rgba(width, height, pixels, bytes).unwrap();
        assert_eq!((layout.stride, layout.bytes), (stride, bytes));
        assert!(Layout::rgba(width, height, pixels, bytes - 1).is_none());
    }
    assert!(Layout::rgba(0, 1, 0, u64::MAX).is_none());
    assert!(Layout::rgba(1, 0, 0, u64::MAX).is_none());
    assert!(Layout::rgba(u32::MAX, 1, 4, u64::MAX).is_none());
    assert!(Layout::rgba(u32::MAX / 4, 1, 4, u64::MAX).is_none());
    assert!(Layout::rgba(65, 3, 65 * 3 * 4 - 1, u64::MAX).is_none());
    let full_hd = Layout::rgba(1920, 1080, 1920 * 1080 * 4, u64::MAX).unwrap();
    assert_eq!(2 * full_hd.bytes, 16588800);
    assert!(2 * full_hd.bytes <= PAYLOAD_BUDGET);
    assert!(3 * full_hd.bytes > PAYLOAD_BUDGET);
}

#[test]
fn upload_row_copy_preserves_rgba_and_does_not_copy_padding_as_pixels() {
    let pixels: Vec<_> = (0..65 * 3 * 4).map(|index| (index % 251) as u8).collect();
    let layout = Layout::rgba(65, 3, pixels.len(), 1536).unwrap();
    let mut padded = vec![0xab; layout.bytes as usize];
    layout.write(&pixels, |range, source| {
        padded[range].copy_from_slice(source);
    });
    for row in 0..3 {
        assert_eq!(
            &padded[row * 512..row * 512 + 260],
            &pixels[row * 260..(row + 1) * 260]
        );
        assert!(
            padded[row * 512 + 260..(row + 1) * 512]
                .iter()
                .all(|value| *value == 0xab)
        );
    }
    let tight = Layout::rgba(64, 1, 256, 256).unwrap();
    let mut destination = vec![0; 256];
    tight.write(&pixels[..256], |range, source| {
        destination[range].copy_from_slice(source);
    });
    assert_eq!(destination, pixels[..256]);
}

fn image(data: Arc<ImageData>, rect: Rect) -> Paint {
    Paint::Image {
        rect,
        clip: rect.into(),
        data,
        uv: Rect::new(0., 0., 1., 1.),
        radius: 0.,
        opacity: 1.,
        tint: Color::hex(0xffffff),
    }
}
fn gpu() -> Rc<Gpu> {
    let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
    eprintln!("stream upload test adapter: {}", gpu.adapter_description);
    gpu
}
fn solid(stream: &ImageStream, width: u32, height: u32, color: [u8; 4]) -> Arc<ImageData> {
    Arc::new(
        stream
            .frame_rgba(
                width,
                height,
                color.repeat(width as usize * height as usize),
            )
            .unwrap(),
    )
}
fn wait(gpu: &Gpu) {
    gpu.device
        .poll(PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(5)),
        })
        .unwrap();
}
fn assert_solid(pixels: &[u8], color: [u8; 4]) {
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == color)
    );
}

#[test]
#[ignore = "requires a Vulkan driver"]
fn stream_upload_odd_rows_alpha_and_density_match_queue_fallback_pixels() {
    let gpu = gpu();
    let mut pooled = Canvas::new(gpu.clone());
    let mut reference = Canvas::new(gpu.clone());
    let stream = ImageStream::new();
    let reference_stream = ImageStream::new();
    let rect = Rect::new(0., 0., 65., 3.);
    let densities = [
        Density::integer(1).unwrap(),
        Density::integer(2).unwrap(),
        Density::new(150, 120).unwrap(),
    ];
    for density in densities {
        for phase in 0..3 {
            let mut pixels = vec![];
            for y in 0..3 {
                for x in 0..65 {
                    pixels.extend_from_slice(&[
                        (x * 3) as u8,
                        (y * 73) as u8,
                        (phase * 80) as u8,
                        if x % 3 == 0 { 128 } else { 255 },
                    ]);
                }
            }
            let data = Arc::new(stream.frame_rgba(65, 3, pixels.clone()).unwrap());
            let expected = Arc::new(reference_stream.frame_rgba(65, 3, pixels).unwrap());
            let actual = pooled
                .snapshot_scaled(65, 3, density, &[image(data, rect)])
                .unwrap();
            gpu.stream_uploads.borrow_mut().disabled = true;
            let baseline = reference
                .snapshot_scaled(65, 3, density, &[image(expected, rect)])
                .unwrap();
            gpu.stream_uploads.borrow_mut().disabled = false;
            assert_eq!(
                actual, baseline,
                "odd-row alpha pixels changed at density {density:?}"
            );
        }
    }
    let pool = gpu.stream_uploads.borrow();
    assert!(pool.counters.copies > 0 && pool.counters.fallbacks > 0);
    assert!(pool.slots.len() <= MAX_SLOTS && pool.owned_bytes() <= PAYLOAD_BUDGET);
    assert!(pool.counters.allocations <= MAX_SLOTS as u64);
}

#[test]
#[ignore = "requires a Vulkan driver"]
fn stream_upload_last_revision_survives_mixed_pool_and_queue_paths() {
    let gpu = gpu();
    let mut canvas = Canvas::new(gpu.clone());
    canvas.stream_tracing = true;
    let streams = [ImageStream::new(), ImageStream::new(), ImageStream::new()];
    let rect = Rect::new(0., 0., 8., 2.);
    let cold: Vec<_> = streams
        .iter()
        .map(|stream| image(solid(stream, 2, 1, [255, 0, 0, 255]), rect))
        .collect();
    canvas.snapshot(8, 2, 1, &cold).unwrap();
    let earlier = solid(&streams[0], 2, 1, [0, 255, 0, 255]);
    let final_frame = solid(&streams[0], 2, 1, [255, 255, 0, 255]);
    let b = solid(&streams[1], 2, 1, [255, 0, 255, 255]);
    let c = solid(&streams[2], 2, 1, [0, 255, 255, 255]);
    let scene = [
        image(earlier, Rect::new(0., 0., 2., 2.)),
        image(b, Rect::new(2., 0., 2., 2.)),
        image(c, Rect::new(4., 0., 2., 2.)),
        image(final_frame.clone(), Rect::new(6., 0., 2., 2.)),
    ];
    let pixels = canvas.snapshot(8, 2, 1, &scene).unwrap();
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        assert_eq!(
            *pixel,
            match index % 8 {
                0..=1 | 6..=7 => [255, 255, 0, 255],
                2..=3 => [255, 0, 255, 255],
                _ => [0, 255, 255, 255],
            }
        );
    }
    let pool = gpu.stream_uploads.borrow();
    assert_eq!(pool.counters.copies, 2);
    assert_eq!(pool.counters.fallbacks, 1);
    assert_eq!(pool.slots.len(), 2);
    assert_eq!(
        canvas
            .last_stream_trace
            .unwrap()
            .revisions()
            .iter()
            .filter(|(id, _)| *id == final_frame.stream_revision().unwrap().0)
            .copied()
            .collect::<Vec<_>>(),
        [final_frame.stream_revision().unwrap()]
    );
}

#[test]
#[ignore = "requires a Vulkan driver"]
fn stream_upload_abandoned_prepare_keeps_revision_lifetime_and_pixels() {
    let gpu = gpu();
    let mut canvas = Canvas::new(gpu.clone());
    canvas.profiling = true;
    let stream = ImageStream::new();
    let rect = Rect::new(0., 0., 8., 4.);
    let first = solid(&stream, 2, 1, [255, 0, 0, 255]);
    canvas
        .snapshot(8, 4, 1, &[image(first.clone(), rect)])
        .unwrap();
    let newer = solid(&stream, 2, 1, [0, 255, 0, 255]);
    let mut pending = StreamUpdates::default();
    let mut profile = ResourceProfile::default();
    let texture = gpu.image(&newer, &mut profile, true, &mut pending);
    assert_eq!(texture.revision.get(), first.stream_revision().unwrap().1);
    assert_eq!(
        pending.revision(&texture),
        newer.stream_revision().unwrap().1
    );
    assert!(texture.image_lifetime.borrow().as_ref().unwrap().is_alive());
    drop(pending);
    assert_eq!(texture.revision.get(), first.stream_revision().unwrap().1);
    assert_solid(
        &canvas.snapshot(8, 4, 1, &[image(first, rect)]).unwrap(),
        [255, 0, 0, 255],
    );
    assert_eq!(
        canvas.last_profile.unwrap().resources.texture_upload_bytes,
        0
    );
    assert_solid(
        &canvas.snapshot(8, 4, 1, &[image(newer, rect)]).unwrap(),
        [0, 255, 0, 255],
    );
    assert_eq!(
        canvas.last_profile.unwrap().resources.texture_upload_bytes,
        8
    );
}

#[test]
#[ignore = "requires a Vulkan driver"]
fn stream_upload_abandoned_encoder_and_stale_mapping_preserve_owned_buffers() {
    let gpu = gpu();
    let mut canvas = Canvas::new(gpu.clone());
    let stream = ImageStream::new();
    let rect = Rect::new(0., 0., 8., 4.);
    let first = solid(&stream, 2, 1, [255, 0, 0, 255]);
    canvas
        .snapshot(8, 4, 1, &[image(first.clone(), rect)])
        .unwrap();
    let newer = solid(&stream, 2, 1, [0, 255, 0, 255]);
    let mut pending = StreamUpdates::default();
    let mut profile = ResourceProfile::default();
    let texture = gpu.image(&newer, &mut profile, true, &mut pending);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let key;
    {
        let mut pool = gpu.stream_uploads.borrow_mut();
        let mut upload = pool.begin(&gpu.device, true);
        assert_eq!(pending.encode(&gpu, &mut upload, &mut encoder), 8);
        key = upload.staged[0].unwrap();
        drop(encoder); // Its deferred map must never run without a submission.
        drop(upload); // Recover exactly this unmapped, unsubmitted buffer.
        assert_eq!(pool.slots[0].phase, Phase::Pending(key));
        pool.complete(Completion {
            key: Key {
                id: key.id,
                generation: key.generation + 1,
            },
            success: true,
        });
        assert_eq!(pool.slots[0].phase, Phase::Pending(key));
    }
    drop(pending);
    wait(&gpu);
    {
        let mut pool = gpu.stream_uploads.borrow_mut();
        pool.drain();
        assert_eq!(pool.slots[0].phase, Phase::Mapped);
        assert_eq!(pool.counters.allocations, 1);
    }
    assert_eq!(texture.revision.get(), first.stream_revision().unwrap().1);
    assert_solid(
        &canvas.snapshot(8, 4, 1, &[image(first, rect)]).unwrap(),
        [255, 0, 0, 255],
    );
    assert_solid(
        &canvas.snapshot(8, 4, 1, &[image(newer, rect)]).unwrap(),
        [0, 255, 0, 255],
    );
    let mut pool = gpu.stream_uploads.borrow_mut();
    pool.drain();
    assert_eq!(pool.counters.allocations, 1);
    let current = Key {
        id: pool.slots[0].id,
        generation: pool.slots[0].generation,
    };
    // Injecting a failed completion quarantines its lifetime; a late/duplicate
    // success cannot turn that failed native mapping into usable CPU storage.
    pool.slots[0].phase = Phase::Pending(current);
    pool.complete(Completion {
        key: current,
        success: false,
    });
    pool.complete(Completion {
        key: current,
        success: true,
    });
    assert_eq!(pool.slots[0].phase, Phase::Failed);
    assert!(pool.disabled);
    assert_eq!(pool.counters.allocations, 1);
}

#[test]
#[ignore = "requires a Vulkan driver"]
fn stream_upload_resize_retains_old_views_and_obeys_global_payload_budget() {
    let gpu = gpu();
    let mut canvas = Canvas::new(gpu.clone());
    canvas.stream_tracing = true;
    gpu.stream_uploads.borrow_mut().budget = 3072;
    let stream = ImageStream::new();
    let left = Rect::new(0., 0., 4., 4.);
    let right = Rect::new(4., 0., 4., 4.);
    let first = solid(&stream, 65, 3, [255, 0, 0, 255]);
    canvas.snapshot(8, 4, 1, &[image(first, left)]).unwrap();
    let old_view = solid(&stream, 65, 3, [0, 255, 0, 255]);
    let resized = solid(&stream, 129, 5, [0, 0, 255, 255]);
    let pixels = canvas
        .snapshot(
            8,
            4,
            1,
            &[image(old_view.clone(), left), image(resized.clone(), right)],
        )
        .unwrap();
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        assert_eq!(
            *pixel,
            if index % 8 < 4 {
                [0, 255, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        );
    }
    let trace = canvas.last_stream_trace.unwrap();
    assert!(
        trace
            .revisions()
            .contains(&old_view.stream_revision().unwrap())
    );
    assert!(
        trace
            .revisions()
            .contains(&resized.stream_revision().unwrap())
    );
    let oversized = solid(&stream, 129, 5, [255, 255, 0, 255]);
    assert_solid(
        &canvas
            .snapshot(8, 4, 1, &[image(oversized, Rect::new(0., 0., 8., 4.))])
            .unwrap(),
        [255, 255, 0, 255],
    );
    let pool = gpu.stream_uploads.borrow();
    assert!(pool.counters.fallbacks > 0);
    assert!(pool.counters.peak_owned_bytes <= 3072);
    assert!(pool.owned_bytes() <= 3072 && pool.slots.len() <= MAX_SLOTS);
}

#[test]
#[ignore = "requires Vulkan; CPU upload diagnostic, not native FPS or RSS acceptance"]
fn stream_upload_cpu_stage_diagnostic_1080p() {
    let gpu = gpu();
    let mut canvas = Canvas::new(gpu.clone());
    canvas.profiling = true;
    let stream = ImageStream::new();
    let rect = Rect::new(0., 0., 64., 36.);
    let colors = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
    let frames: Vec<_> = colors
        .iter()
        .map(|color| solid(&stream, 1920, 1080, *color))
        .collect();
    for pooled in [false, true] {
        gpu.stream_uploads.borrow_mut().disabled = !pooled;
        let mut samples = vec![];
        for index in 0..24 {
            let paint = [image(frames[index % frames.len()].clone(), rect)];
            let pixels = canvas.snapshot(64, 36, 1, &paint).unwrap();
            assert_solid(&pixels, colors[index % colors.len()]);
            if index >= 4 {
                samples.push(
                    canvas
                        .last_profile
                        .unwrap()
                        .resources
                        .texture_upload
                        .as_secs_f64()
                        * 1000.,
                );
            }
        }
        samples.sort_by(f64::total_cmp);
        let pool = gpu.stream_uploads.borrow();
        eprintln!(
            "stream upload diagnostic: pooled={pooled} samples={} mean_ms={} p95_ms={} allocations={} copies={} fallbacks={} owned_payload_bytes={} peak_owned_payload_bytes={}; offscreen/readback-synchronized CPU wall cost only",
            samples.len(),
            samples.iter().sum::<f64>() / samples.len() as f64,
            samples[18],
            pool.counters.allocations,
            pool.counters.copies,
            pool.counters.fallbacks,
            pool.owned_bytes(),
            pool.counters.peak_owned_bytes
        );
        assert!(pool.slots.len() <= MAX_SLOTS && pool.owned_bytes() <= PAYLOAD_BUDGET);
    }
}
