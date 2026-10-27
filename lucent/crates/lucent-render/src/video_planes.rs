//! Opt-in native-plane rendering. Existing RGBA scenes are unchanged.
//! This helper draws one opaque video into a BGRA8-unorm target for
//! the standalone validation consumer. Normal UI scenes use VideoData/Paint::Video.
use super::*;
use crate::planes::PlaneResources;
use lucent_api::video_frame::Yuv420Frame;

/// Texture payload and work performed by this painter, not total GPU memory/RSS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanarUploadStats {
    pub texture_payload_bytes: u64,
    pub texture_sets_created: u64,
    pub frames_uploaded: u64,
    pub bytes_uploaded: u64,
}
/// Reuses three R8 textures and one pipeline for a source extent. GPU uploads
/// still copy into wgpu staging; retaining native bytes is not DMA-BUF import.
pub struct PlanarPainter {
    gpu: Rc<Gpu>,
    pipeline: RenderPipeline,
    layout: BindGroupLayout,
    textures: Option<PlaneResources>,
    stats: PlanarUploadStats,
}
impl PlanarPainter {
    pub fn new(gpu: Rc<Gpu>) -> Self {
        let layout = planes::layout(&gpu);
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some("native video"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let shader = gpu.device.create_shader_module(ShaderModuleDescriptor {
            label: Some("native video color"),
            source: ShaderSource::Wgsl(
                concat!(
                    include_str!("video_color.wgsl"),
                    include_str!("video_planes.wgsl")
                )
                .into(),
            ),
        });
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
        Self {
            gpu,
            pipeline,
            layout,
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
            let textures = PlaneResources::new(&self.gpu, &self.layout, frame);
            self.stats.texture_payload_bytes = textures.texture_bytes();
            self.stats.texture_sets_created += 1;
            self.textures = Some(textures);
        }
        let textures = self
            .textures
            .as_ref()
            .ok_or("Video texture admission failed")?;
        self.stats.bytes_uploaded += textures.upload(&self.gpu, frame);
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
