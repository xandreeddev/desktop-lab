//! Immutable presentation images, prepared before entering the UI event loop.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static NEXT_IMAGE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageError(pub &'static str);
impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ImageError {}

#[derive(Clone, Debug)]
pub struct ImageLevel {
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA8, tightly packed with no row padding.
    pub rgba: Vec<u8>,
}
#[derive(Debug)]
struct Prepared {
    id: u64,
    levels: Vec<ImageLevel>,
}
#[derive(Clone, Debug)]
pub struct ImageData {
    /// Human-readable asset identity. Texture identity uses immutable storage instead.
    pub key: String,
    prepared: Arc<Prepared>,
}
impl ImageData {
    /// Validate straight-alpha RGBA8, premultiply and prepare the full mip chain.
    /// Call on an asset worker, never from view/layout/paint. Cloning is inexpensive.
    pub fn from_rgba(
        key: impl Into<String>,
        width: u32,
        height: u32,
        mut rgba: Vec<u8>,
    ) -> Result<Self, ImageError> {
        validate(&rgba, width, height)?;
        for pixel in rgba.as_chunks_mut::<4>().0 {
            for channel in 0..3 {
                pixel[channel] = (u16::from(pixel[channel]) * u16::from(pixel[3]) / 255) as u8;
            }
        }
        let mut levels = vec![ImageLevel {
            width,
            height,
            rgba,
        }];
        while levels
            .last()
            .is_some_and(|level| level.width > 1 || level.height > 1)
        {
            let previous = levels.last().unwrap();
            let (rgba, width, height) = reduce(&previous.rgba, previous.width, previous.height);
            levels.push(ImageLevel {
                width,
                height,
                rgba,
            });
        }
        Ok(Self {
            key: key.into(),
            prepared: Arc::new(Prepared {
                id: NEXT_IMAGE.fetch_add(1, Ordering::Relaxed),
                levels,
            }),
        })
    }
    /// Copy an already prepared mip chain at or below the requested dimensions.
    /// A small preview does not retain the full-resolution allocation. This only
    /// copies premultiplied pixels; it neither decodes nor resamples the source.
    /// Prepare on an asset worker, outside the UI/render thread.
    pub fn downsample(&self, max_dimension: u32) -> Self {
        let index = self
            .levels()
            .iter()
            .position(|level| level.width.max(level.height) <= max_dimension.max(1))
            .unwrap_or(self.levels().len() - 1);
        if index == 0 {
            return self.clone();
        }
        Self {
            key: self.key.clone(),
            prepared: Arc::new(Prepared {
                id: NEXT_IMAGE.fetch_add(1, Ordering::Relaxed),
                levels: self.levels()[index..].to_vec(),
            }),
        }
    }
    pub fn width(&self) -> u32 {
        self.prepared.levels[0].width
    }
    pub fn height(&self) -> u32 {
        self.prepared.levels[0].height
    }
    pub fn levels(&self) -> &[ImageLevel] {
        &self.prepared.levels
    }
    pub fn pixels(&self) -> &[u8] {
        &self.prepared.levels[0].rgba
    }
    pub fn cache_id(&self) -> u64 {
        self.prepared.id
    }
    pub fn bytes(&self) -> usize {
        self.prepared
            .levels
            .iter()
            .map(|level| level.rgba.len())
            .sum()
    }
}

fn validate(bytes: &[u8], width: u32, height: u32) -> Result<(), ImageError> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(ImageError(
            "Image dimensions exceed the presentation budget",
        ));
    }
    let size = u64::from(width) * u64::from(height) * 4;
    if size > 128 * 1024 * 1024 {
        return Err(ImageError(
            "Image byte size exceeds the presentation budget",
        ));
    }
    if size != bytes.len() as u64 {
        return Err(ImageError("RGBA data does not match image dimensions"));
    }
    Ok(())
}

/// Area reduction on premultiplied RGBA, including odd-sized edge texels.
/// Also used to resolve supersampled text coverage once.
pub fn reduce_premultiplied(
    bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<(Vec<u8>, u32, u32), ImageError> {
    validate(bytes, width, height)?;
    Ok(reduce(bytes, width, height))
}
fn reduce(bytes: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (x0, x1) = (x * width / w, (x + 1) * width / w);
            let (y0, y1) = (y * height / h, (y + 1) * height / h);
            let samples = (x1 - x0) * (y1 - y0);
            let mut channels = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = ((sy * width + sx) * 4) as usize;
                    for c in 0..4 {
                        channels[c] += u32::from(bytes[at + c]);
                    }
                }
            }
            let at = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                out[at + c] = ((channels[c] + samples / 2) / samples) as u8;
            }
        }
    }
    (out, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_copies_only_small_mips_without_premultiplying_again() {
        let image = ImageData::from_rgba("alpha", 8, 4, [240, 80, 40, 128].repeat(32)).unwrap();
        let preview = image.downsample(3);
        assert_eq!((preview.width(), preview.height()), (2, 1));
        assert_eq!(preview.pixels(), image.levels()[2].rgba);
        assert!(preview.bytes() < image.bytes());
        assert_ne!(preview.cache_id(), image.cache_id());
        assert_eq!(image.downsample(8).cache_id(), image.cache_id());
        assert_eq!(image.downsample(0).width(), 1);
        drop(image);
        assert_eq!(&preview.pixels()[..4], &[120, 40, 20, 128]);
    }
    #[test]
    fn validates_storage_before_preparing_mips() {
        assert!(ImageData::from_rgba("empty", 0, 1, vec![]).is_err());
        assert!(ImageData::from_rgba("wrong length", 2, 2, vec![0; 4]).is_err());
        assert!(ImageData::from_rgba("too large", u32::MAX, u32::MAX, vec![]).is_err());
    }
    #[test]
    fn immutable_storage_has_distinct_identity_and_preserves_coverage_at_odd_edges() {
        let image = ImageData::from_rgba(
            "same-name",
            3,
            1,
            vec![255, 0, 0, 128, 255, 255, 255, 0, 0, 0, 0, 0],
        )
        .unwrap();
        assert_eq!(image.pixels()[..4], [128, 0, 0, 128]);
        assert_eq!(image.levels()[1].rgba, [43, 0, 0, 43]);
        assert_eq!(image.bytes(), 16);
        assert_eq!(image.clone().cache_id(), image.cache_id());
        let replacement = ImageData::from_rgba("same-name", 1, 1, vec![255; 4]).unwrap();
        assert_ne!(replacement.cache_id(), image.cache_id());
    }
}
