//! Shared Vulkan device and cached text/image resources for arbitrary framework scenes.
use lucent_api::{Align, Rect};
use lucent_ui::Paint;
use std::{
    cell::RefCell,
    ffi::c_void,
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};
pub use wgpu::rwh as display_handle;
use wgpu::{rwh::*, *};
mod cache;
mod presentation;
mod raster;
pub use cache::CacheStats;
use cache::ResourceCache;
use presentation::{Recovery, SurfaceFailure};
pub type Error = Box<dyn std::error::Error>;
struct TextureResource {
    group: BindGroup,
    width: f32,
    height: f32,
    _texture: Texture,
    bytes: u64,
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
    cache: RefCell<ResourceCache<TextureResource>>,
    white: RefCell<Option<Rc<TextureResource>>>,
    device_failure: Arc<Mutex<Option<String>>>,
    pub adapter_description: String,
}
impl Gpu {
    pub fn new(
        display: impl HasDisplayHandle + std::fmt::Debug + Send + Sync + 'static,
        fonts: Arc<Vec<fontdue::Font>>,
    ) -> Result<Rc<Self>, Error> {
        let mut desc = InstanceDescriptor::new_with_display_handle_from_env(Box::new(display));
        desc.backends = Backends::VULKAN;
        Self::from_instance(Instance::new(desc), fonts)
    }
    /// Use the same Vulkan pipeline without a compositor for deterministic component tests.
    pub fn headless(fonts: Arc<Vec<fontdue::Font>>) -> Result<Rc<Self>, Error> {
        let mut desc = InstanceDescriptor::new_without_display_handle();
        desc.backends = Backends::VULKAN;
        Self::from_instance(Instance::new(desc), fonts)
    }
    fn from_instance(
        instance: Instance,
        fonts: Arc<Vec<fontdue::Font>>,
    ) -> Result<Rc<Self>, Error> {
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
                .map_err(|e| format!("A Vulkan driver is required: {e}"))?;
        let adapter_description = format!("{:?}", adapter.get_info());
        eprintln!("lucent GPU: {adapter_description}");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&DeviceDescriptor::default()))?;
        let device_failure = Arc::new(Mutex::new(None));
        let failure = device_failure.clone();
        device.set_device_lost_callback(move |reason, message| {
            *failure.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(format!("Vulkan device lost ({reason:?}): {message}"));
        });
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
            mipmap_filter: MipmapFilterMode::Linear,
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
            cache: RefCell::new(ResourceCache::new(64 * 1024 * 1024, 1024)),
            white: RefCell::new(None),
            device_failure,
            adapter_description,
        }))
    }
    /// Texture payload bytes exclude driver allocation overhead and surface buffers.
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.borrow().stats()
    }
    fn texture(&self, levels: &[lucent_api::ImageLevel]) -> Rc<TextureResource> {
        let first = &levels[0];
        let (width, height) = (first.width, first.height);
        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some("cached framework texture"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, pixels) in levels.iter().enumerate() {
            self.queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                &pixels.rgba,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pixels.width * 4),
                    rows_per_image: Some(pixels.height),
                },
                Extent3d {
                    width: pixels.width,
                    height: pixels.height,
                    depth_or_array_layers: 1,
                },
            );
        }
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
            bytes: levels.iter().map(|level| level.rgba.len() as u64).sum(),
        })
    }
    fn white(&self) -> Rc<TextureResource> {
        if let Some(v) = self.white.borrow().as_ref() {
            return v.clone();
        }
        let v = self.texture(&[lucent_api::ImageLevel {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        }]);
        *self.white.borrow_mut() = Some(v.clone());
        v
    }
    fn image(&self, data: &lucent_api::ImageData) -> Rc<TextureResource> {
        let key = format!("image:{}", data.cache_id());
        if let Some(v) = self.cache.borrow_mut().get(&key) {
            return v;
        }
        let texture = self.texture(data.levels());
        self.cache
            .borrow_mut()
            .insert(key, texture.clone(), texture.bytes);
        texture
    }
    fn text(&self, text: &str, size: f32, scale: u32, face: usize) -> Rc<TextureResource> {
        let key = format!("text:{face}:{size:.3}:{scale}:{text}");
        let font = self.fonts.get(face).unwrap_or(&self.fonts[0]);
        if let Some(v) = self.cache.borrow_mut().get(&key) {
            return v;
        }
        let (pixels, width, height) = raster::text(font, text, size, scale);
        let texture = self.texture(&[lucent_api::ImageLevel {
            width,
            height,
            rgba: pixels,
        }]);
        self.cache
            .borrow_mut()
            .insert(key, texture.clone(), texture.bytes);
        texture
    }
}
/// Draws an existing Paint scene into a native buffer or an offscreen test target.
/// The shaders, rasterization, cache and command encoding are shared by both paths.
pub struct Canvas {
    gpu: Rc<Gpu>,
    viewport: Buffer,
    frame_group: BindGroup,
    instances: Buffer,
    capacity: usize,
    resources: Vec<Rc<TextureResource>>,
}
impl Canvas {
    pub fn new(gpu: Rc<Gpu>) -> Self {
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
        Self {
            gpu,
            viewport,
            frame_group,
            instances,
            capacity: 1,
            resources: Vec::new(),
        }
    }
    pub fn paint(
        &mut self,
        view: &TextureView,
        width: u32,
        height: u32,
        scale: u32,
        paint: &[Paint],
    ) {
        let mut records = Vec::with_capacity(paint.len());
        let mut textures = Vec::with_capacity(paint.len());
        for primitive in paint {
            let (rect, clip, color, radius, kind, stroke, texture) = match primitive {
                Paint::Outline {
                    rect,
                    clip,
                    color,
                    radius,
                    width,
                } => (*rect, *clip, *color, *radius, 3., *width, self.gpu.white()),
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
                    0.,
                    self.gpu.white(),
                ),
                Paint::Image {
                    rect,
                    clip,
                    data,
                    radius,
                    opacity,
                    tint,
                } => (
                    *rect,
                    *clip,
                    tint.alpha(tint.3 * *opacity),
                    *radius,
                    1.,
                    0.,
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
                        Rect::new(
                            (x * scale as f32).round() / scale as f32,
                            ((rect.y + (rect.h - h).max(0.) / 2.) * scale as f32).round()
                                / scale as f32,
                            w,
                            h,
                        ),
                        *clip,
                        *color,
                        0.,
                        1.,
                        0.,
                        texture,
                    )
                }
            };
            if rect.w <= 0. || rect.h <= 0. || color.3 <= 0. {
                continue;
            }
            records.push([
                rect.x, rect.y, rect.w, rect.h, color.0, color.1, color.2, color.3, radius, kind,
                stroke, 0., clip.x, clip.y, clip.w, clip.h,
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
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                color_attachments: &[Some(RenderPassColorAttachment {
                    view,
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
        // Keep each surface's current scene resident, including while other
        // surfaces redraw. Replacing the scene releases its obsolete references.
        self.resources = textures;
        self.gpu.cache.borrow_mut().trim();
    }
    /// Return premultiplied RGBA pixels at `width*scale` by `height*scale`.
    pub fn snapshot(
        &mut self,
        width: u32,
        height: u32,
        scale: u32,
        paint: &[Paint],
    ) -> Result<Vec<u8>, Error> {
        if width == 0 || height == 0 || scale == 0 {
            return Err("nonzero snapshot dimensions required".into());
        }
        let (w, h) = (width * scale, height * scale);
        let extent = Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let texture = self.gpu.device.create_texture(&TextureDescriptor {
            label: Some("component visual snapshot"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.paint(
            &texture.create_view(&Default::default()),
            width,
            height,
            scale,
            paint,
        );
        let stride = (w * 4).div_ceil(COPY_BYTES_PER_ROW_ALIGNMENT) * COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.gpu.device.create_buffer(&BufferDescriptor {
            label: Some("visual snapshot readback"),
            size: u64::from(stride) * u64::from(h),
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: &buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(h),
                },
            },
            extent,
        );
        self.gpu.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        buffer.slice(..).map_async(MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.gpu.device.poll(PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(10)),
        })?;
        receiver.recv_timeout(std::time::Duration::from_secs(10))??;
        let mapped = buffer.slice(..).get_mapped_range()?;
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for row in mapped.chunks(stride as usize) {
            for pixel in row[..(w * 4) as usize].as_chunks::<4>().0 {
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
        }
        drop(mapped);
        buffer.unmap();
        Ok(rgba)
    }
}
impl Drop for Canvas {
    fn drop(&mut self) {
        self.resources.clear();
        self.gpu.cache.borrow_mut().trim();
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    use lucent_api::{Color, ImageData};

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn renderer_surface_resources_refresh_changed_assets() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let mut canvas = Canvas::new(gpu.clone());
        let rect = Rect::new(0., 0., 8., 8.);
        for rgba in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            let image = Arc::new(ImageData::from_rgba("same-file", 1, 1, rgba.to_vec()).unwrap());
            let paint = [Paint::Image {
                rect,
                clip: rect,
                data: image,
                radius: 0.,
                opacity: 1.,
                tint: Color::hex(0xffffff),
            }];
            let pixels = canvas.snapshot(8, 8, 1, &paint).unwrap();
            let center = (4 * 8 + 4) * 4;
            assert_eq!(&pixels[center..center + 4], &rgba);
        }
        assert_eq!(
            gpu.cache_stats().misses,
            2,
            "changed pixels reused a stale filename-based texture"
        );
    }

    #[test]
    #[ignore = "requires a Vulkan driver; uses an intentionally over-budget texture"]
    fn renderer_surface_resources_remain_resident() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let rect = Rect::new(0., 0., 64., 64.);
        let image = |key: &str, size: u32| Paint::Image {
            rect,
            clip: rect,
            data: Arc::new(
                ImageData::from_rgba(key, size, size, vec![255; (size * size * 4) as usize])
                    .unwrap(),
            ),
            radius: 0.,
            opacity: 1.,
            tint: Color::hex(0xffffff),
        };
        let preparation = std::time::Instant::now();
        let wallpaper = vec![image("large-resource-fixture", 4096)];
        let prepared_in = preparation.elapsed();
        let bar = vec![image("small-resource-fixture", 8)];
        let mut background = Canvas::new(gpu.clone());
        let mut panel = Canvas::new(gpu.clone());
        let upload = std::time::Instant::now();
        background.snapshot(64, 64, 1, &wallpaper).unwrap();
        println!(
            "4096px image preparation: {prepared_in:?}; first upload/render/readback: {:?}",
            upload.elapsed()
        );
        panel.snapshot(64, 64, 1, &bar).unwrap();
        let before = gpu.cache_stats();
        assert!(before.active_bytes > before.budget_bytes);
        let start = std::time::Instant::now();
        for _ in 0..5 {
            background.snapshot(64, 64, 1, &wallpaper).unwrap();
            panel.snapshot(64, 64, 1, &bar).unwrap();
        }
        let after = gpu.cache_stats();
        assert_eq!(
            after.misses, before.misses,
            "alternating surfaces re-uploaded resident textures"
        );
        assert_eq!(after.evictions, 0);
        assert_eq!(after.hits - before.hits, 10);
        println!(
            "Ten alternating warm 64px snapshots (includes readback): {:?}; resident payload bytes: {}",
            start.elapsed(),
            after.bytes
        );
        drop(background);
        assert!(gpu.cache_stats().bytes < before.budget_bytes);
        panel.snapshot(64, 64, 1, &bar).unwrap();
        assert_eq!(gpu.cache_stats().misses, after.misses);
    }
}
/// GPU presentation target. Must be dropped before its native Wayland surface.
pub struct Renderer {
    surface: Surface<'static>,
    gpu: Rc<Gpu>,
    config: SurfaceConfiguration,
    canvas: Canvas,
    display: RawDisplayHandle,
    window: RawWindowHandle,
    reconfigure: bool,
    recovery: Recovery,
    pub frames: u64,
}
/// A skipped presentation has no compositor callback; the runtime must schedule a retry.
#[derive(Debug, PartialEq, Eq)]
pub enum RenderOutcome {
    Presented,
    RetryAfter(Duration),
}
impl Renderer {
    /// # Safety
    /// `surface` must remain a live wl_surface on `display` until this renderer is dropped.
    pub unsafe fn new(
        gpu: Rc<Gpu>,
        display: RawDisplayHandle,
        surface: *mut c_void,
    ) -> Result<Self, Error> {
        let window = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(surface).ok_or("Null surface")?,
        ));
        let target = SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(display),
            raw_window_handle: window,
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
        // The runtime already follows compositor frame callbacks. Mailbox avoids
        // adding a second FIFO pacing queue while still presenting complete frames.
        let present_mode = if caps.present_modes.contains(&PresentMode::Mailbox) {
            PresentMode::Mailbox
        } else {
            PresentMode::Fifo
        };
        eprintln!(
            "Vulkan presentation: {present_mode:?}; supported {:?}",
            caps.present_modes
        );
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: TextureFormat::Bgra8Unorm,
            width: 0,
            height: 0,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: CompositeAlphaMode::PreMultiplied,
            view_formats: vec![],
            color_space: SurfaceColorSpace::Auto,
        };
        let canvas = Canvas::new(gpu.clone());
        Ok(Self {
            surface,
            gpu,
            config,
            canvas,
            display,
            window,
            reconfigure: true,
            recovery: Recovery::default(),
            frames: 0,
        })
    }
    pub fn render(
        &mut self,
        width: u32,
        height: u32,
        scale: u32,
        paint: &[Paint],
        before_present: impl FnOnce(),
    ) -> Result<RenderOutcome, Error> {
        if let Some(error) = self
            .gpu
            .device_failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Err(error.into());
        }
        let physical = (
            width.checked_mul(scale).ok_or("Surface width overflow")?,
            height.checked_mul(scale).ok_or("Surface height overflow")?,
        );
        if physical.0 == 0 || physical.1 == 0 {
            return Ok(RenderOutcome::RetryAfter(self.recovery.retry()));
        }
        if self.reconfigure || self.config.width != physical.0 || self.config.height != physical.1 {
            self.config.width = physical.0;
            self.config.height = physical.1;
            self.surface.configure(&self.gpu.device, &self.config);
            self.reconfigure = false;
        }
        let frame = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) => frame,
            CurrentSurfaceTexture::Suboptimal(frame) => {
                self.reconfigure = true;
                frame
            }
            other => {
                let failure = match other {
                    CurrentSurfaceTexture::Timeout => SurfaceFailure::Timeout,
                    CurrentSurfaceTexture::Occluded => SurfaceFailure::Occluded,
                    CurrentSurfaceTexture::Outdated => SurfaceFailure::Outdated,
                    CurrentSurfaceTexture::Lost => SurfaceFailure::Lost,
                    _ => return Err(format!("Surface presentation: {other:?}").into()),
                };
                self.reconfigure |= failure.reconfigure();
                if failure.recreate() {
                    // SAFETY: new() requires these native handles to outlive this renderer.
                    self.surface = unsafe {
                        self.gpu
                            .instance
                            .create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
                                raw_display_handle: Some(self.display),
                                raw_window_handle: self.window,
                            })
                    }?;
                }
                return Ok(RenderOutcome::RetryAfter(self.recovery.retry()));
            }
        };
        self.canvas.paint(
            &frame.texture.create_view(&Default::default()),
            width,
            height,
            scale,
            paint,
        );
        // Request a Wayland callback only when a buffer will actually be committed.
        before_present();
        self.gpu.queue.present(frame);
        self.frames += 1;
        self.recovery.presented();
        Ok(RenderOutcome::Presented)
    }
}
