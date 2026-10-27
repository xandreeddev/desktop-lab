//! Native planes use normal scene geometry/clips and the same bounded cache.
//! GPU resources contain no Rc<Gpu> and retain no CPU pixels after submission.
use super::*;
use lucent_api::VideoData;

pub(super) struct Pipeline {
    pub pipeline: RenderPipeline,
    layout: BindGroupLayout,
}
impl Pipeline {
    fn new(gpu: &Gpu) -> Self {
        let layout = planes::layout(gpu);
        let shader = gpu.device.create_shader_module(ShaderModuleDescriptor {
            label: Some("native video scene"),
            source: ShaderSource::Wgsl(
                concat!(include_str!("video_color.wgsl"), include_str!("scene.wgsl")).into(),
            ),
        });
        let pipeline = scene_pipeline(&gpu.device, &layout, &gpu.frame_layout, &shader);
        Self { pipeline, layout }
    }
}
struct Update {
    texture: Rc<TextureResource>,
    data: VideoData,
    revision: u64,
}
#[derive(Default)]
pub(super) struct Updates(Vec<Update>);
impl Updates {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn reserve(&mut self, count: usize) -> Result<(), Error> {
        self.0.try_reserve_exact(count)?;
        Ok(())
    }
    fn revision(&self, texture: &Rc<TextureResource>) -> u64 {
        self.0
            .iter()
            .find(|update| Rc::ptr_eq(&update.texture, texture))
            .map_or_else(|| texture.revision.get(), |update| update.revision)
    }
    fn defer(&mut self, texture: &Rc<TextureResource>, data: &VideoData, revision: u64) {
        if let Some(update) = self
            .0
            .iter_mut()
            .find(|update| Rc::ptr_eq(&update.texture, texture))
        {
            update.data = data.clone();
            update.revision = revision;
        } else {
            self.0.push(Update {
                texture: texture.clone(),
                data: data.clone(),
                revision,
            });
        }
    }
    pub fn encode(&self, gpu: &Gpu) -> u64 {
        self.0
            .iter()
            .map(|update| match &update.texture.storage {
                TextureStorage::Video(planes) => planes.upload(gpu, update.data.frame()),
                TextureStorage::Image(_) => unreachable!("image resource in native upload"),
            })
            .sum()
    }
    pub fn commit(self) {
        for update in self.0 {
            update.texture.revision.set(update.revision);
            *update.texture.lifetime.borrow_mut() =
                Some(ResourceLifetime::Video(update.data.lifetime()));
        }
    }
}
impl Gpu {
    pub(super) fn video(
        &self,
        data: &VideoData,
        profile: &mut ResourceProfile,
        enabled: bool,
        updates: &mut Updates,
    ) -> Rc<TextureResource> {
        let streamed = data.stream_revision();
        let key = streamed.map_or_else(
            || format!("video:{}", data.cache_id()),
            |(id, _)| format!("video-stream:{id}"),
        );
        if let Some(texture) = self.cache.borrow_mut().get(&key) {
            if texture.width == data.width() as f32 && texture.height == data.height() as f32 {
                let revision = streamed.map_or(1, |(_, revision)| revision);
                if updates.revision(&texture) != revision {
                    updates.defer(&texture, data, revision);
                }
                return texture;
            }
        }
        if enabled {
            profile.image_misses += 1;
        }
        let mut pipeline = self.video_pipeline.borrow_mut();
        let pipeline = pipeline.get_or_insert_with(|| Pipeline::new(self));
        let planes = planes::PlaneResources::new(self, &pipeline.layout, data.frame());
        let bytes = planes.payload_bytes();
        let texture = Rc::new(TextureResource {
            group: planes.group.clone(),
            width: data.width() as f32,
            height: data.height() as f32,
            storage: TextureStorage::Video(planes),
            revision: Cell::new(0),
            stream_id: Cell::new(streamed.map(|(id, _)| id)),
            lifetime: RefCell::new(Some(ResourceLifetime::Video(data.lifetime()))),
            bytes,
            text_origin_x: 0.,
        });
        self.cache.borrow_mut().insert(key, texture.clone(), bytes);
        updates.defer(&texture, data, streamed.map_or(1, |(_, revision)| revision));
        texture
    }
}
#[cfg(test)]
mod tests;
