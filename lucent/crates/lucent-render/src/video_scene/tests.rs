use super::*;
use lucent_api::{
    Clip, Color, Element, ImageData, ImageStream,
    video_frame::{Bt709, Bytes, ChromaSiting, Plane, Range, Transfer, Yuv420Frame},
};
use lucent_ui::{Interaction, Layout};
fn native(stream: &ImageStream, grey: u8) -> Arc<VideoData> {
    let y = Plane::new(Bytes::from(vec![grey; 4]), 2, 2, 2).unwrap();
    let uv = Plane::new(Bytes::from(vec![128]), 1, 1, 1).unwrap();
    Arc::new(
        stream
            .frame_yuv420(
                Yuv420Frame::new(
                    2,
                    2,
                    [y, uv.clone(), uv],
                    Bt709 {
                        range: Range::Full,
                        transfer: Transfer::Srgb,
                        chroma: ChromaSiting::Left,
                    },
                )
                .unwrap(),
            )
            .unwrap(),
    )
}
fn video(data: Arc<VideoData>, rect: Rect) -> Paint {
    Paint::Video {
        rect,
        uv: Rect::new(0., 0., 1., 1.),
        clip: rect.into(),
        data,
        radius: 0.,
        opacity: 1.,
        tint: Color::hex(0xffffff),
    }
}
fn image(data: Arc<ImageData>, rect: Rect) -> Paint {
    Paint::Image {
        rect,
        uv: Rect::new(0., 0., 1., 1.),
        clip: rect.into(),
        data,
        radius: 0.,
        opacity: 1.,
        tint: Color::hex(0xffffff),
    }
}
#[test]
#[ignore = "requires Vulkan; run explicitly with --ignored"]
fn video_scene_gpu_element_uses_existing_clip_tint_opacity_and_fractional_density() {
    let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
    let mut canvas = Canvas::new(gpu);
    let data = native(&ImageStream::new(), 128);
    let rgba = Arc::new(
        ImageData::from_rgba("opaque comparison", 2, 2, [128, 128, 128, 255].repeat(4)).unwrap(),
    );
    let layout = Layout::new(Arc::new(vec![]));
    let tree = |node: Element<()>| {
        Element::stack(vec![
            node.cover()
                .fill()
                .radius(4.)
                .opacity(0.7)
                .tint(Color::hex(0x80a0c0).alpha(0.6))
                .diagonal_reveal(0.55),
        ])
        .fill()
        .padding(3.)
        .radius(6.)
        .clip()
        .background(Color::hex(0x102030))
    };
    let native = layout.build(
        &tree(Element::video(data)),
        32.,
        24.,
        &Interaction::default(),
        0.,
    );
    let rgba = layout.build(
        &tree(Element::image(rgba)),
        32.,
        24.,
        &Interaction::default(),
        0.,
    );
    for density in [
        Density::integer(1).unwrap(),
        Density::new(5, 4).unwrap(),
        Density::integer(2).unwrap(),
    ] {
        let expected = canvas
            .snapshot_scaled(32, 24, density, &rgba.paint)
            .unwrap();
        let actual = canvas
            .snapshot_scaled(32, 24, density, &native.paint)
            .unwrap();
        assert_eq!(
            actual, expected,
            "shared geometry/coverage must preserve actual pixels at {density:?}"
        );
    }
    assert_eq!(canvas.gpu.cache_stats().budget_bytes, 64 * 1024 * 1024);
}
#[test]
#[ignore = "requires Vulkan; run explicitly with --ignored"]
fn video_scene_gpu_frozen_planes_survive_live_updates_and_rgba_format_fallback() {
    let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
    let mut canvas = Canvas::new(gpu.clone());
    canvas.profiling = true;
    canvas.stream_tracing = true;
    let stream = ImageStream::new();
    let first = native(&stream, 32);
    let frozen = Arc::new(first.snapshot());
    let frozen_id = frozen.cache_id();
    let id = first.stream_revision().unwrap().0;
    let left = Rect::new(0., 0., 4., 4.);
    let right = Rect::new(4., 0., 4., 4.);
    for grey in [128, 224] {
        let current = native(&stream, grey);
        let revision = current.stream_revision().unwrap().1;
        let pixels = canvas
            .snapshot(
                8,
                4,
                1,
                &[video(frozen.clone(), left), video(current, right)],
            )
            .unwrap();
        for y in 0..4 {
            for x in 0..8 {
                let at = ((y * 8 + x) * 4) as usize;
                let expected = if x < 4 { 32 } else { grey };
                assert_eq!(&pixels[at..at + 4], &[expected, expected, expected, 255]);
            }
        }
        assert_eq!(
            canvas.last_stream_trace.unwrap().revisions(),
            &[(id, revision)]
        );
    }
    let fallback = Arc::new(stream.frame_rgba(2, 2, [255, 0, 0, 255].repeat(4)).unwrap());
    let revision = fallback.stream_revision().unwrap().1;
    let pixels = canvas
        .snapshot(
            8,
            4,
            1,
            &[video(frozen.clone(), left), image(fallback, right)],
        )
        .unwrap();
    assert_eq!(&pixels[0..4], &[32, 32, 32, 255]);
    assert_eq!(&pixels[16..20], &[255, 0, 0, 255]);
    assert_eq!(
        canvas.last_stream_trace.unwrap().revisions(),
        &[(id, revision)]
    );
    assert_eq!(frozen.cache_id(), frozen_id);
    assert!(frozen.stream_revision().is_none());
    assert!(
        gpu.cache_stats().bytes < 128,
        "one shared cache counts native planes/uniform and RGBA payload"
    );
}
#[test]
#[ignore = "requires Vulkan; run explicitly with --ignored"]
fn video_scene_gpu_rejects_invalid_scene_before_publishing_native_upload_revision() {
    let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
    let mut canvas = Canvas::new(gpu.clone());
    let stream = ImageStream::new();
    let first = native(&stream, 32);
    let full = Rect::new(0., 0., 4., 4.);
    let target = gpu.device.create_texture(&TextureDescriptor {
        label: Some("native validation preservation"),
        size: Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Bgra8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    canvas.paint(&view, 4, 4, 1, &[video(first, full)]).unwrap();
    let resource = canvas.resources[0].clone();
    assert!(resource.is_video());
    assert_eq!(resource.revision.get(), 1);
    let second = native(&stream, 224);
    let invalid = [
        video(second, full),
        Paint::Shape {
            rect: Rect::new(f32::NAN, 0., 1., 1.),
            clip: Clip::from(full),
            color: Color::hex(0),
            radius: 0.,
        },
    ];
    assert!(canvas.paint(&view, 4, 4, 1, &invalid).is_err());
    assert_eq!(resource.revision.get(), 1);
    let pixels = readback_rgba(
        &gpu,
        &target,
        PhysicalExtent::density(4, 4, Density::integer(1).unwrap(), 8192)
            .unwrap()
            .readback(4096)
            .unwrap(),
        Vec::new(),
    )
    .unwrap();
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p == &[32, 32, 32, 255])
    );
}
