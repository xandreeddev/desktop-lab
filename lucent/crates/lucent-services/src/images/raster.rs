//! Raster resource policy. Large JPEGs use scaled DCT output before final resampling.
use image::{DynamicImage, ImageFormat, ImageReader};
use std::{fmt, io::Cursor, sync::Mutex};

const DECODE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_EDGE: u32 = 16_384;
const MAX_PIXELS: u64 = 100_000_000;
// Progressive JPEGs retain coefficients at source resolution even with scaled output.
// Serialize this expensive path so previews and wallpaper workers cannot multiply it.
static LARGE_JPEG: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub enum LoadError {
    Read(std::io::Error),
    Limit(&'static str),
    Decode(String),
}
impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(f, "Could not read image: {error}"),
            Self::Limit(message) => f.write_str(message),
            Self::Decode(message) => write!(f, "Could not decode image: {message}"),
        }
    }
}
impl std::error::Error for LoadError {}
impl From<image::ImageError> for LoadError {
    fn from(error: image::ImageError) -> Self {
        match error {
            image::ImageError::Limits(_) => {
                Self::Limit("Image exceeds the 128 MiB decoding budget")
            }
            other => Self::Decode(other.to_string()),
        }
    }
}

pub(super) fn decode(bytes: &[u8], size: u32) -> Result<DynamicImage, LoadError> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(LoadError::Read)?;
    let format = reader.format();
    let (width, height) = reader.into_dimensions()?;
    if width > MAX_EDGE || height > MAX_EDGE {
        return Err(LoadError::Limit("Image dimensions exceed 16384 pixels"));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_PIXELS {
        return Err(LoadError::Limit("Image exceeds 100 megapixels"));
    }
    if format == Some(ImageFormat::Jpeg) && pixels * 4 > DECODE_BYTES {
        let _guard = LARGE_JPEG.lock().unwrap_or_else(|error| error.into_inner());
        return scaled_jpeg(bytes, size);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(LoadError::Read)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(DECODE_BYTES);
    reader.limits(limits);
    Ok(reader.decode()?)
}

fn scaled_jpeg(bytes: &[u8], size: u32) -> Result<DynamicImage, LoadError> {
    use jpeg_decoder::PixelFormat;
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder.set_max_decoding_buffer_size(DECODE_BYTES as usize);
    decoder
        .scale(size as u16, size as u16)
        .map_err(|error| LoadError::Decode(error.to_string()))?;
    let info = decoder
        .info()
        .ok_or_else(|| LoadError::Decode("Missing JPEG dimensions".into()))?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    if u64::from(width) * u64::from(height) * 4 > DECODE_BYTES {
        return Err(LoadError::Limit(
            "Scaled JPEG exceeds the 128 MiB decoding budget",
        ));
    }
    if !matches!(info.pixel_format, PixelFormat::RGB24 | PixelFormat::L8) {
        return Err(LoadError::Decode(
            "Large JPEG requires 8-bit RGB or grayscale colors".into(),
        ));
    }
    let pixels = decoder
        .decode()
        .map_err(|error| LoadError::Decode(error.to_string()))?;
    match info.pixel_format {
        PixelFormat::RGB24 => {
            image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
        }
        PixelFormat::L8 => {
            image::GrayImage::from_raw(width, height, pixels).map(DynamicImage::ImageLuma8)
        }
        _ => unreachable!(),
    }
    .ok_or_else(|| LoadError::Decode("Incorrect JPEG pixel count".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        let pixels = image::RgbImage::from_pixel(256, 144, image::Rgb([30, 140, 220]));
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
            .encode_image(&pixels)
            .unwrap();
        bytes
    }
    #[test]
    fn jpeg_decoding_scales_before_allocating_final_pixels() {
        let image = scaled_jpeg(&fixture(), 32).unwrap();
        assert_eq!((image.width(), image.height()), (32, 18));
        let pixel = image.to_rgb8().get_pixel(10, 10).0;
        for (actual, expected) in pixel.into_iter().zip([30u8, 140, 220]) {
            assert!(actual.abs_diff(expected) <= 3);
        }
    }
    #[test]
    fn hostile_dimensions_fail_before_pixel_allocation() {
        let mut jpeg = fixture();
        let offset = jpeg.windows(2).position(|v| v == [0xff, 0xc0]).unwrap();
        jpeg[offset + 5..offset + 9].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        assert!(matches!(decode(&jpeg, 64), Err(LoadError::Limit(_))));
        assert!(matches!(
            decode(b"not an image", 64),
            Err(LoadError::Decode(_))
        ));
    }
    #[test]
    #[ignore = "optional private high-resolution reproduction; set LUCENT_TEST_IMAGE"]
    fn high_resolution_wallpaper_reproduction() {
        let path = std::env::var("LUCENT_TEST_IMAGE").expect("set LUCENT_TEST_IMAGE");
        let mut original = ImageReader::open(&path)
            .unwrap()
            .with_guessed_format()
            .unwrap();
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(DECODE_BYTES);
        original.limits(limits);
        assert!(matches!(
            original.decode(),
            Err(image::ImageError::Limits(_))
        ));
        for size in [64, 128, 1024, 4096] {
            let image = super::super::try_load(std::path::Path::new(&path), size).unwrap();
            assert!(image.width <= size && image.height <= size);
            assert_eq!(
                image.rgba.len(),
                image.width as usize * image.height as usize * 4
            );
            println!("Decoded requested {size}: {}×{}", image.width, image.height);
        }
    }
}
