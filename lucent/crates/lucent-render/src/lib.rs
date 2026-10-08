//! Prototype GPU presentation backend. Raster resources are cached between input events.
//! Text is currently Latin rasterization, not a complete shaping engine.
use std::{ffi::c_void, ptr::NonNull};
use wgpu::{rwh::*, *};

pub type Error = Box<dyn std::error::Error>;

pub struct Renderer {
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    pipeline: RenderPipeline,
    inputs: BindGroupLayout,
    texture: Option<Texture>,
    group: Option<BindGroup>,
    pixels: Vec<u8>,
    font: fontdue::Font,
    _instance: Instance,
    pub frames: u64,
}

impl Renderer {
    /// Creates a renderer attached to an existing native Wayland surface.
    ///
    /// # Safety
    /// The pointer must identify a live wl_surface on the supplied display. The
    /// caller must destroy this renderer before the wl_surface. The owned display
    /// handle keeps its connection alive for the GPU instance, including EGL.
    pub unsafe fn new(
        display: impl HasDisplayHandle + std::fmt::Debug + Send + Sync + 'static,
        surface: *mut c_void,
        font_bytes: Vec<u8>,
    ) -> Result<Self, Error> {
        let raw_display = display.display_handle()?.as_raw();
        let instance = Instance::new(InstanceDescriptor::new_with_display_handle_from_env(
            Box::new(display),
        ));
        let target = SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(raw_display),
            raw_window_handle: RawWindowHandle::Wayland(WaylandWindowHandle::new(
                NonNull::new(surface).ok_or("null Wayland surface")?,
            )),
        };
        // SAFETY: the caller guarantees lifetime and pointer validity as documented above.
        let surface = unsafe { instance.create_surface_unsafe(target) }?;
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        eprintln!("lucent GPU: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&DeviceDescriptor::default()))?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, TextureFormat::Bgra8Unorm | TextureFormat::Rgba8Unorm))
            .ok_or("no linear surface format")?;
        let alpha_mode = if caps
            .alpha_modes
            .contains(&CompositeAlphaMode::PreMultiplied)
        {
            CompositeAlphaMode::PreMultiplied
        } else if caps.alpha_modes.contains(&CompositeAlphaMode::Opaque) {
            // wgpu's GLES backend currently exposes only opaque presentation.
            // Keep the rounded card on a deliberate opaque canvas in that case.
            CompositeAlphaMode::Opaque
        } else {
            return Err("surface has no supported alpha mode".into());
        };
        eprintln!("lucent alpha: {alpha_mode:?}");
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 1,
            height: 1,
            present_mode: PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
            color_space: SurfaceColorSpace::Auto,
        };
        let inputs = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("canvas"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: false },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&inputs)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(include_wgsl!("present.wgsl"));
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("lucent presentation"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
            .map_err(std::io::Error::other)?;
        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            inputs,
            texture: None,
            group: None,
            pixels: vec![],
            font,
            _instance: instance,
            frames: 0,
        })
    }

    pub fn render(
        &mut self,
        width: u32,
        height: u32,
        scale: u32,
        title: &str,
        clicks: u64,
    ) -> Result<(), Error> {
        if self.texture.is_none() || self.config.width != width || self.config.height != height {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.pixels.resize((width * height * 4) as usize, 0);
            let texture = self.device.create_texture(&TextureDescriptor {
                label: Some("cached prototype canvas"),
                size: Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.group = Some(self.device.create_bind_group(&BindGroupDescriptor {
                label: None,
                layout: &self.inputs,
                entries: &[BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&view),
                }],
            }));
            self.texture = Some(texture);
        }
        let s = scale as f32;
        let radius = 28.0 * s;
        let opaque = self.config.alpha_mode == CompositeAlphaMode::Opaque;
        let inset = if opaque { 8.0 * s } else { 0.0 };
        for y in 0..height {
            for x in 0..width {
                let dx = (x as f32 + 0.5 - width as f32 / 2.0).abs()
                    - (width as f32 / 2.0 - radius - inset);
                let dy = (y as f32 + 0.5 - height as f32 / 2.0).abs()
                    - (height as f32 / 2.0 - radius - inset);
                let distance = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - radius;
                let coverage = (0.5 - distance).clamp(0.0, 1.0);
                let i = ((y * width + x) * 4) as usize;
                let rgb = if clicks.is_multiple_of(2) {
                    [29.0, 47.0, 68.0]
                } else {
                    [34.0, 67.0, 72.0]
                };
                for (channel, value) in rgb.iter().enumerate() {
                    let background = if opaque {
                        [13.0, 20.0, 30.0][channel]
                    } else {
                        0.0
                    };
                    self.pixels[i + channel] =
                        (*value * coverage + background * (1.0 - coverage)) as u8;
                }
                self.pixels[i + 3] = if opaque {
                    255
                } else {
                    (255.0 * coverage) as u8
                };
            }
        }
        self.text(title, 30.0 * s, 30.0 * s, 38.0 * s, [151, 222, 242]);
        self.text(
            &format!("Native Wayland + wgpu  |  clicks: {clicks}"),
            16.0 * s,
            30.0 * s,
            92.0 * s,
            [222, 235, 245],
        );
        self.text(
            "Click to change color. Right-click to close.",
            15.0 * s,
            30.0 * s,
            129.0 * s,
            [184, 199, 215],
        );
        self.queue.write_texture(
            TexelCopyTextureInfo {
                texture: self.texture.as_ref().unwrap(),
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &self.pixels,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let frame = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(f) | CurrentSurfaceTexture::Suboptimal(f) => f,
            other => return Err(format!("surface unavailable: {other:?}").into()),
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let attachment = RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
            };
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                color_attachments: &[Some(attachment)],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, self.group.as_ref().unwrap(), &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        self.frames += 1;
        eprintln!("lucent frame {}", self.frames);
        Ok(())
    }

    fn text(&mut self, text: &str, size: f32, left: f32, top: f32, color: [u8; 3]) {
        let mut x = left;
        for ch in text.chars() {
            let (m, bitmap) = self.font.rasterize(ch, size);
            let start_y = top + size - m.height as f32 - m.ymin as f32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let px = x as i32 + m.xmin + col as i32;
                    let py = start_y as i32 + row as i32;
                    if px < 0
                        || py < 0
                        || px >= self.config.width as i32
                        || py >= self.config.height as i32
                    {
                        continue;
                    }
                    let alpha = bitmap[row * m.width + col] as f32 / 255.0;
                    let i = ((py as u32 * self.config.width + px as u32) * 4) as usize;
                    for (c, value) in color.iter().enumerate() {
                        self.pixels[i + c] = (f32::from(*value) * alpha
                            + f32::from(self.pixels[i + c]) * (1.0 - alpha))
                            as u8;
                    }
                }
            }
            x += m.advance_width;
        }
    }
}
