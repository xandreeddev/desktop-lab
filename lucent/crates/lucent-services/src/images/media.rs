//! Selected media artwork. Bounded file/curl transport and raster decoding stay off views.
use lucent_api::ImageData;
use lucent_domain::{DomainError, MediaArtworkSource, Result};
use std::{
    fs::OpenOptions,
    io::{Cursor, Read},
    os::{
        fd::AsRawFd,
        unix::{ffi::OsStringExt, fs::OpenOptionsExt},
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
const BYTES: usize = 4 * 1024 * 1024;
const PIXELS: u64 = 4 * 1024 * 1024;
const EDGE: u32 = 4096;
const TARGET: u32 = 512;
const DEADLINE: Duration = Duration::from_secs(4);
fn unavailable() -> DomainError {
    DomainError::Unavailable("Media artwork could not be read".into())
}
fn invalid() -> DomainError {
    DomainError::Invalid("Media artwork exceeds supported raster limits".into())
}

fn local(source: &str) -> Result<PathBuf> {
    let (_, location) = source.split_once("://").ok_or_else(invalid)?;
    let (_, path) = location.split_at(location.find('/').ok_or_else(invalid)?);
    let mut bytes = vec![];
    let mut input = path.bytes();
    while let Some(value) = input.next() {
        let value = if value == b'%' {
            let a = char::from(input.next().ok_or_else(invalid)?)
                .to_digit(16)
                .ok_or_else(invalid)?;
            let b = char::from(input.next().ok_or_else(invalid)?)
                .to_digit(16)
                .ok_or_else(invalid)?;
            (a * 16 + b) as u8
        } else {
            value
        };
        if value == 0 || value.is_ascii_control() {
            return Err(invalid());
        }
        bytes.push(value);
    }
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn remote(source: &str) -> Result<Vec<u8>> {
    let child = Command::new("curl")
        .args([
            "--disable",
            "--globoff",
            "--silent",
            "--fail",
            "--location",
            "--max-redirs",
            "3",
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
            "--connect-timeout",
            "2",
            "--max-time",
            "4",
            "--max-filesize",
            "4194304",
            "--url",
            source,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| unavailable())?;
    let mut child = Process(child);
    let mut output = child.0.stdout.take().ok_or_else(unavailable)?;
    // SAFETY: fcntl only accesses the live owned pipe's descriptor.
    let flags = unsafe { libc::fcntl(output.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(output.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(unavailable());
    }
    let deadline = Instant::now() + DEADLINE;
    let mut bytes = vec![];
    let mut eof = false;
    loop {
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        let mut chunk = [0; 8192];
        let waiting = match output.read(&mut chunk) {
            Ok(0) => {
                eof = true;
                true
            }
            Ok(count) => {
                if count > BYTES.saturating_sub(bytes.len()) {
                    return Err(invalid());
                }
                bytes.extend_from_slice(&chunk[..count]);
                false
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                true
            }
            Err(_) => return Err(unavailable()),
        };
        if eof && let Some(status) = child.0.try_wait().map_err(|_| unavailable())? {
            return if status.success() {
                Ok(bytes)
            } else {
                Err(unavailable())
            };
        }
        if waiting {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
fn read(source: &MediaArtworkSource) -> Result<Vec<u8>> {
    if source
        .value()
        .split_once("://")
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("file"))
    {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(local(source.value())?)
            .map_err(|_| unavailable())?;
        let metadata = file.metadata().map_err(|_| unavailable())?;
        if !metadata.is_file() || metadata.len() > BYTES as u64 {
            return Err(invalid());
        }
        let mut bytes = vec![];
        file.take(BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable())?;
        if bytes.len() > BYTES {
            return Err(invalid());
        }
        Ok(bytes)
    } else {
        remote(source.value())
    }
}
fn decode(bytes: &[u8], key: &str) -> Result<Arc<ImageData>> {
    let dimensions = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid())?
        .into_dimensions()
        .map_err(|_| invalid())?;
    if dimensions.0 == 0
        || dimensions.1 == 0
        || dimensions.0 > EDGE
        || dimensions.1 > EDGE
        || u64::from(dimensions.0) * u64::from(dimensions.1) > PIXELS
    {
        return Err(invalid());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(EDGE);
    limits.max_image_height = Some(EDGE);
    limits.max_alloc = Some(PIXELS * 4);
    reader.limits(limits);
    let mut rgba = reader.decode().map_err(|_| invalid())?.to_rgba8();
    if rgba.width() > TARGET || rgba.height() > TARGET {
        for pixel in rgba.pixels_mut() {
            for channel in 0..3 {
                pixel[channel] = (u16::from(pixel[channel]) * u16::from(pixel[3]) / 255) as u8;
            }
        }
        rgba = image::DynamicImage::ImageRgba8(rgba)
            .resize(TARGET, TARGET, image::imageops::FilterType::Lanczos3)
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
        .map_err(|_| invalid())
}
pub fn prepare(source: &MediaArtworkSource) -> Result<Arc<ImageData>> {
    decode(&read(source)?, source.value())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        net::TcpListener,
        sync::atomic::{AtomicU64, Ordering},
    };
    fn png() -> Vec<u8> {
        let mut bytes = Cursor::new(vec![]);
        image::DynamicImage::new_rgba8(2, 3)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    #[test]
    fn local_files_decode_without_network_and_reject_nonregular_or_oversized_sources() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lucent-media-art-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, png()).unwrap();
        let source = MediaArtworkSource::new(format!("file://{}", path.display())).unwrap();
        let image = prepare(&source).unwrap();
        assert_eq!((image.width(), image.height()), (2, 3));
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(BYTES as u64 + 1)
            .unwrap();
        assert!(prepare(&source).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(prepare(&source).is_err());
        std::fs::remove_dir(&path).unwrap();
        assert!(decode(b"<svg xmlns='http://www.w3.org/2000/svg'/>", "fixture").is_err());
    }
    #[test]
    fn http_artwork_accepts_owned_raster_bytes_and_rejects_overflow_without_a_length_header() {
        for (overflow, delayed) in [(false, false), (true, false), (false, true)] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            listener.set_nonblocking(true).unwrap();
            let worker = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) => panic!("artwork fixture admission failed: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 4096];
                let read = stream.read(&mut request).unwrap();
                assert!(
                    std::str::from_utf8(&request[..read])
                        .unwrap()
                        .starts_with("GET /art{one,two}.png?slot=[1-2] HTTP/1.1\r\n")
                );
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                    .unwrap();
                if overflow {
                    let _ = stream.write_all(&vec![b'x'; BYTES + 1]);
                } else {
                    let bytes = png();
                    if delayed {
                        stream.write_all(&bytes[..8]).unwrap();
                        std::thread::sleep(Duration::from_millis(100));
                        stream.write_all(&bytes[8..]).unwrap();
                    } else {
                        stream.write_all(&bytes).unwrap();
                    }
                }
            });
            let source =
                MediaArtworkSource::new(format!("http://{address}/art{{one,two}}.png?slot=[1-2]"))
                    .unwrap();
            let result = prepare(&source);
            worker.join().unwrap();
            assert_eq!(result.is_err(), overflow);
        }
    }
}
