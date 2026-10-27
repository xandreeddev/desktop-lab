//! Client presentation state only. No codec, service operation or GPU handle.
use lucent_api::{Element, ImageData, VideoData};
use std::sync::Arc;
#[derive(Clone, Debug)]
pub(crate) enum PresentationFrame {
    Image(Arc<ImageData>),
    Video(Arc<VideoData>),
}
impl From<Arc<ImageData>> for PresentationFrame {
    fn from(value: Arc<ImageData>) -> Self {
        Self::Image(value)
    }
}
impl From<Arc<VideoData>> for PresentationFrame {
    fn from(value: Arc<VideoData>) -> Self {
        Self::Video(value)
    }
}
impl PresentationFrame {
    pub fn as_image(&self) -> Option<&Arc<ImageData>> {
        match self {
            Self::Image(value) => Some(value),
            Self::Video(_) => None,
        }
    }
    pub fn element<M>(&self) -> Element<M> {
        match self {
            Self::Image(value) => Element::image(value.clone()),
            Self::Video(value) => Element::video(value.clone()),
        }
    }
    pub fn cache_id(&self) -> u64 {
        match self {
            Self::Image(value) => value.cache_id(),
            Self::Video(value) => value.cache_id(),
        }
    }
    pub fn stream_revision(&self) -> Option<(u64, u64)> {
        match self {
            Self::Image(value) => value.stream_revision(),
            Self::Video(value) => value.stream_revision(),
        }
    }
    pub fn width(&self) -> u32 {
        match self {
            Self::Image(value) => value.width(),
            Self::Video(value) => value.width(),
        }
    }
    pub fn height(&self) -> u32 {
        match self {
            Self::Image(value) => value.height(),
            Self::Video(value) => value.height(),
        }
    }
    pub fn aspect_size(&self) -> (u32, u32) {
        match self {
            Self::Image(value) => value.aspect_size(),
            Self::Video(value) => value.aspect_size(),
        }
    }
    pub fn bytes(&self) -> usize {
        match self {
            Self::Image(value) => value.bytes(),
            Self::Video(value) => value.bytes(),
        }
    }
    pub fn is_opaque(&self) -> bool {
        match self {
            Self::Image(value) => value.is_opaque(),
            Self::Video(_) => true,
        }
    }
    pub fn representation(&self) -> &'static str {
        match self {
            Self::Image(_) => "rgba",
            Self::Video(_) => "yuv420",
        }
    }
    pub fn snapshot(&self) -> Self {
        if self.stream_revision().is_none() {
            return self.clone();
        }
        match self {
            Self::Image(value) => Self::Image(Arc::new(value.snapshot())),
            Self::Video(value) => Self::Video(Arc::new(value.snapshot())),
        }
    }
}
