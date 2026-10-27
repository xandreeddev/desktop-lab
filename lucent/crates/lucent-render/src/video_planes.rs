//! Opt-in native-plane rendering. Existing RGBA scenes are unchanged.
//! This slice draws one opaque video into a BGRA8-unorm target. It is not yet
//! wired into desktop wallpaper surfaces, UI paint or the stream mailbox.
use super::*;
use lucent_api::video_frame::{ChromaSiting, Range, Transfer, Yuv420Frame};

/// Texture payload and work performed by this painter, not total GPU memory/RSS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanarUploadStats {
    pub texture_payload_bytes: u64,
    pub texture_sets_created: u64,
    pub frames_uploaded: u64,
    pub bytes_uploaded: u64,
}
struct Textures {
    dimensions: (u32, u32),
    planes: [Texture; 3],
    group: BindGroup,
}
/// Reuses three R8 textures and one pipeline for a source extent. GPU uploads
/// still copy into wgpu staging; retaining native bytes is not DMA-BUF import.
pub struct PlanarPainter {
    gpu: Rc<Gpu>,
    pipeline: RenderPipeline,
    layout: BindGroupLayout,
    uniform: Buffer,
    textures: Option<Textures>,
    stats: PlanarUploadStats,
}
impl PlanarPainter {
    pub fn new(gpu: Rc<Gpu>) -> Self {
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
        let layout = gpu
            .device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("native YUV420 planes"),
                entries: &entries,
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some("native video"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(include_wgsl!("video_planes.wgsl"));
        let pipeline = gpu
            .device
            .create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("BT709 video to nonlinear sRGB BGRA8"),
                layout: Some(&pipeline_layout),
                vertex: VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(ColorTargetState {
                        format: TextureFormat::Bgra8Unorm,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        let uniform = gpu.device.create_buffer(&BufferDescriptor {
            label: Some("video color and siting"),
            size: 32,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            gpu,
            pipeline,
            layout,
            uniform,
            textures: None,
            stats: Default::default(),
        }
    }
    pub fn stats(&self) -> PlanarUploadStats {
        self.stats
    }
    fn upload(&mut self, frame: &Yuv420Frame) -> Result<(), Error> {
        let dimensions = frame.dimensions();
        let limit = self.gpu.device.limits().max_texture_dimension_2d;
        if dimensions.0 > limit || dimensions.1 > limit {
            return Err("Video exceeds device texture extent".into());
        }
        if self
            .textures
            .as_ref()
            .is_none_or(|value| value.dimensions != dimensions)
        {
            let planes = std::array::from_fn(|index| {
                let (width, height) = frame.planes()[index].dimensions();
                self.gpu.device.create_texture(&TextureDescriptor {
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
            let group = self.gpu.device.create_bind_group(&BindGroupDescriptor {
                label: Some("native video planes"),
                layout: &self.layout,
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
                        resource: self.uniform.as_entire_binding(),
                    },
                ],
            });
            self.stats.texture_payload_bytes = frame
                .planes()
                .iter()
                .map(|p| {
                    let (w, h) = p.dimensions();
                    u64::from(w) * u64::from(h)
                })
                .sum();
            self.stats.texture_sets_created += 1;
            self.textures = Some(Textures {
                dimensions,
                planes,
                group,
            });
        }
        let textures = self
            .textures
            .as_ref()
            .ok_or("Video texture admission failed")?;
        for (plane, texture) in frame.planes().iter().zip(&textures.planes) {
            let (width, height) = plane.dimensions();
            // Queue::write_texture accepts unaligned strides. The checked plane
            // contains every visible row; a final trailing padding row is absent.
            self.gpu.queue.write_texture(
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
            self.stats.bytes_uploaded += u64::from(width) * u64::from(height);
        }
        let color = frame.color();
        let (cw, ch) = frame.planes()[1].dimensions();
        let values = [
            dimensions.0 as f32,
            dimensions.1 as f32,
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
        let mut bytes = [0; 32];
        for (slot, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(values) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        self.gpu.queue.write_buffer(&self.uniform, 0, &bytes);
        self.stats.frames_uploaded += 1;
        Ok(())
    }
    /// Fill a single-sample `Bgra8Unorm` render attachment with opaque video.
    /// The target MUST NOT be an sRGB texture: the shader already emits nonlinear
    /// sRGB values, matching Lucent's current explicit unorm presentation target.
    pub fn draw_bgra8_unorm(
        &mut self,
        frame: &Yuv420Frame,
        target: &TextureView,
    ) -> Result<(), Error> {
        self.upload(frame)?;
        let textures = self
            .textures
            .as_ref()
            .ok_or("Video texture admission failed")?;
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("native video"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &textures.group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.gpu.queue.submit([encoder.finish()]);
        Ok(())
    }
    /// Validation-only readback. Normal draws do not allocate CPU RGBA pixels.
    pub fn snapshot(
        &mut self,
        frame: &Yuv420Frame,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, Error> {
        let limits = self.gpu.device.limits();
        let extent = PhysicalExtent::density(
            width,
            height,
            Density::integer(1)?,
            limits.max_texture_dimension_2d,
        )?;
        let readback = extent.readback(limits.max_buffer_size)?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(readback.rgba_bytes)?;
        let target = self.gpu.device.create_texture(&TextureDescriptor {
            label: Some("native video validation readback"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.draw_bgra8_unorm(frame, &target.create_view(&Default::default()))?;
        readback_rgba(&self.gpu, &target, readback, rgba)
    }
}

#[cfg(test)]
mod tests;
