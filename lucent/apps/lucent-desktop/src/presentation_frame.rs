//! Client presentation state only. No codec, service operation or GPU handle.
use lucent_api::{Element, ImageData, VideoData};
use std::sync::Arc;
#[derive(Clone, Debug)]
pub(crate) enum PresentationFrame {
    Image(Arc<ImageData>),
    Video {
        data: Arc<VideoData>,
        poster: Arc<ImageData>,
    },
}
impl From<Arc<ImageData>> for PresentationFrame {
    fn from(value: Arc<ImageData>) -> Self {
        Self::Image(value)
    }
}
impl PresentationFrame {
    pub fn native(data: Arc<VideoData>, poster: Arc<ImageData>) -> Self {
        Self::Video { data, poster }
    }
    /// Pure client admission policy. Choosing a previously prepared fallback
    /// performs no fitting, allocation of pixels, decoding or service operation.
    pub fn select_for_target(&self, target: Option<lucent_domain::WallpaperPixelTarget>) -> Self {
        match self {
            Self::Video { data, poster }
                if target.is_none_or(|target| {
                    !target
                        .cover_size(data.width(), data.height())
                        .is_ok_and(|size| size == (data.width(), data.height()))
                }) =>
            {
                Self::Image(poster.clone())
            }
            _ => self.clone(),
        }
    }

    pub fn as_image(&self) -> Option<&Arc<ImageData>> {
        match self {
            Self::Image(value) => Some(value),
            Self::Video { .. } => None,
        }
    }
    pub fn element<M>(&self) -> Element<M> {
        match self {
            Self::Image(value) => Element::image(value.clone()),
            Self::Video { data, .. } => Element::video(data.clone()),
        }
    }
    pub fn cache_id(&self) -> u64 {
        match self {
            Self::Image(value) => value.cache_id(),
            Self::Video { data, .. } => data.cache_id(),
        }
    }
    pub fn stream_revision(&self) -> Option<(u64, u64)> {
        match self {
            Self::Image(value) => value.stream_revision(),
            Self::Video { data, .. } => data.stream_revision(),
        }
    }
    pub fn width(&self) -> u32 {
        match self {
            Self::Image(value) => value.width(),
            Self::Video { data, .. } => data.width(),
        }
    }
    pub fn height(&self) -> u32 {
        match self {
            Self::Image(value) => value.height(),
            Self::Video { data, .. } => data.height(),
        }
    }
    pub fn aspect_size(&self) -> (u32, u32) {
        match self {
            Self::Image(value) => value.aspect_size(),
            Self::Video { data, .. } => data.aspect_size(),
        }
    }
    pub fn bytes(&self) -> usize {
        match self {
            Self::Image(value) => value.bytes(),
            Self::Video { data, poster } => data.bytes() + poster.bytes(),
        }
    }
    pub fn is_opaque(&self) -> bool {
        match self {
            Self::Image(value) => value.is_opaque(),
            Self::Video { .. } => true,
        }
    }
    pub fn representation(&self) -> &'static str {
        match self {
            Self::Image(_) => "rgba",
            Self::Video { .. } => "yuv420",
        }
    }
    pub fn snapshot(&self) -> Self {
        if self.stream_revision().is_none() {
            return self.clone();
        }
        match self {
            Self::Image(value) => Self::Image(Arc::new(value.snapshot())),
            Self::Video { data, poster } => Self::native(Arc::new(data.snapshot()), poster.clone()),
        }
    }
}
