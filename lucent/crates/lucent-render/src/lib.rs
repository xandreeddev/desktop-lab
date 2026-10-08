//! Shared Vulkan device and cached text/image resources for arbitrary framework scenes.
use lucent_api::{Align, Color, Rect};
use lucent_ui::Paint;
use std::{cell::RefCell, collections::HashMap, ffi::c_void, ptr::NonNull, rc::Rc, sync::Arc};
pub use wgpu::rwh as display_handle;
use wgpu::{rwh::*, *};
pub type Error = Box<dyn std::error::Error>;
struct TextureResource {
    group: BindGroup,
    width: f32,
    height: f32,
    _texture: Texture,
}
/// One GPU device is shared by all native surfaces in an application.
pub struct Gpu {
    instance: Instance,
    adapter: Adapter,
    device: Device,
    queue: Queue,
    pipeline: RenderPipeline,
    texture_layout: BindGroupLayout,
    frame_layout: BindGroupLayout,
    sampler: Sampler,
    fonts: Arc<Vec<fontdue::Font>>,
    cache: RefCell<HashMap<String, Rc<TextureResource>>>,
    white: RefCell<Option<Rc<TextureResource>>>,
    pub adapter_description: String,
}
impl Gpu {
    pub fn new(
        display: impl HasDisplayHandle + std::fmt::Debug + Send + Sync + 'static,
        fonts: Arc<Vec<fontdue::Font>>,
    ) -> Result<Rc<Self>, Error> {
        let mut desc = InstanceDescriptor::new_with_display_handle_from_env(Box::new(display));
        desc.backends = Backends::VULKAN;
        let instance = Instance::new(desc);
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
                .map_err(|e| format!("A Vulkan driver is required: {e}"))?;
        let adapter_description = format!("{:?}", adapter.get_info());
        eprintln!("lucent GPU: {adapter_description}");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&DeviceDescriptor::default()))?;
        let texture_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("cached image"),
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
            ],
        });
        let frame_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("viewport"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        let module = device.create_shader_module(include_wgsl!("present.wgsl"));
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&texture_layout), Some(&frame_layout)],
            immediate_size: 0,
        });
        let attributes = vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Float32x4,3=>Float32x4];
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("framework primitives"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(VertexBufferLayout {
                    array_stride: 64,
                    step_mode: VertexStepMode::Instance,
                    attributes: &attributes,
                })],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: TextureFormat::Bgra8Unorm,
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Ok(Rc::new(Self {
            instance,
            adapter,
            device,
            queue,
            pipeline,
            texture_layout,
            frame_layout,
            sampler,
            fonts,
            cache: RefCell::new(HashMap::new()),
            white: RefCell::new(None),
            adapter_description,
        }))
    }
    fn texture(&self, bytes: &[u8], width: u32, height: u32) -> Rc<TextureResource> {
        let size = Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some("cached framework texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            bytes,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * 4),
                rows_per_image: Some(size.height),
            },
            size,
        );
        let view = texture.create_view(&Default::default());
        let group = self.device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &self.texture_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Rc::new(TextureResource {
            group,
            width: width as f32,
            height: height as f32,
            _texture: texture,
        })
    }
    fn white(&self) -> Rc<TextureResource> {
        if let Some(v) = self.white.borrow().as_ref() {
            return v.clone();
        }
        let v = self.texture(&[255, 255, 255, 255], 1, 1);
        *self.white.borrow_mut() = Some(v.clone());
        v
    }
    fn image(&self, data: &lucent_api::ImageData) -> Rc<TextureResource> {
        let key = format!("image:{}", data.key);
        if let Some(v) = self.cache.borrow().get(&key) {
            return v.clone();
        }
        let mut bytes = data.rgba.clone();
        for p in bytes.as_chunks_mut::<4>().0 {
            for c in 0..3 {
                p[c] = (u16::from(p[c]) * u16::from(p[3]) / 255) as u8;
            }
        }
        let texture = self.texture(&bytes, data.width, data.height);
        self.cache.borrow_mut().insert(key, texture.clone());
        texture
    }
    fn text(&self, text: &str, size: f32, scale: u32, face: usize) -> Rc<TextureResource> {
        let pixel_size = (size * scale as f32).max(1.);
        let key = format!("text:{face}:{pixel_size:.2}:{text}");
        let font = self.fonts.get(face).unwrap_or(&self.fonts[0]);
        if let Some(v) = self.cache.borrow().get(&key) {
            return v.clone();
        }
        let line_height = (pixel_size * 1.3).ceil();
        let lines: Vec<_> = text.lines().collect();
        let width = lines
            .iter()
            .map(|line| {
                line.chars()
                    .map(|c| font.metrics(c, pixel_size).advance_width)
                    .sum::<f32>()
            })
            .fold(0., f32::max)
            .ceil()
            .clamp(1., 8192.) as u32
            + 2;
        let height = (line_height * lines.len().max(1) as f32)
            .ceil()
            .clamp(1., 2048.) as u32;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        for (line_index, line) in lines.iter().enumerate() {
            let mut x = 0.;
            for c in line.chars() {
                let (m, bitmap) = font.rasterize(c, pixel_size);
                let top =
                    line_index as f32 * line_height + pixel_size - m.height as f32 - m.ymin as f32;
                for row in 0..m.height {
                    for col in 0..m.width {
                        let px = x as i32 + m.xmin + col as i32;
                        let py = top as i32 + row as i32;
                        if px >= 0 && py >= 0 && px < width as i32 && py < height as i32 {
                            let a = bitmap[row * m.width + col];
                            let i = ((py as u32 * width + px as u32) * 4) as usize;
                            for channel in 0..4 {
                                pixels[i + channel] = a.saturating_add(
                                    (u16::from(pixels[i + channel]) * (255 - u16::from(a)) / 255)
                                        as u8,
                                );
                            }
                        }
                    }
                }
                x += m.advance_width;
                if x > width as f32 {
                    break;
                }
            }
        }
        let texture = self.texture(&pixels, width, height);
        self.cache.borrow_mut().insert(key, texture.clone());
        texture
    }
}
/// GPU presentation target. Must be dropped before its native Wayland surface.
pub struct Renderer {
    surface: Surface<'static>,
    gpu: Rc<Gpu>,
    config: SurfaceConfiguration,
    viewport: Buffer,
    frame_group: BindGroup,
    instances: Buffer,
    capacity: usize,
    pub frames: u64,
}
impl Renderer {
    /// # Safety
    /// `surface` must remain a live wl_surface on `display` until this renderer is dropped.
    pub unsafe fn new(
        gpu: Rc<Gpu>,
        display: RawDisplayHandle,
        surface: *mut c_void,
    ) -> Result<Self, Error> {
        let target = SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(display),
            raw_window_handle: RawWindowHandle::Wayland(WaylandWindowHandle::new(
                NonNull::new(surface).ok_or("Null surface")?,
            )),
        };
        // SAFETY: guaranteed by this function's caller.
        let surface = unsafe { gpu.instance.create_surface_unsafe(target) }?;
        let caps = surface.get_capabilities(&gpu.adapter);
        if !caps.formats.contains(&TextureFormat::Bgra8Unorm)
            || !caps
                .alpha_modes
                .contains(&CompositeAlphaMode::PreMultiplied)
        {
            return Err("Vulkan surface requires BGRA8 and premultiplied alpha".into());
        }
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: TextureFormat::Bgra8Unorm,
            width: 0,
            height: 0,
            present_mode: PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: CompositeAlphaMode::PreMultiplied,
            view_formats: vec![],
            color_space: SurfaceColorSpace::Auto,
        };
        let viewport = gpu.device.create_buffer(&BufferDescriptor {
            label: Some("surface viewport"),
            size: 16,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_group = gpu.device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &gpu.frame_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: viewport.as_entire_binding(),
            }],
        });
        let instances = gpu.device.create_buffer(&BufferDescriptor {
            label: None,
            size: 64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            surface,
            gpu,
            config,
            viewport,
            frame_group,
            instances,
            capacity: 1,
            frames: 0,
        })
    }
    pub fn render(
        &mut self,
        width: u32,
        height: u32,
        scale: u32,
        paint: &[Paint],
    ) -> Result<(), Error> {
        let physical = (width * scale, height * scale);
        if self.config.width != physical.0 || self.config.height != physical.1 {
            self.config.width = physical.0;
            self.config.height = physical.1;
            self.surface.configure(&self.gpu.device, &self.config);
        }
        let mut records = Vec::with_capacity(paint.len());
        let mut textures = Vec::with_capacity(paint.len());
        for primitive in paint {
            let (rect, clip, color, radius, kind, texture) = match primitive {
                Paint::Shape {
                    rect,
                    clip,
                    color,
                    radius,
                    shadow,
                } => (
                    *rect,
                    *clip,
                    *color,
                    *radius,
                    if *shadow { 2. } else { 0. },
                    self.gpu.white(),
                ),
                Paint::Image {
                    rect,
                    clip,
                    data,
                    radius,
                    opacity,
                } => (
                    *rect,
                    *clip,
                    Color(1., 1., 1., *opacity),
                    *radius,
                    1.,
                    self.gpu.image(data),
                ),
                Paint::Text {
                    rect,
                    clip,
                    text,
                    size,
                    face,
                    color,
                    align,
                } => {
                    if text.is_empty() {
                        continue;
                    }
                    let texture = self.gpu.text(text, *size, scale, *face);
                    let w = texture.width / scale as f32;
                    let h = texture.height / scale as f32;
                    let x = rect.x
                        + match align {
                            Align::Start => 0.,
                            Align::Center => (rect.w - w) / 2.,
                            Align::End => rect.w - w,
                        }
                        .max(0.);
                    (
                        Rect::new(x, rect.y + (rect.h - h).max(0.) / 2., w, h),
                        *clip,
                        *color,
                        0.,
                        1.,
                        texture,
                    )
                }
            };
            if rect.w <= 0. || rect.h <= 0. || color.3 <= 0. {
                continue;
            }
            records.push([
                rect.x, rect.y, rect.w, rect.h, color.0, color.1, color.2, color.3, radius, kind,
                0., 0., clip.x, clip.y, clip.w, clip.h,
            ]);
            textures.push(texture);
        }
        if records.len() > self.capacity {
            self.capacity = records.len().next_power_of_two();
            self.instances = self.gpu.device.create_buffer(&BufferDescriptor {
                label: None,
                size: (self.capacity * 64) as u64,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let bytes: Vec<u8> = records
            .iter()
            .flatten()
            .flat_map(|f| f.to_ne_bytes())
            .collect();
        if !bytes.is_empty() {
            self.gpu.queue.write_buffer(&self.instances, 0, &bytes);
        }
        let view_bytes: Vec<u8> = [width as f32, height as f32, scale as f32, 0.]
            .iter()
            .flat_map(|f| f.to_ne_bytes())
            .collect();
        self.gpu.queue.write_buffer(&self.viewport, 0, &view_bytes);
        let frame = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(f) | CurrentSurfaceTexture::Suboptimal(f) => f,
            other => return Err(format!("Surface presentation: {other:?}").into()),
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.gpu.pipeline);
            pass.set_vertex_buffer(0, self.instances.slice(..));
            pass.set_bind_group(1, &self.frame_group, &[]);
            for (i, texture) in textures.iter().enumerate() {
                pass.set_bind_group(0, &texture.group, &[]);
                pass.draw(0..6, i as u32..i as u32 + 1);
            }
        }
        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(frame);
        self.frames += 1;
        // Bound resources from old search strings without invalidating in-flight draws.
        if self.gpu.cache.borrow().len() > 1024 {
            self.gpu
                .cache
                .borrow_mut()
                .retain(|_, v| Rc::strong_count(v) > 1);
        }
        Ok(())
    }
}
