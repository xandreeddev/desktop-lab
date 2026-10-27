//! Opt-in native video delivery. No codec/GPU/OS handles cross this boundary.
use crate::{DomainError, MediaFrame, Result, WallpaperFrameRate};
pub use lucent_video_frame as video_frame;
use std::time::Duration;
pub use video_frame::Yuv420Frame;

#[derive(Clone, Copy, Debug)]
pub struct NativePlaybackOptions {
    pub fps: WallpaperFrameRate,
    pub looping: bool,
}
impl Default for NativePlaybackOptions {
    fn default() -> Self {
        Self {
            fps: WallpaperFrameRate::Source,
            looping: true,
        }
    }
}
/// Publication work only. These counters are not decoder allocation or RSS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanePublication {
    pub referenced_bytes: usize,
    pub copied_bytes: usize,
}
#[derive(Debug)]
pub enum NativeMediaFrame {
    Rgba(MediaFrame),
    Yuv420 {
        frame: Yuv420Frame,
        timestamp: Duration,
        publication: PlanePublication,
    },
}
impl NativeMediaFrame {
    pub fn timestamp(&self) -> Duration {
        match self {
            Self::Rgba(frame) => frame.timestamp,
            Self::Yuv420 { timestamp, .. } => *timestamp,
        }
    }
    pub fn into_rgba(self) -> Result<MediaFrame> {
        match self {
            Self::Rgba(frame) => Ok(frame),
            Self::Yuv420 { .. } => Err(DomainError::Invalid(
                "Planar delivery entered an RGBA-only path".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        LiveWallpaperAsset, PlaybackOptions, StopSignal, Wallpaper, WallpaperFps,
        WallpaperPlaybackPort,
    };
    use std::sync::{Arc, Mutex};

    struct Never;
    impl StopSignal for Never {
        fn cancelled(&self) -> bool {
            false
        }
        fn wait(&self, _: Duration) {}
    }
    struct RgbaAdapter(Mutex<Option<PlaybackOptions>>);
    impl WallpaperPlaybackPort for RgbaAdapter {
        fn prepare(&self, _: &Wallpaper) -> Result<LiveWallpaperAsset> {
            unreachable!("playback does not prepare")
        }
        fn play(
            &self,
            _: &LiveWallpaperAsset,
            options: PlaybackOptions,
            emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
            _: Arc<dyn StopSignal>,
        ) {
            *self.0.lock().unwrap() = Some(options);
            assert!(!emit(Ok(MediaFrame::rgba(
                1,
                1,
                vec![1, 2, 3, 255],
                Duration::from_millis(7)
            )
            .unwrap())));
        }
    }
    #[test]
    fn native_port_default_delegates_to_existing_rgba_with_exact_source_cadence_options() {
        let adapter = RgbaAdapter(Mutex::new(None));
        let source = Wallpaper {
            path: "/fixture/source.mp4".into(),
            name: "Fixture".into(),
        };
        let asset = LiveWallpaperAsset {
            source: source.clone(),
            path: source.path.clone(),
            poster: source,
        };
        let mut seen = 0;
        adapter.play_native(
            &asset,
            NativePlaybackOptions {
                fps: WallpaperFrameRate::Capped(WallpaperFps::new(60).unwrap()),
                looping: false,
            },
            &mut |value| {
                let frame = value.unwrap();
                assert_eq!(frame.timestamp(), Duration::from_millis(7));
                assert_eq!(
                    frame.into_rgba().unwrap().into_rgba().2.as_slice(),
                    &[1, 2, 3, 255]
                );
                seen += 1;
                false
            },
            Arc::new(Never),
        );
        assert_eq!(seen, 1);
        let options = adapter.0.lock().unwrap().unwrap();
        assert!(options.target.is_none());
        assert!(!options.looping);
        assert_eq!(
            options.fps,
            WallpaperFrameRate::Capped(WallpaperFps::new(60).unwrap())
        );
    }
    #[test]
    fn native_planar_delivery_retains_timestamp_and_cannot_enter_an_rgba_only_consumer() {
        use video_frame::{Bt709, Bytes, ChromaSiting, Plane, Range, Transfer};
        let plane = || Plane::new(Bytes::from(vec![128]), 1, 1, 1).unwrap();
        let frame = Yuv420Frame::new(
            1,
            1,
            [plane(), plane(), plane()],
            Bt709 {
                range: Range::Full,
                transfer: Transfer::Srgb,
                chroma: ChromaSiting::Center,
            },
        )
        .unwrap();
        let value = NativeMediaFrame::Yuv420 {
            frame,
            timestamp: Duration::from_millis(9),
            publication: PlanePublication {
                referenced_bytes: 0,
                copied_bytes: 3,
            },
        };
        assert_eq!(value.timestamp(), Duration::from_millis(9));
        assert!(value.into_rgba().is_err());
    }
}
