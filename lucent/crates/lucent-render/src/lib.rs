//! Shared Vulkan device and cached text/image resources for arbitrary framework scenes.
use lucent_api::{Align, Density, ImageFormat, Rect};
use lucent_ui::Paint;
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};
pub use wgpu::rwh as display_handle;
use wgpu::{rwh::*, *};
mod cache;
mod extent;
mod presentation;
mod profiling;
pub use profiling::{PaintProfile, RenderProfile, ResourceProfile};
mod fallback;
mod raster;
mod scene;
pub use cache::CacheStats;
use cache::ResourceCache;
use extent::{PhysicalExtent, ReadbackLayout};
use presentation::{Recovery, SurfaceFailure};
pub type Error = Box<dyn std::error::Error>;
struct TextureResource {
    group: BindGroup,
    width: f32,
    height: f32,
    _texture: Texture,
    revision: Cell<u64>,
    image_lifetime: RefCell<Option<lucent_api::ImageLifetime>>,
    bytes: u64,
    /// Logical left bearing for text masks; ordinary image resources use zero.
    text_origin_x: f32,
}
impl TextureResource {
    fn reusable(&self) -> bool {
        self.image_lifetime
            .borrow()
            .as_ref()
            .is_none_or(lucent_api::ImageLifetime::is_alive)
    }
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
    fonts: Arc<Vec<lucent_ui::text::Font>>,
    cache: RefCell<ResourceCache<TextureResource>>,
    white: RefCell<Option<Rc<TextureResource>>>,
    device_failure: Arc<Mutex<Option<String>>>,
    pub adapter_description: String,
}
impl Gpu {
    pub fn new(
        display: impl HasDisplayHandle + std::fmt::Debug + Send + Sync + 'static,
        fonts: Arc<Vec<lucent_ui::text::Font>>,
    ) -> Result<Rc<Self>, Error> {
        let mut desc = InstanceDescriptor::new_with_display_handle_from_env(Box::new(display));
        desc.backends = Backends::VULKAN;
        Self::from_instance(Instance::new(desc), fonts)
    }
    /// Use the same Vulkan pipeline without a compositor for deterministic component tests.
    pub fn headless(fonts: Arc<Vec<lucent_ui::text::Font>>) -> Result<Rc<Self>, Error> {
        let mut desc = InstanceDescriptor::new_without_display_handle();
        desc.backends = Backends::VULKAN;
        Self::from_instance(Instance::new(desc), fonts)
    }
    fn from_instance(
        instance: Instance,
        fonts: Arc<Vec<lucent_ui::text::Font>>,
    ) -> Result<Rc<Self>, Error> {
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
                .map_err(|e| format!("A Vulkan driver is required: {e}"))?;
        let adapter_description = format!("{:?}", adapter.get_info());
        eprintln!("lucent GPU: {adapter_description}");
        // A desktop UI has modest active resources and long idle periods. Ask
        // the backend to favor smaller allocation pools over throughput. This
        // is an allocation preference, not a hard memory/resource limit.
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            memory_hints: MemoryHints::MemoryUsage,
            ..Default::default()
        }))?;
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
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
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
        let attributes = vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Float32x4,3=>Float32x4,4=>Float32x4,5=>Float32x4];
        let constants = [(
            "SHADOW_OUTSET",
            f64::from(lucent_api::Shadow::blur_outset(1.)),
        )];
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("framework primitives"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                buffers: &[Some(VertexBufferLayout {
                    array_stride: scene::INSTANCE_BYTES,
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
                compilation_options: PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
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
    fn texture(
        &self,
        levels: &[lucent_api::ImageLevel],
        format: ImageFormat,
        profile: &mut ResourceProfile,
        enabled: bool,
    ) -> Rc<TextureResource> {
        let started = profiling::start(enabled);
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
            format: match format {
                ImageFormat::PremultipliedRgba8 => TextureFormat::Rgba8Unorm,
                ImageFormat::Alpha8 => TextureFormat::R8Unorm,
            },
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
                &pixels.pixels,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pixels.width * format.bytes_per_pixel()),
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
        let texture = Rc::new(TextureResource {
            group,
            width: width as f32,
            height: height as f32,
            _texture: texture,
            revision: Cell::new(0),
            image_lifetime: RefCell::new(None),
            bytes: levels.iter().map(|level| level.pixels.len() as u64).sum(),
            text_origin_x: 0.,
        });
        if enabled {
            profile.texture_upload += profiling::elapsed(started);
            profile.texture_upload_bytes += texture.bytes;
        }
        texture
    }
    fn white(&self, profile: &mut ResourceProfile, enabled: bool) -> Rc<TextureResource> {
        if let Some(v) = self.white.borrow().as_ref() {
            return v.clone();
        }
        let v = self.texture(
            &[lucent_api::ImageLevel {
                width: 1,
                height: 1,
                pixels: vec![255; 4],
            }],
            ImageFormat::PremultipliedRgba8,
            profile,
            enabled,
        );
        *self.white.borrow_mut() = Some(v.clone());
        v
    }
    fn image(
        &self,
        data: &lucent_api::ImageData,
        profile: &mut ResourceProfile,
        enabled: bool,
    ) -> Rc<TextureResource> {
        let streamed = data.stream_revision();
        let key = streamed.map_or_else(
            || format!("image:{}", data.cache_id()),
            |(id, _)| format!("stream:{id}"),
        );
        if let Some(texture) = self.cache.borrow_mut().get(&key) {
            if let Some((_, revision)) = streamed {
                if texture.width == data.width() as f32 && texture.height == data.height() as f32 {
                    if texture.revision.get() != revision {
                        let started = profiling::start(enabled);
                        self.queue.write_texture(
                            TexelCopyTextureInfo {
                                texture: &texture._texture,
                                mip_level: 0,
                                origin: Origin3d::ZERO,
                                aspect: TextureAspect::All,
                            },
                            data.pixels(),
                            TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(data.width() * 4),
                                rows_per_image: Some(data.height()),
                            },
                            Extent3d {
                                width: data.width(),
                                height: data.height(),
                                depth_or_array_layers: 1,
                            },
                        );
                        texture.revision.set(revision);
                        *texture.image_lifetime.borrow_mut() = Some(data.lifetime());
                        if enabled {
                            profile.texture_upload += profiling::elapsed(started);
                            profile.texture_upload_bytes += data.bytes() as u64;
                        }
                    }
                    return texture;
                }
                // Size changes replace this cache slot; already submitted work
                // keeps its own resource reference until its lifetime ends.
            } else {
                return texture;
            }
        }
        if enabled {
            profile.image_misses += 1;
        }
        let texture = self.texture(data.levels(), data.format(), profile, enabled);
        *texture.image_lifetime.borrow_mut() = Some(data.lifetime());
        if let Some((_, revision)) = streamed {
            texture.revision.set(revision);
        }
        self.cache
            .borrow_mut()
            .insert(key, texture.clone(), texture.bytes);
        texture
    }
    fn text(
        &self,
        text: &str,
        size: f32,
        scale: f32,
        face: usize,
        profile: &mut ResourceProfile,
        enabled: bool,
    ) -> Result<Rc<TextureResource>, Error> {
        let font = self
            .fonts
            .get(face)
            .or_else(|| self.fonts.first())
            .ok_or("text requires at least one font")?;
        let key = format!("text:{face}:{}:{scale}:{text}", size.to_bits());
        if let Some(v) = self.cache.borrow_mut().get(&key) {
            return Ok(v);
        }
        let started = profiling::start(enabled);
        let (pixels, width, height, origin_x) = raster::text(
            font,
            text,
            size,
            scale,
            self.device.limits().max_texture_dimension_2d,
        )?;
        if enabled {
            profile.text_raster += profiling::elapsed(started);
            profile.text_misses += 1;
        }
        let mut texture = self.texture(
            &[lucent_api::ImageLevel {
                width,
                height,
                pixels,
            }],
            ImageFormat::Alpha8,
            profile,
            enabled,
        );
        Rc::get_mut(&mut texture)
            .expect("new text texture is not shared")
            .text_origin_x = origin_x;
        self.cache
            .borrow_mut()
            .insert(key, texture.clone(), texture.bytes);
        Ok(texture)
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
    clips: Buffer,
    clip_capacity: usize,
    profiling: bool,
    last_profile: Option<PaintProfile>,
}
struct PreparedScene {
    instances: Vec<u8>,
    clips: Vec<u8>,
    textures: Vec<Rc<TextureResource>>,
    geometry: scene::Geometry,
}
struct ValidatedScene<'a> {
    width: u32,
    height: u32,
    density: Density,
    paint: &'a [Paint],
    geometry: scene::Geometry,
    validation: Duration,
}
fn append_record(bytes: &mut Vec<u8>, record: &[f32]) {
    for value in record {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
}
impl Canvas {
    pub fn new(gpu: Rc<Gpu>) -> Self {
        let viewport = gpu.device.create_buffer(&BufferDescriptor {
            label: Some("surface viewport"),
            size: 16,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let clips = gpu.device.create_buffer(&BufferDescriptor {
            label: Some("shape clip chain"),
            size: scene::CLIP_BYTES,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_group = gpu.device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &gpu.frame_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: viewport.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: clips.as_entire_binding(),
                },
            ],
        });
        let instances = gpu.device.create_buffer(&BufferDescriptor {
            label: None,
            size: scene::INSTANCE_BYTES,
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
            clips,
            clip_capacity: 1,
            profiling: false,
            last_profile: None,
        }
    }
    /// Requested geometry-buffer payload, excluding driver allocation overhead.
    pub fn buffer_payload_bytes(&self) -> u64 {
        self.capacity as u64 * scene::INSTANCE_BYTES
            + self.clip_capacity as u64 * scene::CLIP_BYTES
            + 16
    }
    fn validate_scene<'a>(
        &mut self,
        width: u32,
        height: u32,
        density: Density,
        paint: &'a [Paint],
    ) -> Result<ValidatedScene<'a>, Error> {
        self.last_profile = None;
        let started = profiling::start(self.profiling);
        let limits = self.gpu.device.limits();
        PhysicalExtent::density(width, height, density, limits.max_texture_dimension_2d)?;
        let geometry = scene::validate(
            paint,
            density.factor(),
            &self.gpu.fonts,
            &limits,
            self.capacity,
            self.clip_capacity,
        )?;
        Ok(ValidatedScene {
            width,
            height,
            density,
            paint,
            geometry,
            validation: profiling::elapsed(started),
        })
    }
    fn prepare(
        &self,
        validated: &ValidatedScene<'_>,
        profile: &mut PaintProfile,
    ) -> Result<PreparedScene, Error> {
        let ValidatedScene {
            density,
            paint,
            geometry,
            ..
        } = *validated;
        let scale = density.factor();
        let mut records = Vec::new();
        records.try_reserve_exact(paint.len() * scene::INSTANCE_BYTES as usize)?;
        let mut textures = Vec::new();
        textures.try_reserve_exact(paint.len())?;
        let mut clip_records = Vec::new();
        clip_records.try_reserve_exact(geometry.clip_count * scene::CLIP_BYTES as usize)?;
        for (index, primitive) in paint.iter().enumerate() {
            let (rect, clip, color, radius, kind, detail, texture) = match primitive {
                Paint::Outline {
                    rect,
                    clip,
                    color,
                    radius,
                    width,
                } => (
                    *rect,
                    clip,
                    *color,
                    *radius,
                    3.,
                    *width,
                    self.gpu.white(&mut profile.resources, self.profiling),
                ),
                Paint::Shape {
                    rect,
                    clip,
                    color,
                    radius,
                } => (
                    *rect,
                    clip,
                    *color,
                    *radius,
                    0.,
                    0.,
                    self.gpu.white(&mut profile.resources, self.profiling),
                ),
                Paint::Shadow {
                    rect,
                    clip,
                    color,
                    radius,
                    blur,
                } => (
                    *rect,
                    clip,
                    *color,
                    *radius,
                    2.,
                    *blur,
                    self.gpu.white(&mut profile.resources, self.profiling),
                ),
                Paint::Image {
                    rect,
                    clip,
                    data,
                    radius,
                    opacity,
                    tint,
                    ..
                } => (
                    *rect,
                    clip,
                    tint.alpha(tint.3 * *opacity),
                    *radius,
                    1.,
                    if data.format() == ImageFormat::Alpha8 {
                        1.
                    } else {
                        0.
                    },
                    self.gpu.image(data, &mut profile.resources, self.profiling),
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
                    let texture = self
                        .gpu
                        .text(
                            text,
                            *size,
                            scale,
                            *face,
                            &mut profile.resources,
                            self.profiling,
                        )
                        .map_err(|e| format!("scene primitive {index}: {e}"))?;
                    let w = texture.width / scale;
                    let h = texture.height / scale;
                    let x = rect.x
                        + match align {
                            Align::Start => 0.,
                            Align::Center => (rect.w - w) / 2.,
                            Align::End => rect.w - w,
                        }
                        .max(0.);
                    (
                        Rect::new(
                            (x * scale).round() / scale + texture.text_origin_x,
                            ((rect.y + (rect.h - h).max(0.) / 2.) * scale).round() / scale,
                            w,
                            h,
                        ),
                        clip,
                        *color,
                        0.,
                        1.,
                        1.,
                        texture,
                    )
                }
            };
            if rect.w <= 0. || rect.h <= 0. || color.3 <= 0. {
                continue;
            }
            let clip_start = clip_records.len() / scene::CLIP_BYTES as usize;
            for shape in clip.shapes() {
                let (rect, parameters) = match shape {
                    lucent_api::ClipShape::Rounded(shape) => {
                        (shape.rect, [shape.radius, 0., 0., 0.])
                    }
                    lucent_api::ClipShape::Diagonal(shape) => {
                        (shape.rect, [shape.progress(), 1., 0., 0.])
                    }
                };
                append_record(
                    &mut clip_records,
                    &[
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h,
                        parameters[0],
                        parameters[1],
                        parameters[2],
                        parameters[3],
                    ],
                );
            }
            let uv = match primitive {
                Paint::Image { uv, .. } => *uv,
                _ => Rect::new(0., 0., 1., 1.),
            };
            let bounds = clip.bounds;
            append_record(
                &mut records,
                &[
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    color.0,
                    color.1,
                    color.2,
                    color.3,
                    radius,
                    kind,
                    detail,
                    0.,
                    bounds.x,
                    bounds.y,
                    bounds.w,
                    bounds.h,
                    uv.x,
                    uv.y,
                    uv.w,
                    uv.h,
                    clip_start as f32,
                    clip.shapes().len() as f32,
                    0.,
                    0.,
                ],
            );
            textures.push(texture);
        }
        Ok(PreparedScene {
            instances: records,
            clips: clip_records,
            textures,
            geometry,
        })
    }
    /// Draw a scene to a compatible BGRA8 target from this canvas's device.
    /// Input rejection preserves current resources and submits no new frame.
    /// Actual device-memory availability and external target compatibility remain
    /// the caller/backend's responsibility.
    pub fn paint(
        &mut self,
        view: &TextureView,
        width: u32,
        height: u32,
        scale: u32,
        paint: &[Paint],
    ) -> Result<(), Error> {
        self.paint_scaled(view, width, height, Density::integer(scale)?, paint)
    }
    pub fn paint_scaled(
        &mut self,
        view: &TextureView,
        width: u32,
        height: u32,
        density: Density,
        paint: &[Paint],
    ) -> Result<(), Error> {
        let validated = self.validate_scene(width, height, density, paint)?;
        self.paint_validated(view, validated)
    }
    fn paint_validated(
        &mut self,
        view: &TextureView,
        validated: ValidatedScene<'_>,
    ) -> Result<(), Error> {
        let started = profiling::start(self.profiling);
        let mut profile = PaintProfile::default();
        let (width, height) = (validated.width, validated.height);
        let physical = validated.density.physical_size(width, height)?;
        let PreparedScene {
            instances,
            clips,
            textures,
            geometry,
        } = match self.prepare(&validated, &mut profile) {
            Ok(prepared) => prepared,
            Err(error) => {
                // Partially prepared resources have dropped their temporary
                // references; current scenes remain pinned in their canvases.
                self.gpu.cache.borrow_mut().trim(TextureResource::reusable);
                return Err(error);
            }
        };
        if textures.len() > self.capacity {
            self.capacity = geometry.instances.capacity;
            self.instances = self.gpu.device.create_buffer(&BufferDescriptor {
                label: None,
                size: geometry.instances.bytes,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if clips.len() / scene::CLIP_BYTES as usize > self.clip_capacity {
            self.clip_capacity = geometry.clips.capacity;
            self.clips = self.gpu.device.create_buffer(&BufferDescriptor {
                label: Some("shape clip chain"),
                size: geometry.clips.bytes,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.frame_group = self.gpu.device.create_bind_group(&BindGroupDescriptor {
                label: None,
                layout: &self.gpu.frame_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: self.viewport.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: self.clips.as_entire_binding(),
                    },
                ],
            });
        }
        if !clips.is_empty() {
            self.gpu.queue.write_buffer(&self.clips, 0, &clips);
        }
        if !instances.is_empty() {
            self.gpu.queue.write_buffer(&self.instances, 0, &instances);
        }
        let view_bytes: Vec<u8> = [
            width as f32,
            height as f32,
            physical.0 as f32 / width as f32,
            physical.1 as f32 / height as f32,
        ]
        .iter()
        .flat_map(|f| f.to_ne_bytes())
        .collect();
        self.gpu.queue.write_buffer(&self.viewport, 0, &view_bytes);
        profile.prepare = validated.validation + profiling::elapsed(started);
        let started = profiling::start(self.profiling);
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
        let command = encoder.finish();
        profile.encode = profiling::elapsed(started);
        let started = profiling::start(self.profiling);
        self.gpu.queue.submit([command]);
        profile.submit = profiling::elapsed(started);
        let started = profiling::start(self.profiling);
        // Keep each surface's current scene resident, including while other
        // surfaces redraw. Replacing the scene releases its obsolete references.
        self.resources = textures;
        self.gpu.cache.borrow_mut().trim(TextureResource::reusable);
        profile.retire = profiling::elapsed(started);
        self.last_profile = self.profiling.then_some(profile);
        Ok(())
    }
    /// Return premultiplied RGBA pixels at `width*scale` by `height*scale`.
    /// Invalid/overflowing sizes and device texture/readback limits return an
    /// error before rendering. Device limits do not guarantee available memory.
    pub fn snapshot(
        &mut self,
        width: u32,
        height: u32,
        scale: u32,
        paint: &[Paint],
    ) -> Result<Vec<u8>, Error> {
        self.snapshot_scaled(width, height, Density::integer(scale)?, paint)
    }
    pub fn snapshot_scaled(
        &mut self,
        width: u32,
        height: u32,
        density: Density,
        paint: &[Paint],
    ) -> Result<Vec<u8>, Error> {
        let limits = self.gpu.device.limits();
        let physical =
            PhysicalExtent::density(width, height, density, limits.max_texture_dimension_2d)?;
        let readback = physical.readback(limits.max_buffer_size)?;
        let validated = self.validate_scene(width, height, density, paint)?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(readback.rgba_bytes)?;
        let extent = Extent3d {
            width: physical.width,
            height: physical.height,
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
        self.paint_validated(&texture.create_view(&Default::default()), validated)?;
        self.readback(&texture, readback, rgba)
    }
    fn readback(
        &self,
        texture: &Texture,
        readback: ReadbackLayout,
        mut rgba: Vec<u8>,
    ) -> Result<Vec<u8>, Error> {
        let extent = texture.size();
        let buffer = self.gpu.device.create_buffer(&BufferDescriptor {
            label: Some("visual snapshot readback"),
            size: readback.buffer_bytes,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: &buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(readback.stride),
                    rows_per_image: Some(extent.height),
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
        for row in mapped.chunks_exact(readback.stride as usize) {
            for pixel in row[..readback.row_bytes].as_chunks::<4>().0 {
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
        self.gpu.cache.borrow_mut().trim(TextureResource::reusable);
    }
}

#[cfg(test)]
mod resource_tests {
    #[test]
    #[ignore = "requires Vulkan; run explicitly with --ignored"]
    fn fractional_pixels_use_actual_per_axis_density_and_keep_integer_convenience_identical() {
        let fonts = Arc::new(vec![
            lucent_ui::text::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
                Default::default(),
            )
            .unwrap(),
        ]);
        let gpu = Gpu::headless(fonts).unwrap();
        let mut canvas = Canvas::new(gpu);
        let (width, height) = (65, 13);
        let clip = lucent_api::Clip::from(Rect::new(4.15, 3.65, 28.23, 4.59));
        let paint = [Paint::Shape {
            rect: Rect::new(0., 0., width as f32, height as f32),
            clip: clip.clone(),
            color: lucent_api::Color::hex(0xffffff),
            radius: 0.,
        }];
        for density in [
            Density::new(5, 4).unwrap(),
            Density::new(3, 2).unwrap(),
            Density::new(7, 4).unwrap(),
            Density::new(9, 4).unwrap(),
        ] {
            let pixels = canvas
                .snapshot_scaled(width, height, density, &paint)
                .unwrap();
            let (pw, ph) = density.physical_size(width, height).unwrap();
            assert_eq!(pixels.len(), (pw * ph * 4) as usize);
            for y in 0..ph {
                for x in 0..pw {
                    let point = (
                        (x as f32 + 0.5) / (pw as f32 / width as f32),
                        (y as f32 + 0.5) / (ph as f32 / height as f32),
                    );
                    let expected = if clip.contains(point.0, point.1) {
                        255
                    } else {
                        0
                    };
                    assert_eq!(
                        pixels[((y * pw + x) * 4 + 3) as usize],
                        expected,
                        "sample {x},{y} at {density:?}"
                    );
                }
            }
        }
        for scale in [1, 2] {
            assert_eq!(
                canvas.snapshot(width, height, scale, &paint).unwrap(),
                canvas
                    .snapshot_scaled(width, height, Density::integer(scale).unwrap(), &paint)
                    .unwrap()
            );
        }
    }

    use super::*;
    use lucent_api::{Color, ImageData};

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn alpha_masks_match_rgba_coverage_and_cache_payloads() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let mut canvas = Canvas::new(gpu.clone());
        let coverage: Vec<u8> = (0..65 * 37).map(|n| (n * 73) as u8).collect();
        let rgba: Vec<u8> = coverage.iter().flat_map(|a| [255, 255, 255, *a]).collect();
        let mask = Arc::new(ImageData::from_alpha("same name", 65, 37, coverage).unwrap());
        let color = Arc::new(ImageData::from_rgba("same name", 65, 37, rgba).unwrap());
        assert_eq!(mask.bytes() * 4, color.bytes());
        let mut scenes = Vec::new();
        for data in [mask.clone(), color.clone()] {
            let mut paint = Vec::new();
            for (x, size, opacity, tint) in [
                (1.25, 6., 1., Color::hex(0xffffff)),
                (14.5, 16., 0.7, Color::hex(0x427fa8)),
                (38., 53., 0.9, Color::hex(0xf2718b).alpha(0.6)),
            ] {
                let rect = Rect::new(x, 3.25, size, size);
                paint.push(Paint::Image {
                    rect,
                    clip: Rect::new(0.5, 1.25, 91., 65.).into(),
                    data: data.clone(),
                    uv: Rect::new(0.125, 0.1, 0.75, 0.8),
                    radius: 2.5,
                    opacity,
                    tint,
                });
            }
            scenes.push(paint);
        }
        canvas.profiling = true;
        for scale in [1, 2] {
            let mask_pixels = canvas.snapshot(93, 68, scale, &scenes[0]).unwrap();
            let color_pixels = canvas.snapshot(93, 68, scale, &scenes[1]).unwrap();
            assert_eq!(
                mask_pixels, color_pixels,
                "mask changed coverage at scale {scale}"
            );
            assert!(
                mask_pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[3] > 0 && p[3] < 255)
            );
            assert_eq!(
                canvas.last_profile.unwrap().resources.image_misses,
                u64::from(scale == 1)
            );
        }
        assert_eq!(
            gpu.cache_stats().bytes,
            (mask.bytes() + color.bytes()) as u64
        );
        assert_eq!(gpu.cache_stats().entries, 2);
        canvas.snapshot(93, 68, 2, &scenes[0]).unwrap();
        assert_eq!(
            canvas.last_profile.unwrap().resources.texture_upload_bytes,
            0
        );
    }

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn scene_rejection_preserves_pixels_resources_and_subsequent_rendering() {
        let font = lucent_ui::text::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
            Default::default(),
        )
        .unwrap();
        let gpu = Gpu::headless(Arc::new(vec![font])).unwrap();
        // Force unused partial preparations out immediately, while the current
        // scene must remain pinned even over the reuse budget.
        *gpu.cache.borrow_mut() = ResourceCache::new(0, 0);
        let mut canvas = Canvas::new(gpu.clone());
        canvas.profiling = true;
        let rect = Rect::new(0., 0., 16., 16.);
        let image = |key, width, rgba: [u8; 4]| Paint::Image {
            rect,
            clip: rect.into(),
            uv: Rect::new(0., 0., 1., 1.),
            data: Arc::new(
                ImageData::from_rgba(key, width, 1, rgba.repeat(width as usize)).unwrap(),
            ),
            radius: 0.,
            opacity: 1.,
            tint: Color::hex(0xffffff),
        };
        let text = |size| Paint::Text {
            rect,
            clip: rect.into(),
            text: "W".into(),
            size,
            face: 0,
            color: Color::hex(0xffffff),
            align: Align::Start,
        };
        let texture = gpu.device.create_texture(&TextureDescriptor {
            label: Some("retained frame regression"),
            size: Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let read = |canvas: &Canvas| {
            canvas
                .readback(
                    &texture,
                    PhysicalExtent::new(16, 16, 1, 8192)
                        .unwrap()
                        .readback(4096)
                        .unwrap(),
                    Vec::with_capacity(16 * 16 * 4),
                )
                .unwrap()
        };
        canvas
            .paint(&view, 16, 16, 1, &[image("current", 1, [255, 0, 0, 255])])
            .unwrap();
        let expected = [255, 0, 0, 255].repeat(16 * 16);
        assert_eq!(read(&canvas), expected);
        let resident = Rc::as_ptr(&canvas.resources[0]);
        let buffer_bytes = canvas.buffer_payload_bytes();
        let cache_bytes = gpu.cache_stats().bytes;
        let oversized = gpu.device.limits().max_texture_dimension_2d + 1;
        assert!(
            oversized <= 16384,
            "fixture requires a larger ImageData envelope than the requested device"
        );
        for invalid in [
            image("too wide", oversized, [255; 4]),
            text(f32::NAN),
            Paint::Shape {
                rect: Rect::new(f32::MAX, 0., 16., 16.),
                clip: rect.into(),
                color: Color::hex(0xffffff),
                radius: 0.,
            },
        ] {
            let misses = gpu.cache_stats().misses;
            assert!(canvas.paint(&view, 16, 16, 1, &[invalid]).is_err());
            assert_eq!(
                gpu.cache_stats().misses,
                misses,
                "invalid input reached resource preparation"
            );
            assert_eq!(read(&canvas), expected);
            assert!(canvas.last_profile.is_none());
        }
        // This request passes cheap scene validation, but its glyph bitmap is
        // too large. An earlier, valid upload must be released after rejection.
        let error = canvas
            .paint(
                &view,
                16,
                16,
                1,
                &[image("partial", 2, [0, 255, 0, 255]), text(4096.)],
            )
            .unwrap_err();
        assert!(error.to_string().contains("glyph bitmap"), "{error}");
        assert_eq!(read(&canvas), expected);
        assert_eq!(canvas.buffer_payload_bytes(), buffer_bytes);
        assert_eq!(canvas.resources.len(), 1);
        assert_eq!(Rc::as_ptr(&canvas.resources[0]), resident);
        assert_eq!(gpu.cache_stats().entries, 1);
        assert_eq!(gpu.cache_stats().bytes, cache_bytes);
        assert!(canvas.last_profile.is_none());
        canvas
            .paint(&view, 16, 16, 1, &[image("next", 1, [0, 255, 0, 255])])
            .unwrap();
        assert_eq!(read(&canvas), [0, 255, 0, 255].repeat(16 * 16));
        assert_eq!(gpu.cache_stats().entries, 1);
    }

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn snapshot_rejects_invalid_extents_without_poisoning_the_device() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let limit = gpu.device.limits().max_texture_dimension_2d;
        let mut canvas = Canvas::new(gpu);
        for (width, height, scale) in [
            (0, 1, 1),
            (1, 0, 1),
            (1, 1, 0),
            (u32::MAX, 1, 2),
            (1, u32::MAX, 2),
            (limit + 1, 1, 1),
            (1, limit + 1, 1),
        ] {
            assert!(canvas.snapshot(width, height, scale, &[]).is_err());
        }
        // A non-aligned row verifies padding removal and channel order. Running
        // after rejection also detects accidental invalid commands reaching wgpu.
        let rect = Rect::new(0., 0., 65., 3.);
        let image =
            Arc::new(ImageData::from_rgba("readback", 1, 1, vec![0x12, 0x34, 0x56, 255]).unwrap());
        let paint = [Paint::Image {
            rect,
            clip: rect.into(),
            data: image,
            uv: Rect::new(0., 0., 1., 1.),
            radius: 0.,
            opacity: 1.,
            tint: Color::hex(0xffffff),
        }];
        for scale in [1, 2] {
            let pixels = canvas.snapshot(65, 3, scale, &paint).unwrap();
            assert_eq!(pixels.len(), (65 * 3 * scale * scale * 4) as usize);
            for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
                assert_eq!(
                    *pixel,
                    [0x12, 0x34, 0x56, 255],
                    "scale {scale}, pixel {index}"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn frame_profiling_distinguishes_cold_uploads_from_warm_draws() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let mut canvas = Canvas::new(gpu);
        let image = Arc::new(ImageData::from_rgba("profile", 8, 8, vec![255; 256]).unwrap());
        let rect = Rect::new(0., 0., 16., 16.);
        let paint = [
            Paint::Shape {
                rect,
                clip: rect.into(),
                color: Color::hex(0xffffff),
                radius: 0.,
            },
            Paint::Image {
                rect,
                clip: rect.into(),
                data: image.clone(),
                uv: Rect::new(0., 0., 1., 1.),
                radius: 0.,
                opacity: 1.,
                tint: Color::hex(0xffffff),
            },
        ];
        canvas.profiling = true;
        let cold_pixels = canvas.snapshot(16, 16, 1, &paint).unwrap();
        let cold = canvas.last_profile.unwrap();
        assert_eq!(cold.resources.image_misses, 1);
        assert_eq!(
            cold.resources.texture_upload_bytes,
            image.bytes() as u64 + 4
        );
        assert!(cold.resources.texture_upload <= cold.prepare);
        assert_eq!(cold.resources.text_raster, Duration::ZERO);
        assert!(canvas.buffer_payload_bytes() >= 16 + 96 * 2 + 32);
        let warm_pixels = canvas.snapshot(16, 16, 1, &paint).unwrap();
        let warm = canvas.last_profile.unwrap();
        assert_eq!(warm.resources.image_misses, 0);
        assert_eq!(warm.resources.texture_upload_bytes, 0);
        assert_eq!(warm.resources.texture_upload, Duration::ZERO);
        assert_eq!(cold_pixels, warm_pixels);
        canvas.profiling = false;
        assert_eq!(canvas.snapshot(16, 16, 1, &paint).unwrap(), cold_pixels);
        assert!(canvas.last_profile.is_none());
    }

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
                clip: rect.into(),
                uv: Rect::new(0., 0., 1., 1.),
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
    #[ignore = "requires a Vulkan driver"]
    fn renderer_surface_resources_follow_pixel_lifetime_across_clones_and_streams() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let mut left = Canvas::new(gpu.clone());
        let mut right = Canvas::new(gpu.clone());
        left.profiling = true;
        let rect = Rect::new(0., 0., 8., 8.);
        let paint = |data| {
            [Paint::Image {
                rect,
                clip: rect.into(),
                data,
                uv: Rect::new(0., 0., 1., 1.),
                radius: 0.,
                opacity: 1.,
                tint: Color::hex(0xffffff),
            }]
        };
        let image = ImageData::from_rgba("lifetime", 2, 2, [80, 40, 20, 255].repeat(4)).unwrap();
        let clone = image.clone();
        let lifetime = image.lifetime();
        let original = paint(Arc::new(image));
        let expected = left.snapshot(8, 8, 1, &original).unwrap();
        right.snapshot(8, 8, 1, &original).unwrap();
        drop(original);
        left.snapshot(8, 8, 1, &[]).unwrap();
        right.snapshot(8, 8, 1, &[]).unwrap();
        assert_eq!(gpu.cache_stats().active_bytes, 0);
        assert_eq!(
            gpu.cache_stats().entries,
            1,
            "a value clone can still reuse the image"
        );

        let alias = paint(Arc::new(clone));
        assert_eq!(left.snapshot(8, 8, 1, &alias).unwrap(), expected);
        assert_eq!(left.last_profile.unwrap().resources.texture_upload_bytes, 0);
        drop(alias);
        assert!(!lifetime.is_alive(), "the cache must not retain CPU pixels");
        right.snapshot(8, 8, 1, &[]).unwrap();
        assert_eq!(
            gpu.cache_stats().entries,
            1,
            "left's current draw remains pinned"
        );
        left.snapshot(8, 8, 1, &[]).unwrap();
        assert_eq!(gpu.cache_stats().entries, 0);
        assert_eq!(gpu.cache_stats().bytes, 0);
        assert_eq!(gpu.cache_stats().evictions, 1);

        let stream = lucent_api::ImageStream::new();
        let first = paint(Arc::new(
            stream.frame_rgba(2, 1, [255, 0, 0, 255].repeat(2)).unwrap(),
        ));
        left.snapshot(8, 8, 1, &first).unwrap();
        drop(first);
        let next = paint(Arc::new(
            stream.frame_rgba(2, 1, [0, 0, 255, 255].repeat(2)).unwrap(),
        ));
        let blue = left.snapshot(8, 8, 1, &next).unwrap();
        assert_eq!(left.last_profile.unwrap().resources.image_misses, 0);
        left.snapshot(8, 8, 1, &[]).unwrap();
        assert_eq!(
            gpu.cache_stats().entries,
            1,
            "reuse observes the latest frame's lifetime"
        );
        assert_eq!(left.snapshot(8, 8, 1, &next).unwrap(), blue);
        assert_eq!(left.last_profile.unwrap().resources.texture_upload_bytes, 0);
        drop(next);
        left.snapshot(8, 8, 1, &[]).unwrap();
        assert_eq!(gpu.cache_stats().entries, 0);
        assert_eq!(gpu.cache_stats().evictions, 2);
    }

    #[test]
    #[ignore = "requires a Vulkan driver; uses an intentionally over-budget texture"]
    fn renderer_surface_resources_remain_resident() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let rect = Rect::new(0., 0., 64., 64.);
        let image = |key: &str, size: u32| Paint::Image {
            rect,
            clip: rect.into(),
            uv: Rect::new(0., 0., 1., 1.),
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

#[cfg(test)]
mod clipping_tests {
    use super::*;
    use lucent_api::{Color, Element, ImageData};
    use lucent_ui::{Interaction, Layout};

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn diagonal_reveal_preserves_rounded_pixels_and_exact_endpoints_at_both_densities() {
        let fonts = Arc::new(vec![]);
        let gpu = Gpu::headless(fonts.clone()).unwrap();
        let layout = Layout::new(fonts);
        let mut canvas = Canvas::new(gpu);
        let image = Arc::new(
            ImageData::from_rgba("diagonal pixels", 1, 1, vec![220, 80, 40, 255]).unwrap(),
        );
        let tree = |progress| {
            let image = Element::<()>::image(image.clone())
                .size(49., 35.)
                .at(7., 5.)
                .radius(8.)
                .clip();
            let image = match progress {
                Some(progress) => image.diagonal_reveal(progress),
                None => image,
            };
            Element::stack(vec![image]).fill()
        };
        for scale in [1, 2] {
            let snapshot = |canvas: &mut Canvas, progress| {
                let scene = layout.build(&tree(progress), 67., 51., &Interaction::default(), 0.);
                canvas.snapshot(67, 51, scale, &scene.paint).unwrap()
            };
            let full = snapshot(&mut canvas, None);
            assert_eq!(snapshot(&mut canvas, Some(1.)), full, "full at {scale}x");
            assert!(
                snapshot(&mut canvas, Some(0.))
                    .iter()
                    .all(|byte| *byte == 0)
            );
            for progress in [0.2, 0.5, 0.8] {
                let pixels = snapshot(&mut canvas, Some(progress));
                let width = 67 * scale;
                let mut antialiased = false;
                for (index, (actual, original)) in pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(full.as_chunks::<4>().0)
                    .enumerate()
                {
                    let x = (index as u32 % width) as f32 + 0.5;
                    let y = (index as u32 / width) as f32 + 0.5;
                    let local = (x / scale as f32 - 7.) + (y / scale as f32 - 5.);
                    let distance = (local - progress * 84.) * scale as f32;
                    // The diagonal and rounded distances share a 2x2 fine
                    // derivative quad. At their intersection the combined AA
                    // fringe can change even when this sample is just beyond
                    // the diagonal's own fringe. Assert exact pixels outside
                    // that full neighborhood; endpoints above stay exact over
                    // every pixel, including their rounded boundaries.
                    if distance < -4. {
                        assert_eq!(actual, original, "visible at {index}, {scale}x, {progress}");
                    } else if distance > 4. {
                        assert_eq!(*actual, [0; 4], "masked at {index}, {scale}x, {progress}");
                    } else if original[3] == 255 && actual[3] > 0 && actual[3] < 255 {
                        antialiased = true;
                    }
                    assert!(
                        actual[3] <= original[3],
                        "reveal cannot expand rounded ancestors"
                    );
                }
                assert!(
                    antialiased,
                    "diagonal edge needs pixel coverage at {scale}x, {progress}"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn trimmed_cover_preserves_pixels_at_both_physical_densities() {
        let fonts = Arc::new(vec![]);
        let gpu = Gpu::headless(fonts.clone()).unwrap();
        let layout = Layout::new(fonts);
        let mut canvas = Canvas::new(gpu);
        let mut pixels = Vec::new();
        for y in 0..36 {
            for x in 0..64 {
                pixels.extend_from_slice(&[(x * 4) as u8, (y * 7) as u8, 100, 255]);
            }
        }
        let source = ImageData::from_rgba("density fixture", 64, 36, pixels).unwrap();
        for scale in [1, 2] {
            let trimmed = source.downsample_for_cover(16 * scale, 9 * scale).unwrap();
            assert_eq!((trimmed.width(), trimmed.height()), (16 * scale, 9 * scale));
            assert!(trimmed.bytes() < source.bytes());
            let render = |canvas: &mut Canvas, image| {
                let root = Element::<()>::image(Arc::new(image)).size(16., 9.).cover();
                let scene = layout.build(&root, 16., 9., &Interaction::default(), 0.);
                canvas.snapshot(16, 9, scale, &scene.paint).unwrap()
            };
            let original_pixels = render(&mut canvas, source.clone());
            let retained_pixels = render(&mut canvas, trimmed);
            assert_eq!(retained_pixels, original_pixels, "density {scale}");
        }
    }

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn opaque_images_retain_coverage_on_odd_sized_targets_and_clips() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let mut canvas = Canvas::new(gpu);
        let image =
            Arc::new(ImageData::from_rgba("odd edges", 1, 1, vec![255, 0, 0, 255]).unwrap());
        for (width, height) in [(1, 1), (3, 5), (17, 11), (65, 3), (67, 5)] {
            for scale in [1, 2] {
                for inset in [0., 0.75] {
                    let rect = Rect::new(0., 0., width as f32, height as f32);
                    let clip = Rect::new(inset, inset, rect.w - inset * 2., rect.h - inset * 2.);
                    if clip.w <= 0. || clip.h <= 0. {
                        continue;
                    }
                    let paint = [Paint::Image {
                        rect,
                        clip: clip.into(),
                        data: image.clone(),
                        uv: Rect::new(0., 0., 1., 1.),
                        radius: 0.,
                        opacity: 1.,
                        tint: Color::hex(0xffffff),
                    }];
                    let pixels = canvas.snapshot(width, height, scale, &paint).unwrap();
                    let physical_width = width * scale;
                    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
                        let x = (index as u32 % physical_width) as f32 + 0.5;
                        let y = (index as u32 / physical_width) as f32 + 0.5;
                        let expected = if clip.contains(x / scale as f32, y / scale as f32) {
                            [255, 0, 0, 255]
                        } else {
                            [0; 4]
                        };
                        assert_eq!(
                            *pixel, expected,
                            "{width}x{height}, scale {scale}, inset {inset}, pixel {index}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn edge_coverage_is_mirror_symmetric_at_both_densities() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let mut canvas = Canvas::new(gpu);
        for scale in [1, 2] {
            // Exercise both quad phases, straight/rounded primitives and rounded
            // ancestor clips. Reflection must not depend on a driver's choice of
            // coarse derivatives or which corner of a quad contains the fragment.
            for inset in [0., 3.] {
                for radius in [0., 6.] {
                    let rect = Rect::new(inset, inset, 32. - inset * 2., 24. - inset * 2.);
                    for rounded_clip in [false, true] {
                        let clip = if rounded_clip {
                            lucent_api::Clip::from(rect).with_rounded(rect, 6.)
                        } else {
                            rect.into()
                        };
                        let pixels = canvas
                            .snapshot(
                                32,
                                24,
                                scale,
                                &[Paint::Shape {
                                    rect,
                                    clip,
                                    radius,
                                    color: Color::hex(0xffffff),
                                }],
                            )
                            .unwrap();
                        let (w, h) = (32 * scale, 24 * scale);
                        assert_eq!(pixels[(((h / 2) * w + w / 2) * 4 + 3) as usize], 255);
                        if inset > 0. {
                            assert_eq!(pixels[3], 0);
                        }
                        for y in 0..h {
                            for x in 0..w {
                                let alpha = |x, y| pixels[((y * w + x) * 4 + 3) as usize];
                                let a = alpha(x, y);
                                assert!(
                                    a.abs_diff(alpha(w - 1 - x, y)) <= 2,
                                    "horizontal coverage at {x},{y}, scale {scale}, inset {inset}, radius {radius}, clip {rounded_clip}"
                                );
                                assert!(
                                    a.abs_diff(alpha(x, h - 1 - y)) <= 2,
                                    "vertical coverage at {x},{y}, scale {scale}, inset {inset}, radius {radius}, clip {rounded_clip}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn shadow_blur_is_supplied_by_the_client_and_scales_with_the_surface() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let mut canvas = Canvas::new(gpu);
        for scale in [1, 2] {
            let mut samples = Vec::new();
            for blur in [4., 12.] {
                let paint = [Paint::Shadow {
                    rect: Rect::new(16., 16., 24., 24.),
                    clip: Rect::new(0., 0., 64., 64.).into(),
                    color: Color(0., 0., 0., 1.),
                    radius: 0.,
                    blur,
                }];
                let pixels = canvas.snapshot(64, 64, scale, &paint).unwrap();
                let offset = ((28 * scale * 64 * scale + 48 * scale) * 4 + 3) as usize;
                samples.push(pixels[offset]);
            }
            assert!(
                samples[1] > samples[0].saturating_add(60),
                "blur was ignored: {samples:?}"
            );
        }
    }

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn slider_track_thumb_and_focus_render_at_both_densities() {
        use lucent_api::{Key, SliderSpec, SliderStyle};
        let fonts = Arc::new(vec![]);
        let gpu = Gpu::headless(fonts.clone()).unwrap();
        let layout = Layout::new(fonts);
        let mut canvas = Canvas::new(gpu);
        for value in [0., 0.5, 1.] {
            let root = Element::stack(vec![
                Element::slider(SliderSpec::new(value, 0.1).unwrap(), |event| event)
                    .id("level")
                    .size(112., 40.)
                    .at(8., 12.)
                    .padding_xy(8., 0.)
                    .radius(8.)
                    .slider_style(SliderStyle {
                        track: Color::hex(0x0000ff),
                        fill: Color::hex(0xff0000),
                        thumb: Color::hex(0xffffff),
                        track_height: 4.,
                        thumb_diameter: 16.,
                    }),
            ]);
            let mut interaction = Interaction::default();
            let scene = layout.build(&root, 128., 64., &interaction, 0.);
            interaction.key(&scene, &Key::Tab);
            let scene = layout.build(&root, 128., 64., &interaction, 0.);
            for scale in [1, 2] {
                let pixels = canvas.snapshot(128, 64, scale, &scene.paint).unwrap();
                let pixel = |x: u32, y: u32| {
                    let offset = ((y * scale * 128 * scale + x * scale) * 4) as usize;
                    &pixels[offset..offset + 4]
                };
                assert_eq!(pixel((24. + 80. * value) as u32, 32), &[255, 255, 255, 255]);
                assert_eq!(pixel(1, 1), &[0, 0, 0, 0]);
                assert!(pixel(64, 12)[3] > 0, "keyboard focus outline missing");
                let track = pixel(if value < 0.5 { 64 } else { 40 }, 32);
                assert_eq!(
                    track,
                    if value < 0.5 {
                        &[0, 0, 255, 255]
                    } else {
                        &[255, 0, 0, 255]
                    }
                );
            }
        }
    }

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn scroll_viewport_thumb_and_focus_match_input_at_both_densities() {
        use lucent_api::{Key, Length, ScrollSpec, ScrollStyle};
        let fonts = Arc::new(vec![]);
        let gpu = Gpu::headless(fonts.clone()).unwrap();
        let layout = Layout::new(fonts);
        let mut canvas = Canvas::new(gpu);
        for offset in [0., 28., 56.] {
            let content = Element::column(
                [0xff0000, 0x00ff00, 0x0000ff]
                    .into_iter()
                    .enumerate()
                    .map(|(n, color)| {
                        Element::empty()
                            .width(Length::Fill)
                            .height(Length::Fixed(40.))
                            .background(Color::hex(color))
                            .on_click(())
                            .id(format!("row-{n}"))
                    })
                    .collect(),
            )
            .width(Length::Fill)
            .gap(8.);
            let root = Element::stack(vec![
                Element::scroll(ScrollSpec::new(offset, 40.).unwrap(), content, |_| ())
                    .size(112., 96.)
                    .at(8., 8.)
                    .padding(8.)
                    .radius(40.)
                    .id("scroll")
                    .scroll_style(ScrollStyle {
                        thumb: Color::hex(0xffffff),
                        width: 4.,
                        gutter: 12.,
                        minimum_thumb: 16.,
                    }),
            ]);
            let mut input = Interaction::default();
            let scene = layout.build(&root, 128., 112., &input, 0.);
            let scroll = scene.hits[0].scroll.as_ref().unwrap();
            assert_eq!(scroll.maximum, 56.);
            for scale in [1, 2] {
                let pixels = canvas.snapshot(128, 112, scale, &scene.paint).unwrap();
                let pixel = |x: u32, y: u32| {
                    let p = ((y * scale * 128 * scale + x * scale) * 4) as usize;
                    &pixels[p..p + 4]
                };
                for (x, y) in [(1, 1), (16, 16), (16, 95), (64, 12), (64, 100), (104, 48)] {
                    assert_eq!(
                        pixel(x, y),
                        &[0, 0, 0, 0],
                        "clip/gutter at {x},{y}, offset {offset}, scale {scale}"
                    );
                    assert!(
                        !scene
                            .hits
                            .iter()
                            .skip(1)
                            .any(|h| h.contains(x as f32 + 0.5, y as f32 + 0.5))
                    );
                }
                assert_eq!(
                    pixel(110, (scroll.thumb.y + scroll.thumb.h / 2.) as u32),
                    &[255, 255, 255, 255]
                );
                for row in 0..3 {
                    let y = 36. + row as f32 * 48. - offset;
                    if !(18. ..94.).contains(&y) {
                        continue;
                    }
                    let mut expected = [0, 0, 0, 255];
                    expected[row] = 255;
                    assert_eq!(pixel(64, y as u32), &expected);
                    assert!(scene.hits[row + 1].contains(64., y));
                }
            }
            input.key(&scene, &Key::Tab);
            let focused = layout.build(&root, 128., 112., &input, 0.);
            for scale in [1, 2] {
                let pixels = canvas.snapshot(128, 112, scale, &focused.paint).unwrap();
                let p = ((8 * scale * 128 * scale + 64 * scale) * 4 + 3) as usize;
                assert!(pixels[p] > 0, "scroll viewport keyboard focus is visible");
            }
        }
    }

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn rounded_cover_and_nested_clips_match_pixels_and_input_at_both_densities() {
        let fonts = Arc::new(vec![]);
        let gpu = Gpu::headless(fonts.clone()).unwrap();
        let layout = Layout::new(fonts);
        let mut canvas = Canvas::new(gpu);
        let image = Arc::new(ImageData::from_rgba("wide fixture", 2, 1, vec![255; 8]).unwrap());
        let cover = Element::<()>::image(image)
            .size(64., 64.)
            .cover()
            .radius(16.)
            .on_click(());
        let nested = Element::stack(vec![
            Element::stack(vec![
                Element::new(lucent_api::Kind::Empty)
                    .size(64., 64.)
                    .background(Color::hex(0xffffff))
                    .on_click(()),
            ])
            .size(48., 48.)
            .at(8., 4.)
            .radius(16.)
            .clip(),
        ])
        .size(64., 64.)
        .radius(24.)
        .clip();
        for root in [cover, nested] {
            let scene = layout.build(&root, 64., 64., &Interaction::default(), 0.);
            for scale in [1, 2] {
                let pixels = canvas.snapshot(64, 64, scale, &scene.paint).unwrap();
                let alpha = |x: usize, y: usize| pixels[(y * 64 * scale as usize + x) * 4 + 3];
                assert_eq!(
                    alpha(scale as usize, scale as usize),
                    0,
                    "rounded corner was filled"
                );
                assert_eq!(alpha(32 * scale as usize, 32 * scale as usize), 255);
                // Away from anti-aliased boundaries, GPU coverage, hit testing
                // and the native-region scanline contract must all agree.
                for (x, y) in [(1., 1.), (9., 5.), (32., 32.), (60., 60.)] {
                    let hit = scene.hits.iter().any(|hit| hit.contains(x, y));
                    let region = scene.regions.iter().any(|clip| clip.contains(x, y));
                    assert_eq!(hit, region);
                    let opaque =
                        alpha((x * scale as f32) as usize, (y * scale as f32) as usize) > 127;
                    assert_eq!(
                        hit, opaque,
                        "input/pixel disagreement at {x},{y}, scale {scale}"
                    );
                }
            }
        }
        // Cover must crop the source around its center, without stretching its
        // aspect ratio or rounding the oversized source rectangle instead.
        let mut pixels = Vec::new();
        for _ in 0..4 {
            for x in 0..8 {
                pixels.extend_from_slice(if (2..6).contains(&x) {
                    &[0, 255, 0, 255]
                } else {
                    &[255, 0, 0, 255]
                });
            }
        }
        let image = Arc::new(ImageData::from_rgba("crop fixture", 8, 4, pixels).unwrap());
        let root = Element::<()>::image(image).size(64., 64.).cover();
        let scene = layout.build(&root, 64., 64., &Interaction::default(), 0.);
        for scale in [1, 2] {
            let pixels = canvas.snapshot(64, 64, scale, &scene.paint).unwrap();
            for x in [16, 48] {
                let offset = ((32 * scale * 64 * scale + x * scale) * 4) as usize;
                assert_eq!(&pixels[offset..offset + 4], &[0, 255, 0, 255]);
            }
            assert_eq!(pixels[3], 255, "unrounded image edges must not fade");
        }
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
    pub last_profile: Option<RenderProfile>,
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
            last_profile: None,
        })
    }
    pub fn buffer_payload_bytes(&self) -> u64 {
        self.canvas.buffer_payload_bytes()
    }
    /// Color payload of one configured surface buffer. The driver determines how
    /// many buffers it actually allocates; this is not total swapchain/GPU memory.
    pub fn single_surface_buffer_bytes(&self) -> u64 {
        u64::from(self.config.width) * u64::from(self.config.height) * 4
    }
    /// Disabled by default. Enabling records CPU calls for successful presentations.
    pub fn enable_profiling(&mut self, enabled: bool) {
        self.canvas.profiling = enabled;
        if !enabled {
            self.last_profile = None;
        }
    }
    pub fn render(
        &mut self,
        width: u32,
        height: u32,
        scale: u32,
        paint: &[Paint],
        before_present: impl FnOnce(),
    ) -> Result<RenderOutcome, Error> {
        if scale == 0 {
            return Ok(RenderOutcome::RetryAfter(self.recovery.retry()));
        }
        self.render_scaled(
            width,
            height,
            Density::integer(scale)?,
            paint,
            before_present,
        )
    }
    pub fn render_scaled(
        &mut self,
        width: u32,
        height: u32,
        density: Density,
        paint: &[Paint],
        before_present: impl FnOnce(),
    ) -> Result<RenderOutcome, Error> {
        let enabled = self.canvas.profiling;
        let total = profiling::start(enabled);
        let mut profile = RenderProfile::default();
        self.last_profile = None;
        if let Some(error) = self
            .gpu
            .device_failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Err(error.into());
        }
        if width == 0 || height == 0 {
            return Ok(RenderOutcome::RetryAfter(self.recovery.retry()));
        }
        let physical = PhysicalExtent::density(
            width,
            height,
            density,
            self.gpu.device.limits().max_texture_dimension_2d,
        )?;
        let validated = self.canvas.validate_scene(width, height, density, paint)?;
        let started = profiling::start(enabled);
        if self.reconfigure
            || self.config.width != physical.width
            || self.config.height != physical.height
        {
            self.config.width = physical.width;
            self.config.height = physical.height;
            self.surface.configure(&self.gpu.device, &self.config);
            self.reconfigure = false;
        }
        profile.configure = profiling::elapsed(started);
        let started = profiling::start(enabled);
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
        profile.acquire = profiling::elapsed(started);
        self.canvas
            .paint_validated(&frame.texture.create_view(&Default::default()), validated)?;
        profile.paint = self.canvas.last_profile.unwrap_or_default();
        let started = profiling::start(enabled);
        // Request a Wayland callback only when a buffer will actually be committed.
        before_present();
        self.gpu.queue.present(frame);
        profile.present = profiling::elapsed(started);
        profile.total = profiling::elapsed(total);
        self.last_profile = enabled.then_some(profile);
        self.frames += 1;
        self.recovery.presented();
        Ok(RenderOutcome::Presented)
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn frozen_stream_snapshot_keeps_pixels_while_shared_live_texture_advances() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let mut transition = Canvas::new(gpu.clone());
        let mut live_output = Canvas::new(gpu.clone());
        transition.profiling = true;
        live_output.profiling = true;
        let stream = lucent_api::ImageStream::new();
        let first = stream.frame_rgba(2, 1, [255, 0, 0, 255].repeat(2)).unwrap();
        let frozen = Arc::new(first.snapshot());
        let full = Rect::new(0., 0., 8., 4.);
        let paint = |data: Arc<lucent_api::ImageData>, rect: Rect, opacity| Paint::Image {
            rect,
            clip: full.into(),
            data,
            uv: Rect::new(0., 0., 1., 1.),
            radius: 0.,
            opacity,
            tint: lucent_api::Color::hex(0xffffff),
        };
        for scale in [1, 2] {
            for color in [[0, 255, 0, 255], [0, 0, 255, 255]] {
                let current = Arc::new(stream.frame_rgba(2, 1, color.repeat(2)).unwrap());
                let live_scene = [paint(current.clone(), full, 1.)];
                let live_pixels = live_output.snapshot(8, 4, scale, &live_scene).unwrap();
                assert!(
                    live_pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|pixel| *pixel == color)
                );
                let split_scene = [
                    paint(frozen.clone(), Rect::new(0., 0., 4., 4.), 1.),
                    paint(current.clone(), Rect::new(4., 0., 4., 4.), 1.),
                ];
                let split = transition.snapshot(8, 4, scale, &split_scene).unwrap();
                for (index, pixel) in split.as_chunks::<4>().0.iter().enumerate() {
                    let x = index % (8 * scale) as usize;
                    assert_eq!(
                        *pixel,
                        if x < (4 * scale) as usize {
                            [255, 0, 0, 255]
                        } else {
                            color
                        }
                    );
                }
                // The outgoing snapshot and the live frame use two resources,
                // while both live scenes share the single current stream slot.
                assert_eq!(gpu.cache_stats().entries, 2);
                assert_eq!(
                    transition
                        .last_profile
                        .unwrap()
                        .resources
                        .texture_upload_bytes,
                    if scale == 1 && color == [0, 255, 0, 255] {
                        8
                    } else {
                        0
                    }
                );
                let again = live_output.snapshot(8, 4, scale, &live_scene).unwrap();
                assert_eq!(again, live_pixels);
                assert_eq!(
                    live_output
                        .last_profile
                        .unwrap()
                        .resources
                        .texture_upload_bytes,
                    0
                );
                let layered_scene = [paint(frozen.clone(), full, 1.), paint(current, full, 0.5)];
                let blend = transition.snapshot(8, 4, scale, &layered_scene).unwrap();
                assert!(blend.as_chunks::<4>().0.iter().all(|pixel| pixel[0] > 0
                    && pixel[0] < 255
                    && pixel[3] == 255
                    && if color[1] == 255 {
                        pixel[1] > 0 && pixel[2] == 0
                    } else {
                        pixel[2] > 0 && pixel[1] == 0
                    }));
                assert_eq!(
                    transition
                        .last_profile
                        .unwrap()
                        .resources
                        .texture_upload_bytes,
                    0
                );
            }
        }
    }

    #[test]
    #[ignore = "requires a Vulkan driver"]
    fn streamed_images_reuse_one_gpu_texture_and_update_real_pixels() {
        let gpu = Gpu::headless(Arc::new(vec![])).unwrap();
        let mut canvas = Canvas::new(gpu.clone());
        canvas.profiling = true;
        let stream = lucent_api::ImageStream::new();
        let rect = Rect::new(0., 0., 8., 8.);
        for (index, color) in [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
            .into_iter()
            .enumerate()
        {
            let image = Arc::new(stream.frame_rgba(2, 1, color.repeat(2)).unwrap());
            let paint = [Paint::Image {
                rect,
                clip: rect.into(),
                data: image.clone(),
                uv: Rect::new(0., 0., 1., 1.),
                radius: 0.,
                opacity: 1.,
                tint: lucent_api::Color::hex(0xffffff),
            }];
            let pixels = canvas.snapshot(8, 8, 1, &paint).unwrap();
            assert!(
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == color)
            );
            let profile = canvas.last_profile.unwrap();
            assert_eq!(
                profile.resources.image_misses,
                usize::from(index == 0) as u64
            );
            assert_eq!(profile.resources.texture_upload_bytes, 8);
            let warm = canvas.snapshot(8, 8, 1, &paint).unwrap();
            assert_eq!(warm, pixels);
            assert_eq!(
                canvas.last_profile.unwrap().resources.texture_upload_bytes,
                0
            );
            assert_eq!(gpu.cache_stats().entries, 1);
        }
        let changed = Arc::new(stream.frame_rgba(1, 1, vec![255, 255, 0, 255]).unwrap());
        let paint = [Paint::Image {
            rect,
            clip: rect.into(),
            data: changed,
            uv: Rect::new(0., 0., 1., 1.),
            radius: 0.,
            opacity: 1.,
            tint: lucent_api::Color::hex(0xffffff),
        }];
        let pixels = canvas.snapshot(8, 8, 2, &paint).unwrap();
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [255, 255, 0, 255])
        );
        assert_eq!(gpu.cache_stats().entries, 1);
    }
}
