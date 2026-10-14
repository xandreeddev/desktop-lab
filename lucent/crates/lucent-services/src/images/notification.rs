//! Bounded local notification artwork; SVGs cannot read nested resources.
use super::LoadError;
use lucent_api::ImageData;
use lucent_domain::*;
use std::{
    fs::OpenOptions,
    io::{Cursor, Read},
    os::unix::{ffi::OsStringExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};
const FILE_BYTES: u64 = 4 * 1024 * 1024;
const DECODE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_EDGE: u32 = 4096;
const TARGET: (u32, u32) = (400, 200);

fn path(source: &NotificationImageSource) -> Option<PathBuf> {
    let value = source.value();
    if value.starts_with('/') {
        return Some(PathBuf::from(value));
    }
    let value = value
        .strip_prefix("file://localhost")
        .or_else(|| value.strip_prefix("file://"))?;
    if !value.starts_with('/') || value.contains(['?', '#']) {
        return None;
    }
    let mut bytes = vec![];
    let mut input = value.bytes();
    while let Some(byte) = input.next() {
        let byte = if byte == b'%' {
            let a = char::from(input.next()?).to_digit(16)?;
            let b = char::from(input.next()?).to_digit(16)?;
            (a * 16 + b) as u8
        } else {
            byte
        };
        if byte == 0 || byte.is_ascii_control() {
            return None;
        }
        bytes.push(byte);
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}
fn read(path: &Path) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| DomainError::Unavailable("Notification image could not be read".into()))?;
    let metadata = file.metadata().map_err(|_| {
        DomainError::Unavailable("Notification image metadata is unavailable".into())
    })?;
    if !metadata.is_file() || metadata.len() > FILE_BYTES {
        return Err(DomainError::Invalid(
            "Notification image source exceeds supported limits".into(),
        ));
    }
    let mut bytes = vec![];
    file.take(FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DomainError::Unavailable("Notification image could not be read".into()))?;
    if bytes.len() as u64 > FILE_BYTES {
        return Err(DomainError::Invalid(
            "Notification image source exceeds supported limits".into(),
        ));
    }
    Ok(bytes)
}
fn prepared(key: &str, mut rgba: image::RgbaImage) -> Result<Arc<ImageData>> {
    if rgba.width() > TARGET.0 || rgba.height() > TARGET.1 {
        for pixel in rgba.pixels_mut() {
            for channel in 0..3 {
                pixel[channel] = (u16::from(pixel[channel]) * u16::from(pixel[3]) / 255) as u8;
            }
        }
        rgba = image::DynamicImage::ImageRgba8(rgba)
            .resize(TARGET.0, TARGET.1, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        for pixel in rgba.pixels_mut() {
            for channel in 0..3 {
                pixel[channel] = if pixel[3] == 0 {
                    0
                } else {
                    (u32::from(pixel[channel]) * 255 / u32::from(pixel[3])).min(255) as u8
                };
            }
        }
    }
    ImageData::from_rgba(key, rgba.width(), rgba.height(), rgba.into_raw())
        .map(Arc::new)
        .map_err(|_| DomainError::Invalid("Notification image pixels are invalid".into()))
}
fn decode(path: &Path, key: &str) -> Result<Arc<ImageData>> {
    let bytes = read(path)?;
    let xml = bytes
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(&bytes)
        .iter()
        .find(|byte| !byte.is_ascii_whitespace())
        == Some(&b'<');
    if xml {
        let options = resvg::usvg::Options {
            image_href_resolver: resvg::usvg::ImageHrefResolver {
                resolve_data: Box::new(|_, _, _| None),
                resolve_string: Box::new(|_, _| None),
            },
            ..Default::default()
        };
        let tree = resvg::usvg::Tree::from_data(&bytes, &options)
            .map_err(|_| DomainError::Invalid("Notification SVG is invalid".into()))?;
        if !tree.filters().is_empty() {
            return Err(DomainError::Invalid(
                "Notification SVG filters are unsupported".into(),
            ));
        }
        let scale =
            (TARGET.0 as f32 / tree.size().width()).min(TARGET.1 as f32 / tree.size().height());
        let width = (tree.size().width() * scale).ceil() as u32;
        let height = (tree.size().height() * scale).ceil() as u32;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| {
            DomainError::Invalid("Notification SVG dimensions are invalid".into())
        })?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let mut pixels = pixmap.take();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            for channel in 0..3 {
                pixel[channel] = if pixel[3] == 0 {
                    0
                } else {
                    (u32::from(pixel[channel]) * 255 / u32::from(pixel[3])).min(255) as u8
                };
            }
        }
        let image = image::RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| DomainError::Invalid("Notification SVG pixels are invalid".into()))?;
        return prepared(key, image);
    }
    let dimensions = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(LoadError::Read)
        .and_then(|reader| reader.into_dimensions().map_err(LoadError::from))
        .map_err(|_| DomainError::Invalid("Notification image header is invalid".into()))?;
    if dimensions.0 > MAX_EDGE
        || dimensions.1 > MAX_EDGE
        || u64::from(dimensions.0) * u64::from(dimensions.1) * 4 > DECODE_BYTES
    {
        return Err(DomainError::Invalid(
            "Notification image decode exceeds supported limits".into(),
        ));
    }
    let mut reader = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|_| DomainError::Invalid("Notification image format is invalid".into()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(DECODE_BYTES);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| DomainError::Invalid("Notification image could not be decoded".into()))?;
    prepared(key, image.to_rgba8())
}
pub fn prepare(key: &str, image: &NotificationImage) -> Result<Arc<ImageData>> {
    match image {
        NotificationImage::Pixels(pixels) => prepared(
            key,
            image::RgbaImage::from_raw(pixels.width(), pixels.height(), pixels.rgba().to_vec())
                .ok_or_else(|| {
                    DomainError::Invalid("Notification image pixels are invalid".into())
                })?,
        ),
        NotificationImage::Source(source) if source.local() => decode(
            &path(source)
                .ok_or_else(|| DomainError::Invalid("Notification file URI is invalid".into()))?,
            key,
        ),
        NotificationImage::Source(source) => {
            super::icon_with(source.value(), |path| decode(path, key).ok())
                .ok_or_else(|| DomainError::Unavailable("Notification icon is unavailable".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "lucent-notification-image-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn files_decode_and_svg_cannot_read_nested_resources_or_filters() {
        let fixture = Fixture::new();
        let png = fixture.0.join("pixel.png");
        image::save_buffer(&png, &[10, 20, 30, 128], 1, 1, image::ColorType::Rgba8).unwrap();
        let source = NotificationImageSource::new(png.to_str().unwrap()).unwrap();
        let prepared = prepare("pixel", &NotificationImage::Source(source)).unwrap();
        assert_eq!(prepared.pixels(), [5, 10, 15, 128]);
        let svg = fixture.0.join("image.svg");
        std::fs::write(&svg,format!("<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><image href='{}' width='2' height='2'/></svg>",png.display())).unwrap();
        let pixels = decode(&svg, "external").unwrap();
        assert!(pixels.pixels().iter().all(|byte| *byte == 0));
        std::fs::write(&svg,"<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><defs><filter id='f'><feGaussianBlur stdDeviation='1000'/></filter></defs><rect width='2' height='2' filter='url(#f)'/></svg>").unwrap();
        assert!(decode(&svg, "filter").is_err());
        // Opening an untrusted FIFO must return without waiting for a writer.
        let fifo = fixture.0.join("pipe");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(read(&fifo).is_err());
    }
    #[test]
    fn local_uri_decoding_rejects_controls_and_does_not_fetch_network_images() {
        assert_eq!(
            path(&NotificationImageSource::new("file:///tmp/a%20b.png").unwrap()).unwrap(),
            PathBuf::from("/tmp/a b.png")
        );
        for value in [
            "file:///tmp/a%00",
            "file:///tmp/a%0a",
            "file:///tmp/a%zz",
            "file:///tmp/a?query",
        ] {
            assert!(path(&NotificationImageSource::new(value).unwrap()).is_none());
        }
        assert!(NotificationImageSource::new("https://example.test/a.png").is_err());
    }
    #[test]
    fn rgba_preparation_keeps_straight_alpha_semantics_and_bounded_density() {
        let pixels =
            NotificationPixels::new(400, 100, [200, 100, 40, 128].repeat(400 * 100)).unwrap();
        let image = prepare("fixture", &NotificationImage::Pixels(pixels)).unwrap();
        assert_eq!(image.aspect_size(), (400, 100));
        assert_eq!(&image.pixels()[..4], &[100, 50, 20, 128]);
        let pixels = NotificationPixels::new(256, 256, vec![255; 256 * 256 * 4]).unwrap();
        let image = prepare("square", &NotificationImage::Pixels(pixels)).unwrap();
        assert_eq!(image.aspect_size(), (200, 200));
    }
}
