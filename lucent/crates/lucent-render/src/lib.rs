//! Vulkan card renderer. Shape animation runs on the GPU; text uploads are cached.
//! Text is currently Latin rasterization, not a complete shaping engine.
use lucent_domain::{CARD_HEIGHT, CARD_WIDTH, CardVisual};
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
    uniforms: Buffer,
    sampler: Sampler,
    texture: Option<Texture>,
    group: Option<BindGroup>,
    pixels: Vec<u8>,
    text_size: (u32, u32),
    text_key: Option<(u32, String, u64)>,
    font: fontdue::Font,
    _instance: Instance,
    pub frames: u64,
}

impl Renderer {
    /// Creates a Vulkan renderer attached to an existing native Wayland surface.
    ///
    /// # Safety
    /// The pointer must identify a live wl_surface on the supplied display. The
    /// caller must destroy this renderer before the wl_surface. The owned display
    /// handle keeps its connection alive for the GPU instance.
    pub unsafe fn new(
        display: impl HasDisplayHandle + std::fmt::Debug + Send + Sync + 'static,
        surface: *mut c_void,
        font_bytes: Vec<u8>,
    ) -> Result<Self, Error> {
        let raw_display = display.display_handle()?.as_raw();
        let mut descriptor =
            InstanceDescriptor::new_with_display_handle_from_env(Box::new(display));
        descriptor.backends = Backends::VULKAN;
        let instance = Instance::new(descriptor);
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
        }))
        .map_err(|e| format!("A working Vulkan driver is required (VMs need Venus): {e}"))?;
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
        if !caps
            .alpha_modes
            .contains(&CompositeAlphaMode::PreMultiplied)
        {
            return Err("Vulkan surface must support premultiplied transparency".into());
        }
        eprintln!("lucent alpha: PreMultiplied");
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 1,
            height: 1,
            present_mode: PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: CompositeAlphaMode::PreMultiplied,
            view_formats: vec![],
            color_space: SurfaceColorSpace::Auto,
        };
        let inputs = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("card inputs"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let uniforms = device.create_buffer(&BufferDescriptor {
            label: Some("card animation"),
            size: 48,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&inputs)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(include_wgsl!("present.wgsl"));
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("lucent Vulkan card"),
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
            uniforms,
            sampler,
            texture: None,
            group: None,
            pixels: vec![],
            text_size: (0, 0),
            text_key: None,
            font,
            _instance: instance,
            frames: 0,
        })
    }

    pub fn render(
        &mut self,
        viewport: (u32, u32),
        scale: u32,
        title: &str,
        clicks: u64,
        card: CardVisual,
    ) -> Result<(), Error> {
        let (width, height) = viewport;
        if self.config.width != width || self.config.height != height {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
        self.prepare_text(scale, title, clicks);
        let values = [
            width as f32,
            height as f32,
            scale as f32,
            0.0,
            card.position.x,
            card.position.y,
            CARD_WIDTH,
            CARD_HEIGHT,
            card.scale,
            card.opacity,
            card.hover,
            card.color,
        ];
        let mut bytes = [0u8; 48];
        for (chunk, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(values) {
            chunk.copy_from_slice(&value.to_ne_bytes());
        }
        self.queue.write_buffer(&self.uniforms, 0, &bytes);
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
            pass.draw(0..6, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        self.frames += 1;
        eprintln!("lucent frame {}", self.frames);
        Ok(())
    }

    fn prepare_text(&mut self, scale: u32, title: &str, clicks: u64) {
        if self
            .text_key
            .as_ref()
            .is_some_and(|k| k.0 == scale && k.1 == title && k.2 == clicks)
        {
            return;
        }
        let width = CARD_WIDTH as u32 * scale;
        let height = CARD_HEIGHT as u32 * scale;
        if self.text_size != (width, height) {
            self.text_size = (width, height);
            self.pixels.resize((width * height * 4) as usize, 0);
            let texture = self.device.create_texture(&TextureDescriptor {
                label: Some("cached card lettering"),
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
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(&view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(&self.sampler),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: self.uniforms.as_entire_binding(),
                    },
                ],
            }));
            self.texture = Some(texture);
        }
        self.pixels.fill(0);
        let s = scale as f32;
        self.text("LUCENT", 12.0 * s, 50.0 * s, 26.0 * s, [157, 209, 225]);
        self.text(title, 30.0 * s, 30.0 * s, 63.0 * s, [232, 244, 251]);
        self.text(
            "A little space of your own.",
            17.0 * s,
            31.0 * s,
            111.0 * s,
            [173, 192, 210],
        );
        self.text(
            "Drag to move  ·  Click to recolor",
            14.0 * s,
            30.0 * s,
            178.0 * s,
            [174, 200, 216],
        );
        self.text(
            &format!("{:02}", clicks),
            14.0 * s,
            479.0 * s,
            114.0 * s,
            [157, 209, 225],
        );
        self.text(
            "Right-click to close",
            12.0 * s,
            382.0 * s,
            180.0 * s,
            [136, 160, 183],
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
        self.text_key = Some((scale, title.into(), clicks));
        eprintln!("lucent text upload");
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
                        || px >= self.text_size.0 as i32
                        || py >= self.text_size.1 as i32
                    {
                        continue;
                    }
                    let alpha = bitmap[row * m.width + col] as f32 / 255.0;
                    let i = ((py as u32 * self.text_size.0 + px as u32) * 4) as usize;
                    for (c, value) in color.iter().enumerate() {
                        self.pixels[i + c] = (f32::from(*value) * alpha
                            + f32::from(self.pixels[i + c]) * (1.0 - alpha))
                            as u8;
                    }
                    self.pixels[i + 3] =
                        (255.0 * alpha + f32::from(self.pixels[i + 3]) * (1.0 - alpha)) as u8;
                }
            }
            x += m.advance_width;
        }
    }
}
