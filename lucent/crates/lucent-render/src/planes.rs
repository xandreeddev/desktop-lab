//! Shared native-plane GPU resources, without an Rc<Gpu> ownership cycle.
use super::*;
use lucent_api::video_frame::{ChromaSiting, Range, Transfer, Yuv420Frame};

pub(super) fn layout(gpu: &Gpu) -> BindGroupLayout {
    let mut entries: Vec<_> = (0..3)
        .map(|binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: false },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        })
        .collect();
    entries.push(BindGroupLayoutEntry {
        binding: 3,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: BufferSize::new(32),
        },
        count: None,
    });
    gpu.device
        .create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("native YUV420 planes"),
            entries: &entries,
        })
}
pub(super) struct PlaneResources {
    pub dimensions: (u32, u32),
    pub group: BindGroup,
    planes: [Texture; 3],
    uniform: Buffer,
}
impl PlaneResources {
    /// Frame extents must already be checked against this device by scene
    /// validation (or the standalone painter). No CPU pixels are uploaded yet.
    pub fn new(gpu: &Gpu, layout: &BindGroupLayout, frame: &Yuv420Frame) -> Self {
        let planes: [_; 3] = std::array::from_fn(|index| {
            let (width, height) = frame.planes()[index].dimensions();
            gpu.device.create_texture(&TextureDescriptor {
                label: Some("native video R8 plane"),
                size: Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::R8Unorm,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            })
        });
        let views: [_; 3] =
            std::array::from_fn(|index| planes[index].create_view(&Default::default()));
        let uniform = gpu.device.create_buffer(&BufferDescriptor {
            label: Some("video color and siting"),
            size: 32,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = gpu.device.create_bind_group(&BindGroupDescriptor {
            label: Some("native video planes"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&views[0]),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&views[1]),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&views[2]),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        Self {
            dimensions: frame.dimensions(),
            group,
            planes,
            uniform,
        }
    }
    pub fn texture_bytes(&self) -> u64 {
        self.planes
            .iter()
            .map(|p| {
                let size = p.size();
                u64::from(size.width) * u64::from(size.height)
            })
            .sum()
    }
    pub fn payload_bytes(&self) -> u64 {
        self.texture_bytes() + self.uniform.size()
    }
    /// Deferred until the validated scene is submitted. Queue writes precede
    /// that scene's encoder; each resource owns its own input planes/uniform.
    pub fn upload(&self, gpu: &Gpu, frame: &Yuv420Frame) -> u64 {
        debug_assert_eq!(self.dimensions, frame.dimensions());
        let mut bytes = 0;
        for (plane, texture) in frame.planes().iter().zip(&self.planes) {
            let (width, height) = plane.dimensions();
            gpu.queue.write_texture(
                TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                plane.bytes(),
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(plane.stride()),
                    rows_per_image: Some(height),
                },
                Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            bytes += u64::from(width) * u64::from(height);
        }
        let color = frame.color();
        let (w, h) = frame.dimensions();
        let (cw, ch) = frame.planes()[1].dimensions();
        let values = [
            w as f32,
            h as f32,
            cw as f32,
            ch as f32,
            if color.range == Range::Limited {
                1.
            } else {
                0.
            },
            if color.transfer == Transfer::Bt709 {
                1.
            } else {
                0.
            },
            if color.chroma == ChromaSiting::Left {
                0.
            } else {
                0.5
            },
            0.5,
        ];
        let mut metadata = [0; 32];
        for (slot, value) in metadata.as_chunks_mut::<4>().0.iter_mut().zip(values) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        gpu.queue.write_buffer(&self.uniform, 0, &metadata);
        bytes
    }
}
