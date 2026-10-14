//! Bounded notification image content. No paths are opened or pixels rendered.
use crate::{DomainError, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NotificationImageSource(String);
impl NotificationImageSource {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 2048
            || value.chars().any(char::is_control)
            || !(value.starts_with('/')
                || value.starts_with("file:///")
                || value.starts_with("file://localhost/")
                || value
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.')))
        {
            return Err(DomainError::Invalid(
                "Notification image source is invalid".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(&self) -> &str {
        &self.0
    }
    pub fn local(&self) -> bool {
        self.0.starts_with('/') || self.0.starts_with("file://")
    }
}
impl TryFrom<String> for NotificationImageSource {
    type Error = DomainError;
    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}
impl From<NotificationImageSource> for String {
    fn from(value: NotificationImageSource) -> Self {
        value.0
    }
}

/// Unpremultiplied RGBA content; immutable storage makes snapshots cheap.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PixelRecord", into = "PixelRecord")]
pub struct NotificationPixels {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
}
impl NotificationPixels {
    pub const MAX_EDGE: u32 = 512;
    pub const MAX_PIXELS: u64 = 65536;
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
        let count = u64::from(width) * u64::from(height);
        if width == 0
            || height == 0
            || width > Self::MAX_EDGE
            || height > Self::MAX_EDGE
            || count > Self::MAX_PIXELS
            || rgba.len() as u64 != count * 4
        {
            return Err(DomainError::Invalid(
                "Notification image dimensions or bytes are invalid".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            rgba: rgba.into(),
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}
#[derive(Serialize, Deserialize)]
struct PixelRecord {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}
impl TryFrom<PixelRecord> for NotificationPixels {
    type Error = DomainError;
    fn try_from(value: PixelRecord) -> Result<Self> {
        Self::new(value.width, value.height, value.rgba)
    }
}
impl From<NotificationPixels> for PixelRecord {
    fn from(value: NotificationPixels) -> Self {
        Self {
            width: value.width,
            height: value.height,
            rgba: value.rgba.to_vec(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationImage {
    Source(NotificationImageSource),
    Pixels(NotificationPixels),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimensions_byte_count_and_sources_have_real_invariants() {
        for (w, h, bytes) in [
            (0, 1, 0),
            (513, 1, 2052),
            (512, 512, 512 * 512 * 4),
            (2, 2, 15),
        ] {
            assert!(NotificationPixels::new(w, h, vec![0; bytes]).is_err());
        }
        let image = NotificationPixels::new(400, 100, vec![255; 400 * 100 * 4]).unwrap();
        assert_eq!(
            (image.width(), image.height(), image.rgba().len()),
            (400, 100, 160000)
        );
        for value in [
            "https://example.test/image",
            "../../image",
            "bad/icon",
            "name\n",
            "file://remote/a",
            "",
        ] {
            assert!(NotificationImageSource::new(value).is_err());
        }
        for value in ["mail-unread", "/tmp/a b.png", "file:///tmp/a%20b.png"] {
            assert_eq!(NotificationImageSource::new(value).unwrap().value(), value);
        }
    }
}
