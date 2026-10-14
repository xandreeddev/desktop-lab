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

/// Storage selected during asset preparation, independent of a GPU backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    PremultipliedRgba8,
    /// One coverage byte per pixel. The sampled color is premultiplied white;
    /// the element's tint supplies the final color.
    Alpha8,
}
impl ImageFormat {
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::PremultipliedRgba8 => 4,
            Self::Alpha8 => 1,
        }
    }
}
#[derive(Clone, Debug)]
pub struct ImageLevel {
    pub width: u32,
    pub height: u32,
    /// Tightly packed, with no row padding; see ImageData::format().
    pub pixels: Vec<u8>,
}
#[derive(Debug)]
struct Prepared {
    id: u64,
    format: ImageFormat,
    stream: Option<(u64, u64)>,
    levels: Arc<[ImageLevel]>,
}
#[derive(Clone, Debug)]
pub struct ImageData {
    /// Human-readable asset identity. Texture identity uses immutable storage instead.
    pub key: String,
    aspect_size: (u32, u32),
    prepared: Arc<Prepared>,
}
/// One generic streamed image resource, independent of media/OS policy. Each
/// prepared frame is immutable; the renderer may reuse its GPU allocation when
/// dimensions match. Clones of a stream share its identity and revision counter.
#[derive(Clone, Debug)]
pub struct ImageStream(Arc<StreamIdentity>);
#[derive(Debug)]
struct StreamIdentity {
    id: u64,
    revision: AtomicU64,
}
impl Default for ImageStream {
    fn default() -> Self {
        Self::new()
    }
}
impl ImageStream {
    pub fn new() -> Self {
        Self(Arc::new(StreamIdentity {
            id: NEXT_IMAGE.fetch_add(1, Ordering::Relaxed),
            revision: AtomicU64::new(0),
        }))
    }
    /// Prepare one straight-alpha RGBA8 frame without building per-frame mips.
    /// Call from a producer/asset worker. Static images retain full mip support.
    pub fn frame_rgba(
        &self,
        width: u32,
        height: u32,
        mut pixels: Vec<u8>,
    ) -> Result<ImageData, ImageError> {
        validate(&pixels, width, height, ImageFormat::PremultipliedRgba8)?;
        for pixel in pixels.as_chunks_mut::<4>().0 {
            if pixel[3] != 255 {
                for channel in 0..3 {
                    pixel[channel] = (u16::from(pixel[channel]) * u16::from(pixel[3]) / 255) as u8;
                }
            }
        }
        let revision = self
            .0
            .revision
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| ImageError("Image stream revision exhausted"))?
            + 1;
        Ok(ImageData {
            key: "streamed-image".into(),
            aspect_size: (width, height),
            prepared: Arc::new(Prepared {
                id: NEXT_IMAGE.fetch_add(1, Ordering::Relaxed),
                format: ImageFormat::PremultipliedRgba8,
                stream: Some((self.0.id, revision)),
                levels: vec![ImageLevel {
                    width,
                    height,
                    pixels,
                }]
                .into(),
            }),
        })
    }
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
        validate(&rgba, width, height, ImageFormat::PremultipliedRgba8)?;
        for pixel in rgba.as_chunks_mut::<4>().0 {
            for channel in 0..3 {
                pixel[channel] = (u16::from(pixel[channel]) * u16::from(pixel[3]) / 255) as u8;
            }
        }
        Ok(Self::prepare(
            key.into(),
            width,
            height,
            rgba,
            ImageFormat::PremultipliedRgba8,
        ))
    }
    /// Prepare a monochrome coverage image without duplicating coverage in four
    /// channels. Keeps the same dimensions, filtering and mip rounding as RGBA.
    /// Call on an asset worker; no color is inferred from an arbitrary image.
    pub fn from_alpha(
        key: impl Into<String>,
        width: u32,
        height: u32,
        coverage: Vec<u8>,
    ) -> Result<Self, ImageError> {
        validate(&coverage, width, height, ImageFormat::Alpha8)?;
        Ok(Self::prepare(
            key.into(),
            width,
            height,
            coverage,
            ImageFormat::Alpha8,
        ))
    }
    fn prepare(key: String, width: u32, height: u32, pixels: Vec<u8>, format: ImageFormat) -> Self {
        let mut levels = vec![ImageLevel {
            width,
            height,
            pixels,
        }];
        while levels
            .last()
            .is_some_and(|level| level.width > 1 || level.height > 1)
        {
            let previous = levels.last().unwrap();
            let (pixels, width, height) =
                reduce(&previous.pixels, previous.width, previous.height, format);
            levels.push(ImageLevel {
                width,
                height,
                pixels,
            });
        }
        Self {
            key,
            aspect_size: (width, height),
            prepared: Arc::new(Prepared {
                id: NEXT_IMAGE.fetch_add(1, Ordering::Relaxed),
                format,
                stream: None,
                levels: levels.into(),
            }),
        }
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
            aspect_size: self.aspect_size,
            prepared: Arc::new(Prepared {
                id: NEXT_IMAGE.fetch_add(1, Ordering::Relaxed),
                format: self.format(),
                stream: None,
                levels: self.levels()[index..].to_vec().into(),
            }),
        }
    }
    /// Retain the coarsest mip chain that still supplies at least one texel per
    /// physical output pixel for a cover image. Keep the base if the source is
    /// already too small. Prepare on an asset worker; this copies only retained
    /// pixels and never enlarges an original or retains discarded allocations.
    pub fn downsample_for_cover(&self, width: u32, height: u32) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError(
                "Cover target must have positive physical dimensions",
            ));
        }
        let level = self
            .levels()
            .iter()
            .take_while(|level| level.width >= width && level.height >= height)
            .last()
            .unwrap_or(&self.levels()[0]);
        Ok(self.downsample(level.width.max(level.height)))
    }
    /// Dimensions defining the image's natural aspect, independent of retained
    /// mip storage. Odd mip sizes must not alter cover cropping or containment.
    pub fn aspect_size(&self) -> (u32, u32) {
        self.aspect_size
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
    pub fn format(&self) -> ImageFormat {
        self.prepared.format
    }
    /// Base level bytes in format(), without row padding.
    pub fn pixels(&self) -> &[u8] {
        &self.prepared.levels[0].pixels
    }
    pub fn cache_id(&self) -> u64 {
        self.prepared.id
    }
    /// Stable GPU resource identity and immutable content revision for a stream.
    pub fn stream_revision(&self) -> Option<(u64, u64)> {
        self.prepared.stream
    }
    /// Freeze this immutable frame as an independent GPU image. Pixels and mip
    /// levels remain shared; no copying, decoding or mip preparation occurs.
    /// Later stream uploads cannot overwrite a snapshot's texture. Static images
    /// keep their existing descriptor and cache identity.
    pub fn snapshot(&self) -> Self {
        if self.prepared.stream.is_none() {
            return self.clone();
        }
        Self {
            key: self.key.clone(),
            aspect_size: self.aspect_size,
            prepared: Arc::new(Prepared {
                // Every streamed frame already has a globally unique immutable
                // storage ID, distinct from the stream's mutable resource ID.
                id: self.prepared.id,
                format: self.prepared.format,
                stream: None,
                levels: self.prepared.levels.clone(),
            }),
        }
    }
    pub fn bytes(&self) -> usize {
        self.prepared
            .levels
            .iter()
            .map(|level| level.pixels.len())
            .sum()
    }
}

fn validate(bytes: &[u8], width: u32, height: u32, format: ImageFormat) -> Result<(), ImageError> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(ImageError(
            "Image dimensions exceed the presentation budget",
        ));
    }
    let size = u64::from(width) * u64::from(height) * u64::from(format.bytes_per_pixel());
    if size > 128 * 1024 * 1024 {
        return Err(ImageError(
            "Image byte size exceeds the presentation budget",
        ));
    }
    if size != bytes.len() as u64 {
        return Err(ImageError(
            "Pixel data does not match image dimensions and format",
        ));
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
    validate(bytes, width, height, ImageFormat::PremultipliedRgba8)?;
    Ok(reduce(
        bytes,
        width,
        height,
        ImageFormat::PremultipliedRgba8,
    ))
}
/// Area reduction on single-channel coverage, using the same odd-edge and
/// integer rounding rules as premultiplied RGBA. Useful for text supersampling.
pub fn reduce_alpha(
    bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<(Vec<u8>, u32, u32), ImageError> {
    validate(bytes, width, height, ImageFormat::Alpha8)?;
    Ok(reduce(bytes, width, height, ImageFormat::Alpha8))
}
fn reduce(bytes: &[u8], width: u32, height: u32, format: ImageFormat) -> (Vec<u8>, u32, u32) {
    let channels = format.bytes_per_pixel() as usize;
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0; (w * h) as usize * channels];
    for y in 0..h {
        for x in 0..w {
            let (x0, x1) = (x * width / w, (x + 1) * width / w);
            let (y0, y1) = (y * height / h, (y + 1) * height / h);
            let samples = (x1 - x0) * (y1 - y0);
            let mut totals = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = (sy * width + sx) as usize * channels;
                    for c in 0..channels {
                        totals[c] += u32::from(bytes[at + c]);
                    }
                }
            }
            let at = (y * w + x) as usize * channels;
            for c in 0..channels {
                out[at + c] = ((totals[c] + samples / 2) / samples) as u8;
            }
        }
    }
    (out, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_storage_preserves_every_rgba_mip_and_owns_trimmed_pixels() {
        for (width, height) in [(1, 1), (3, 1), (1, 7), (65, 37)] {
            let alpha: Vec<u8> = (0..width * height).map(|n| (n * 73) as u8).collect();
            let rgba: Vec<u8> = alpha.iter().flat_map(|a| [255, 255, 255, *a]).collect();
            let mask = ImageData::from_alpha("mask", width, height, alpha).unwrap();
            let color = ImageData::from_rgba("same coverage", width, height, rgba).unwrap();
            assert_eq!(mask.format(), ImageFormat::Alpha8);
            assert_eq!(color.format(), ImageFormat::PremultipliedRgba8);
            assert_eq!(mask.bytes() * 4, color.bytes());
            for (mask, color) in mask.levels().iter().zip(color.levels()) {
                assert_eq!((mask.width, mask.height), (color.width, color.height));
                let expanded: Vec<u8> = mask.pixels.iter().flat_map(|a| [*a; 4]).collect();
                assert_eq!(expanded, color.pixels);
            }
            let preview = mask.downsample(2);
            assert_eq!(preview.format(), ImageFormat::Alpha8);
            assert_eq!(preview.aspect_size(), (width, height));
            assert_eq!(preview.clone().cache_id(), preview.cache_id());
            if width.max(height) > 2 {
                assert!(preview.bytes() < mask.bytes());
                assert_ne!(preview.cache_id(), mask.cache_id());
            }
            let pixels = preview.pixels().to_vec();
            drop(mask);
            assert_eq!(preview.pixels(), pixels);
        }
    }
    #[test]
    fn alpha_rejects_invalid_storage_before_preparation() {
        for (width, height, bytes) in [(0, 1, vec![]), (2, 2, vec![0; 16]), (u32::MAX, 1, vec![])] {
            assert!(ImageData::from_alpha("bad", width, height, bytes).is_err());
        }
        assert!(reduce_alpha(&[], 0, 1).is_err());
        assert!(reduce_alpha(&[0; 4], 3, 1).is_err());
    }
    #[test]
    fn cover_retains_physical_density_and_aspect_without_finer_allocations() {
        let image = ImageData::from_rgba("cover", 64, 36, vec![255; 64 * 36 * 4]).unwrap();
        let normal = image.downsample_for_cover(16, 9).unwrap();
        let hidpi = image.downsample_for_cover(32, 18).unwrap();
        assert_eq!((normal.width(), normal.height()), (16, 9));
        assert_eq!((hidpi.width(), hidpi.height()), (32, 18));
        assert_eq!(normal.aspect_size(), (64, 36));
        assert!(normal.bytes() < image.bytes() / 4);
        assert_ne!(normal.cache_id(), image.cache_id());
        assert_eq!(normal.pixels(), image.levels()[2].pixels);
        assert_eq!(hidpi.pixels(), image.levels()[1].pixels);
        let tall_target = image.downsample_for_cover(8, 32).unwrap();
        assert_eq!(tall_target.cache_id(), image.cache_id());
        assert_eq!(
            image.downsample_for_cover(1000, 1000).unwrap().cache_id(),
            image.cache_id()
        );
        assert!(image.downsample_for_cover(0, 9).is_err());
        let odd = ImageData::from_rgba("odd", 65, 37, vec![255; 65 * 37 * 4]).unwrap();
        let trimmed = odd.downsample_for_cover(16, 9).unwrap();
        assert_eq!(trimmed.aspect_size(), (65, 37));
        assert_eq!((trimmed.width(), trimmed.height()), (16, 9));
    }
    #[test]
    fn preview_copies_only_small_mips_without_premultiplying_again() {
        let image = ImageData::from_rgba("alpha", 8, 4, [240, 80, 40, 128].repeat(32)).unwrap();
        let preview = image.downsample(3);
        assert_eq!((preview.width(), preview.height()), (2, 1));
        assert_eq!(preview.pixels(), image.levels()[2].pixels);
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
        assert_eq!(image.levels()[1].pixels, [43, 0, 0, 43]);
        assert_eq!(image.bytes(), 16);
        assert_eq!(image.clone().cache_id(), image.cache_id());
        let replacement = ImageData::from_rgba("same-name", 1, 1, vec![255; 4]).unwrap();
        assert_ne!(replacement.cache_id(), image.cache_id());
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;
    #[test]
    fn streaming_frames_are_immutable_and_have_one_mip_with_a_stable_resource_identity() {
        let stream = ImageStream::new();
        let first = stream
            .frame_rgba(2, 1, vec![200, 100, 50, 128, 0, 0, 0, 255])
            .unwrap();
        let second = stream
            .frame_rgba(2, 1, vec![0, 255, 0, 255, 0, 0, 0, 255])
            .unwrap();
        assert_eq!(first.levels().len(), 1);
        assert_eq!(first.pixels()[0], 100);
        assert_eq!(first.bytes(), 8);
        assert_ne!(first.cache_id(), second.cache_id());
        let (a, ra) = first.stream_revision().unwrap();
        let (b, rb) = second.stream_revision().unwrap();
        assert_eq!(a, b);
        assert!(rb > ra);
        assert_eq!(first.pixels()[0], 100);
        assert!(stream.frame_rgba(2, 1, vec![0; 4]).is_err());
        assert!(
            ImageData::from_rgba("static", 1, 1, vec![0; 4])
                .unwrap()
                .stream_revision()
                .is_none()
        );
    }

    #[test]
    fn snapshot_shares_immutable_pixels_but_retires_mutable_stream_identity() {
        let stream = ImageStream::new();
        let first = stream
            .frame_rgba(2, 1, vec![200, 100, 50, 128, 0, 0, 0, 255])
            .unwrap();
        let snapshot = first.snapshot();
        let second = stream.frame_rgba(2, 1, vec![255; 8]).unwrap();
        assert!(snapshot.stream_revision().is_none());
        assert_eq!(snapshot.cache_id(), first.cache_id());
        assert_ne!(snapshot.cache_id(), second.cache_id());
        assert_eq!(snapshot.format(), first.format());
        assert_eq!(snapshot.aspect_size(), first.aspect_size());
        assert_eq!(snapshot.levels().len(), 1);
        assert_eq!(snapshot.bytes(), first.bytes());
        assert_eq!(snapshot.pixels().as_ptr(), first.pixels().as_ptr());
        assert!(Arc::ptr_eq(
            &snapshot.prepared.levels,
            &first.prepared.levels
        ));
        assert_eq!(snapshot.pixels(), [100, 50, 25, 128, 0, 0, 0, 255]);
        drop(first);
        drop(second);
        drop(stream);
        assert_eq!(snapshot.pixels(), [100, 50, 25, 128, 0, 0, 0, 255]);
        let again = snapshot.snapshot();
        assert!(Arc::ptr_eq(&snapshot.prepared, &again.prepared));
    }

    #[test]
    fn static_snapshot_preserves_existing_descriptor_and_full_mip_storage() {
        let image = ImageData::from_rgba("still", 4, 2, [80, 40, 20, 128].repeat(8)).unwrap();
        let snapshot = image.snapshot();
        assert!(Arc::ptr_eq(&image.prepared, &snapshot.prepared));
        assert_eq!(image.levels().len(), 3);
        assert_eq!(snapshot.cache_id(), image.cache_id());
        assert_eq!(snapshot.pixels().as_ptr(), image.pixels().as_ptr());
        assert_eq!(snapshot.bytes(), image.bytes());
    }
}
