use super::*;
use lucent_api::video_frame::{Bt709, Bytes, ChromaSiting, Plane, Range, Transfer};

fn frame(
    width: u32,
    height: u32,
    samples: [Vec<u8>; 3],
    color: Bt709,
    padding: u32,
) -> Yuv420Frame {
    let planes = samples
        .into_iter()
        .enumerate()
        .map(|(index, samples)| {
            let (w, h) = if index == 0 {
                (width, height)
            } else {
                (width.div_ceil(2), height.div_ceil(2))
            };
            assert_eq!(samples.len(), (w * h) as usize);
            let mut stored = Vec::new();
            for (row, values) in samples.chunks_exact(w as usize).enumerate() {
                stored.extend_from_slice(values);
                if row + 1 < h as usize {
                    stored.extend(std::iter::repeat_n(255, padding as usize));
                }
            }
            Plane::new(Bytes::from(stored), w, h, w + padding).unwrap()
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    Yuv420Frame::new(width, height, planes, color).unwrap()
}
fn color(range: Range, transfer: Transfer, chroma: ChromaSiting) -> Bt709 {
    Bt709 {
        range,
        transfer,
        chroma,
    }
}
fn painter() -> PlanarPainter {
    PlanarPainter::new(Gpu::headless(Arc::new(Vec::new())).unwrap())
}

// Independent f64 oracle: matrix is derived from Kr/Kb instead of copying shader
// coefficients. Spatial interpolation sums four explicit weights and code values.
fn reference(frame: &Yuv420Frame, x: u32, y: u32, width: u32, height: u32) -> [u8; 4] {
    let uv = [
        (x as f64 + 0.5) / width as f64,
        (y as f64 + 0.5) / height as f64,
    ];
    let (w, h) = frame.dimensions();
    let luma = [uv[0] * w as f64 - 0.5, uv[1] * h as f64 - 0.5];
    let offset = if frame.color().chroma == ChromaSiting::Left {
        [0., 0.5]
    } else {
        [0.5, 0.5]
    };
    let chroma = [(luma[0] - offset[0]) / 2., (luma[1] - offset[1]) / 2.];
    let sample = |plane: &Plane, at: [f64; 2]| {
        let (w, h) = plane.dimensions();
        let x = at[0].clamp(0., w as f64 - 1.);
        let y = at[1].clamp(0., h as f64 - 1.);
        let (left, top) = (x.floor() as u32, y.floor() as u32);
        let (right, bottom) = ((left + 1).min(w - 1), (top + 1).min(h - 1));
        let (fx, fy) = (x.fract(), y.fract());
        [
            (left, top, (1. - fx) * (1. - fy)),
            (right, top, fx * (1. - fy)),
            (left, bottom, (1. - fx) * fy),
            (right, bottom, fx * fy),
        ]
        .into_iter()
        .map(|(x, y, weight)| f64::from(plane.row(y).unwrap()[x as usize]) * weight)
        .sum::<f64>()
    };
    let y = sample(&frame.planes()[0], luma);
    let cb = sample(&frame.planes()[1], chroma) - 128.;
    let cr = sample(&frame.planes()[2], chroma) - 128.;
    let (y, cb, cr) = if frame.color().range == Range::Limited {
        ((y - 16.) / 219., cb / 224., cr / 224.)
    } else {
        (y / 255., cb / 255., cr / 255.)
    };
    let (kr, kb) = (0.2126, 0.0722);
    let kg = 1. - kr - kb;
    let red = y + 2. * (1. - kr) * cr;
    let blue = y + 2. * (1. - kb) * cb;
    let green = (y - kr * red - kb * blue) / kg;
    let rgb = [red, green, blue].map(|signal: f64| {
        let signal = signal.clamp(0., 1.);
        let encoded = if frame.color().transfer == Transfer::Srgb {
            signal
        } else {
            let alpha = 1.099296826809442;
            let beta = 0.018053968510807;
            let linear = if signal < 4.5 * beta {
                signal / 4.5
            } else {
                ((signal + alpha - 1.) / alpha).powf(1. / 0.45)
            };
            if linear <= 0.0031308 {
                12.92 * linear
            } else {
                1.055 * linear.powf(1. / 2.4) - 0.055
            }
        };
        (encoded * 255.).round() as u8
    });
    [rgb[0], rgb[1], rgb[2], 255]
}
fn assert_pixels(frame: &Yuv420Frame, actual: &[u8], width: u32, height: u32) {
    assert_eq!(actual.len(), (width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let expected = reference(frame, x, y, width, height);
            let begin = ((y * width + x) * 4) as usize;
            for channel in 0..4 {
                assert!(
                    actual[begin + channel].abs_diff(expected[channel]) <= 1,
                    "source {:?}, output {width}x{height}, {:?}, pixel ({x},{y}) channel {channel}: actual {:?}, independent expected {expected:?}",
                    frame.dimensions(),
                    frame.color(),
                    &actual[begin..begin + 4]
                );
            }
        }
    }
}

#[test]
#[ignore = "requires Vulkan; run explicitly with --ignored"]
fn video_planes_gpu_matrix_range_transfer_and_opaque_output_match_independent_vectors() {
    let mut painter = painter();
    for range in [Range::Full, Range::Limited] {
        for transfer in [Transfer::Srgb, Transfer::Bt709] {
            for (y, u, v) in [
                (0, 128, 128),
                (16, 128, 128),
                (235, 128, 128),
                (255, 128, 128),
                (45, 128, 128),
                (128, 128, 128),
                (63, 102, 240),
                (173, 42, 26),
                (32, 240, 118),
            ] {
                let frame = frame(
                    2,
                    2,
                    [vec![y; 4], vec![u], vec![v]],
                    color(range, transfer, ChromaSiting::Left),
                    0,
                );
                let pixels = painter.snapshot(&frame, 4, 4).unwrap();
                assert_pixels(&frame, &pixels, 4, 4);
            }
        }
    }
    assert_eq!(painter.stats().texture_sets_created, 1);
    assert_eq!(painter.stats().frames_uploaded, 36);
}
#[test]
#[ignore = "requires Vulkan; run explicitly with --ignored"]
fn video_planes_gpu_odd_edges_strides_and_chroma_siting_match_independent_resampling() {
    let mut painter = painter();
    for siting in [ChromaSiting::Left, ChromaSiting::Center] {
        let frame = frame(
            5,
            3,
            [
                vec![
                    64, 85, 106, 127, 148, 79, 100, 121, 142, 163, 94, 115, 136, 157, 178,
                ],
                vec![64, 128, 192, 96, 160, 224],
                vec![192, 128, 64, 224, 160, 96],
            ],
            color(Range::Limited, Transfer::Bt709, siting),
            3,
        );
        for (w, h) in [(5, 3), (13, 7), (3, 2)] {
            let pixels = painter.snapshot(&frame, w, h).unwrap();
            assert_pixels(&frame, &pixels, w, h);
        }
    }
    assert_eq!(painter.stats().texture_sets_created, 1);
    assert_eq!(painter.stats().texture_payload_bytes, 27);
    assert_eq!(painter.stats().bytes_uploaded, 27 * 6);
}
#[test]
#[ignore = "requires Vulkan; run explicitly with --ignored"]
fn video_planes_gpu_same_extent_reuses_resources_and_replacement_drops_old_pixels() {
    let mut painter = painter();
    let c = color(Range::Full, Transfer::Srgb, ChromaSiting::Center);
    for y in [32, 224, 128] {
        let frame = frame(2, 2, [vec![y; 4], vec![128], vec![128]], c, 0);
        let pixels = painter.snapshot(&frame, 3, 3).unwrap();
        assert_pixels(&frame, &pixels, 3, 3);
    }
    assert_eq!(painter.stats().texture_sets_created, 1);
    let frame = frame(5, 1, [vec![180; 5], vec![128; 3], vec![128; 3]], c, 2);
    let pixels = painter.snapshot(&frame, 5, 1).unwrap();
    assert_pixels(&frame, &pixels, 5, 1);
    assert_eq!(painter.stats().texture_sets_created, 2);
    assert_eq!(painter.stats().texture_payload_bytes, 11);
    assert!(painter.snapshot(&frame, 0, 1).is_err());
    assert_eq!(
        painter.stats().frames_uploaded,
        4,
        "rejected output must not upload"
    );
}
