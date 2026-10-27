//! Immutable video presentation descriptors. Codec ownership remains hidden in
//! the neutral plane bytes; neither this API nor views perform decoding or I/O.
use crate::video_frame::Yuv420Frame;
use std::sync::{Arc, Weak};

#[derive(Clone, Debug)]
pub struct VideoData {
    frame: Arc<Yuv420Frame>,
    id: u64,
    stream: Option<(u64, u64)>,
}
#[derive(Clone, Debug)]
pub struct VideoLifetime(Weak<Yuv420Frame>);
impl VideoLifetime {
    /// Observes the shared frame owner without retaining any plane bytes.
    pub fn is_alive(&self) -> bool {
        self.0.strong_count() > 0
    }
}
impl VideoData {
    pub(crate) fn prepared(frame: Yuv420Frame, id: u64, stream: (u64, u64)) -> Self {
        Self {
            frame: Arc::new(frame),
            id,
            stream: Some(stream),
        }
    }
    pub fn frame(&self) -> &Yuv420Frame {
        &self.frame
    }
    pub fn width(&self) -> u32 {
        self.frame.dimensions().0
    }
    pub fn height(&self) -> u32 {
        self.frame.dimensions().1
    }
    /// This checked 8-bit planar contract uses square pixels and no crop.
    pub fn aspect_size(&self) -> (u32, u32) {
        self.frame.dimensions()
    }
    pub fn cache_id(&self) -> u64 {
        self.id
    }
    pub fn stream_revision(&self) -> Option<(u64, u64)> {
        self.stream
    }
    pub fn lifetime(&self) -> VideoLifetime {
        VideoLifetime(Arc::downgrade(&self.frame))
    }
    pub fn bytes(&self) -> usize {
        self.frame.byte_len()
    }
    /// Freeze GPU identity while sharing the same immutable plane owners.
    pub fn snapshot(&self) -> Self {
        Self {
            frame: self.frame.clone(),
            id: self.id,
            stream: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ImageStream,
        video_frame::{Bt709, Bytes, ChromaSiting, Plane, Range, Transfer},
    };
    fn frame() -> Yuv420Frame {
        let p = || Plane::new(Bytes::from(vec![128]), 1, 1, 1).unwrap();
        Yuv420Frame::new(
            1,
            1,
            [p(), p(), p()],
            Bt709 {
                range: Range::Full,
                transfer: Transfer::Srgb,
                chroma: ChromaSiting::Center,
            },
        )
        .unwrap()
    }
    #[test]
    fn video_and_rgba_share_checked_stream_revisions_without_changing_immutable_identity() {
        let stream = ImageStream::new();
        let clone = stream.clone();
        let first = stream.frame_rgba(1, 1, vec![1, 2, 3, 255]).unwrap();
        let second = clone.frame_yuv420(frame()).unwrap();
        let third = stream.frame_rgba(1, 1, vec![4, 5, 6, 255]).unwrap();
        let id = first.stream_revision().unwrap().0;
        assert_eq!(first.stream_revision(), Some((id, 1)));
        assert_eq!(second.stream_revision(), Some((id, 2)));
        assert_eq!(third.stream_revision(), Some((id, 3)));
        assert_ne!(first.cache_id(), second.cache_id());
        assert_ne!(second.cache_id(), third.cache_id());
    }
    #[test]
    fn frozen_video_preserves_planes_and_lifetime_without_a_mutable_stream_identity() {
        let source = frame();
        let pointer = source.planes()[0].bytes().as_ptr();
        let data = ImageStream::new().frame_yuv420(source).unwrap();
        let life = data.lifetime();
        let frozen = data.snapshot();
        assert!(life.is_alive());
        assert_eq!(data.cache_id(), frozen.cache_id());
        assert!(frozen.stream_revision().is_none());
        assert_eq!(frozen.frame().planes()[0].bytes().as_ptr(), pointer);
        drop(data);
        assert!(life.is_alive());
        assert_eq!(frozen.bytes(), 3);
        drop(frozen);
        assert!(!life.is_alive());
    }
}
