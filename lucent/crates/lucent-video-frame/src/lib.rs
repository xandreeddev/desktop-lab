//! Immutable, checked video data. No codec, OS, GPU or client dependencies.
//! Byte owners must expose initialized bytes; native adapters never expose
//! uninitialized row padding. Existing RGBA image contracts are independent.
pub use bytes::Bytes;
use std::fmt;

pub const MAX_EDGE: u32 = 4096;
const MAX_BYTES: usize = MAX_EDGE as usize * MAX_EDGE as usize * 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameError(pub &'static str);
impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for FrameError {}
type Result<T> = std::result::Result<T, FrameError>;

/// Row stride may include initialized padding. The final row need only contain
/// its visible samples. All layout arithmetic is checked before publication.
#[derive(Clone)]
pub struct Plane {
    data: Bytes,
    width: u32,
    height: u32,
    stride: u32,
}
impl fmt::Debug for Plane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plane")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("stride", &self.stride)
            .field("bytes", &self.data.len())
            .finish()
    }
}
impl Plane {
    pub fn new(data: Bytes, width: u32, height: u32, stride: u32) -> Result<Self> {
        if width == 0
            || height == 0
            || width > MAX_EDGE
            || height > MAX_EDGE
            || stride < width
            || stride as usize > MAX_BYTES
        {
            return Err(FrameError("Invalid video plane dimensions or stride"));
        }
        let required = (height as usize - 1)
            .checked_mul(stride as usize)
            .and_then(|n| n.checked_add(width as usize))
            .ok_or(FrameError("Video plane extent overflow"))?;
        if required > MAX_BYTES || data.len() < required || data.len() > MAX_BYTES {
            return Err(FrameError("Invalid video plane storage extent"));
        }
        Ok(Self {
            data: data.slice(..required),
            width,
            height,
            stride,
        })
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn stride(&self) -> u32 {
        self.stride
    }
    pub fn bytes(&self) -> &Bytes {
        &self.data
    }
    pub fn row(&self, index: u32) -> Option<&[u8]> {
        if index >= self.height {
            return None;
        }
        let begin = index as usize * self.stride as usize;
        self.data.get(begin..begin + self.width as usize)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Limited,
    Full,
}
/// Both transfers use BT.709/sRGB primaries and D65 white. Other primaries,
/// matrices and HDR transfers are outside this initial native capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    Bt709,
    Srgb,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromaSiting {
    Left,
    Center,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bt709 {
    pub range: Range,
    pub transfer: Transfer,
    pub chroma: ChromaSiting,
}
/// The matrix is explicitly BT.709; an unspecified/other matrix cannot be
/// mistaken for supported metadata by a default enum value.
#[derive(Clone, Debug)]
pub struct Yuv420Frame {
    width: u32,
    height: u32,
    planes: [Plane; 3],
    color: Bt709,
}
impl Yuv420Frame {
    pub fn new(width: u32, height: u32, planes: [Plane; 3], color: Bt709) -> Result<Self> {
        if width == 0 || height == 0 || width > MAX_EDGE || height > MAX_EDGE {
            return Err(FrameError("Invalid video frame dimensions"));
        }
        let chroma = (width.div_ceil(2), height.div_ceil(2));
        if planes[0].dimensions() != (width, height)
            || planes[1].dimensions() != chroma
            || planes[2].dimensions() != chroma
        {
            return Err(FrameError("YUV420 plane geometry does not match frame"));
        }
        let bytes = planes
            .iter()
            .try_fold(0usize, |n, p| n.checked_add(p.bytes().len()))
            .ok_or(FrameError("Video storage extent overflow"))?;
        if bytes > MAX_BYTES {
            return Err(FrameError("Video frame storage exceeds bounds"));
        }
        Ok(Self {
            width,
            height,
            planes,
            color,
        })
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn planes(&self) -> &[Plane; 3] {
        &self.planes
    }
    pub fn color(&self) -> Bt709 {
        self.color
    }
    pub fn byte_len(&self) -> usize {
        self.planes.iter().map(|p| p.bytes().len()).sum()
    }
    /// Pure fixture oracle; conversion policy is not part of the public DTO API.
    #[cfg(test)]
    fn sample_srgb(&self, u: f32, v: f32) -> [f32; 3] {
        let y = sample(
            &self.planes[0],
            u * self.width as f32 - 0.5,
            v * self.height as f32 - 0.5,
        );
        let location = match self.color.chroma {
            ChromaSiting::Left => [0., 0.5],
            ChromaSiting::Center => [0.5, 0.5],
        };
        let x = (u * self.width as f32 - 0.5 - location[0]) * 0.5;
        let yy = (v * self.height as f32 - 0.5 - location[1]) * 0.5;
        to_srgb(
            y,
            sample(&self.planes[1], x, yy),
            sample(&self.planes[2], x, yy),
            self.color,
        )
    }
}
#[cfg(test)]
fn sample(plane: &Plane, x: f32, y: f32) -> f32 {
    let x = x.clamp(0., plane.width as f32 - 1.);
    let y = y.clamp(0., plane.height as f32 - 1.);
    let left = x.floor() as u32;
    let top = y.floor() as u32;
    let right = (left + 1).min(plane.width - 1);
    let bottom = (top + 1).min(plane.height - 1);
    let at = |x: u32, y: u32| plane.row(y).expect("checked plane row")[x as usize] as f32 / 255.;
    let first = at(left, top) + (at(right, top) - at(left, top)) * x.fract();
    let second = at(left, bottom) + (at(right, bottom) - at(left, bottom)) * x.fract();
    first + (second - first) * y.fract()
}
#[cfg(test)]
fn to_srgb(y: f32, u: f32, v: f32, color: Bt709) -> [f32; 3] {
    let (y, u, v) = match color.range {
        Range::Limited => (
            (y * 255. - 16.) / 219.,
            (u * 255. - 128.) / 224.,
            (v * 255. - 128.) / 224.,
        ),
        Range::Full => (y, (u * 255. - 128.) / 255., (v * 255. - 128.) / 255.),
    };
    let rgb = [
        y + 1.5748 * v,
        y - 0.18732427 * u - 0.46812427 * v,
        y + 1.8556 * u,
    ];
    rgb.map(|value| {
        let value = value.clamp(0., 1.);
        match color.transfer {
            Transfer::Srgb => value,
            Transfer::Bt709 => {
                // Continuous BT.709 curve used by FFmpeg's color utilities;
                // see docs/native-video-planes.md for the independent sources.
                let linear = if value < 0.08124286 {
                    value / 4.5
                } else {
                    ((value + 0.09929683) / 1.0992968).powf(1. / 0.45)
                };
                if linear <= 0.0031308 {
                    12.92 * linear
                } else {
                    1.055 * linear.powf(1. / 2.4) - 0.055
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn color(range: Range) -> Bt709 {
        Bt709 {
            range,
            transfer: Transfer::Srgb,
            chroma: ChromaSiting::Center,
        }
    }
    #[test]
    fn plane_validates_odd_stride_and_last_row_without_requiring_padding() {
        let plane = Plane::new(Bytes::from(vec![1, 2, 3, 99, 4, 5, 6]), 3, 2, 4).unwrap();
        assert_eq!(plane.row(0), Some(&[1, 2, 3][..]));
        assert_eq!(plane.row(1), Some(&[4, 5, 6][..]));
        assert!(plane.row(2).is_none());
        assert!(Plane::new(Bytes::from(vec![1; 6]), 3, 2, 4).is_err());
        assert!(Plane::new(Bytes::from(vec![1; 6]), 3, 2, 2).is_err());
        assert!(Plane::new(Bytes::new(), 0, 2, 4).is_err());
        assert!(Plane::new(Bytes::from(vec![1; 8]), 1, MAX_EDGE, u32::MAX).is_err());
        assert!(Plane::new(Bytes::from(vec![1]), 1, 1, u32::MAX).is_err());
    }
    #[test]
    fn odd_420_geometry_rounds_each_chroma_edge_up_and_stays_bounded() {
        let y = Plane::new(Bytes::from(vec![128; 15]), 5, 3, 5).unwrap();
        let uv = Plane::new(Bytes::from(vec![128; 6]), 3, 2, 3).unwrap();
        let frame = Yuv420Frame::new(
            5,
            3,
            [y.clone(), uv.clone(), uv.clone()],
            color(Range::Full),
        )
        .unwrap();
        assert_eq!(frame.byte_len(), 27);
        assert_eq!(frame.dimensions(), (5, 3));
        let wrong = Plane::new(Bytes::from(vec![128; 4]), 2, 2, 2).unwrap();
        assert!(Yuv420Frame::new(5, 3, [y, wrong, uv], color(Range::Full)).is_err());
    }
    #[test]
    fn limited_and_full_neutral_extremes_produce_black_white_and_preserve_grey() {
        for (range, black, white) in [(Range::Limited, 16., 235.), (Range::Full, 0., 255.)] {
            assert!(
                to_srgb(black / 255., 128. / 255., 128. / 255., color(range))
                    .iter()
                    .all(|v| v.abs() < 1e-6)
            );
            assert!(
                to_srgb(white / 255., 128. / 255., 128. / 255., color(range))
                    .iter()
                    .all(|v| (v - 1.).abs() < 1e-6)
            );
        }
        let grey = to_srgb(0.5, 128. / 255., 128. / 255., color(Range::Full));
        assert!(grey.iter().all(|v| (v - 0.5).abs() < 1e-6));
    }
    #[test]
    fn bt709_matrix_recovers_primary_vectors_before_display_transfer() {
        for rgb in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
            let y = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
            let u = (rgb[2] - y) / 1.8556;
            let v = (rgb[0] - y) / 1.5748;
            let decoded = to_srgb(y, u + 128. / 255., v + 128. / 255., color(Range::Full));
            assert!(decoded.iter().zip(rgb).all(|(a, b)| (a - b).abs() < 2e-6));
        }
    }
    #[test]
    fn bt709_transfer_is_converted_once_into_the_existing_nonlinear_srgb_target() {
        let mut c = color(Range::Full);
        c.transfer = Transfer::Bt709;
        let signal = 4.5 * 0.01;
        let expected = 1.055 * 0.01f32.powf(1. / 2.4) - 0.055;
        let rgb = to_srgb(signal, 128. / 255., 128. / 255., c);
        assert!(rgb.iter().all(|v| (v - expected).abs() < 1e-6));
        assert!((rgb[0] - signal).abs() > 0.04);
    }
    #[test]
    fn siting_and_odd_right_edge_follow_luma_sample_coordinates() {
        let y = Plane::new(Bytes::from(vec![128; 5]), 5, 1, 5).unwrap();
        let u = Plane::new(Bytes::from(vec![64, 128, 192]), 3, 1, 3).unwrap();
        let v = Plane::new(Bytes::from(vec![128; 3]), 3, 1, 3).unwrap();
        let mut c = color(Range::Full);
        c.chroma = ChromaSiting::Left;
        let left = Yuv420Frame::new(5, 1, [y.clone(), u.clone(), v.clone()], c).unwrap();
        c.chroma = ChromaSiting::Center;
        let center = Yuv420Frame::new(5, 1, [y, u, v], c).unwrap();
        assert!(left.sample_srgb(0.3, 0.5)[2] > center.sample_srgb(0.3, 0.5)[2]);
        let expected = to_srgb(128. / 255., 192. / 255., 128. / 255., color(Range::Full));
        let edge = left.sample_srgb(0.9, 0.5);
        assert!(edge.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-6));
    }
    #[test]
    fn neutral_owner_lives_until_the_last_plane_view_and_contains_no_codec_type() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        struct Owner {
            bytes: Vec<u8>,
            drops: Arc<AtomicUsize>,
        }
        impl AsRef<[u8]> for Owner {
            fn as_ref(&self) -> &[u8] {
                &self.bytes
            }
        }
        impl Drop for Owner {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let data = Bytes::from_owner(Owner {
            bytes: vec![1, 2, 3, 4],
            drops: drops.clone(),
        });
        let plane = Plane::new(data, 2, 2, 2).unwrap();
        let retained = plane.clone();
        drop(plane);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(retained.row(1), Some(&[3, 4][..]));
        drop(retained);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}
