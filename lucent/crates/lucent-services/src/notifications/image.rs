//! Borrow wire variants before validating allocation/stride and copying content.
use lucent_domain::*;
use std::collections::HashMap;
use zbus::zvariant::{Array, OwnedValue, Structure};

fn pixels(value: &OwnedValue) -> Option<NotificationPixels> {
    let fields = <&Structure>::try_from(value).ok()?.fields();
    if fields.len() != 7 {
        return None;
    }
    let width = u32::try_from(i32::try_from(&fields[0]).ok()?).ok()?;
    let height = u32::try_from(i32::try_from(&fields[1]).ok()?).ok()?;
    let stride = usize::try_from(i32::try_from(&fields[2]).ok()?).ok()?;
    let alpha = bool::try_from(&fields[3]).ok()?;
    if i32::try_from(&fields[4]).ok()? != 8 {
        return None;
    }
    let channels = usize::try_from(i32::try_from(&fields[5]).ok()?).ok()?;
    if channels != if alpha { 4 } else { 3 }
        || width == 0
        || height == 0
        || width > NotificationPixels::MAX_EDGE
        || height > NotificationPixels::MAX_EDGE
        || u64::from(width) * u64::from(height) > NotificationPixels::MAX_PIXELS
    {
        return None;
    }
    let bytes = <&Array>::try_from(&fields[6]).ok()?.inner();
    let row = (width as usize).checked_mul(channels)?;
    let required = stride.checked_mul(height as usize - 1)?.checked_add(row)?;
    if stride < row || bytes.len() < required || bytes.len() > 1024 * 1024 || required > 1024 * 1024
    {
        return None;
    }
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let at = y * stride + x * channels;
            rgba.extend([
                u8::try_from(&bytes[at]).ok()?,
                u8::try_from(&bytes[at + 1]).ok()?,
                u8::try_from(&bytes[at + 2]).ok()?,
                if alpha {
                    u8::try_from(&bytes[at + 3]).ok()?
                } else {
                    255
                },
            ]);
        }
    }
    NotificationPixels::new(width, height, rgba).ok()
}
pub(super) fn select(
    hints: &HashMap<String, OwnedValue>,
    app_icon: &str,
) -> Option<NotificationImage> {
    for name in ["image-data", "image_data"] {
        if let Some(image) = hints.get(name).and_then(pixels) {
            return Some(NotificationImage::Pixels(image));
        }
    }
    for name in ["image-path", "image_path"] {
        if let Some(image) = hints
            .get(name)
            .and_then(|value| <&str>::try_from(value).ok())
            .and_then(|value| NotificationImageSource::new(value).ok())
        {
            return Some(NotificationImage::Source(image));
        }
    }
    if let Ok(image) = NotificationImageSource::new(app_icon) {
        return Some(NotificationImage::Source(image));
    }
    hints
        .get("icon_data")
        .and_then(pixels)
        .map(NotificationImage::Pixels)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn wire(
        width: i32,
        height: i32,
        stride: i32,
        alpha: bool,
        channels: i32,
        bytes: Vec<u8>,
    ) -> OwnedValue {
        OwnedValue::try_from(zbus::zvariant::Value::from((
            width, height, stride, alpha, 8i32, channels, bytes,
        )))
        .unwrap()
    }
    #[test]
    fn rgb_and_rgba_rows_preserve_color_alpha_and_ignore_padding() {
        let image = pixels(&wire(
            2,
            2,
            8,
            false,
            3,
            vec![10, 20, 30, 40, 50, 60, 0, 0, 70, 80, 90, 100, 110, 120],
        ))
        .unwrap();
        assert_eq!(
            image.rgba(),
            [
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255
            ]
        );
        let image = pixels(&wire(1, 1, 4, true, 4, vec![10, 20, 30, 40])).unwrap();
        assert_eq!(image.rgba(), [10, 20, 30, 40]);
    }
    #[test]
    fn invalid_dimensions_stride_channels_and_truncated_storage_have_no_pixels() {
        for value in [
            wire(-1, 1, 4, true, 4, vec![0; 4]),
            wire(513, 1, 513 * 4, true, 4, vec![0; 513 * 4]),
            wire(1, 1, 3, true, 4, vec![0; 4]),
            wire(2, 2, 8, true, 4, vec![0; 15]),
            wire(1, 1, 4, false, 4, vec![0; 4]),
            wire(256, 257, 1024, true, 4, vec![0; 256 * 257 * 4]),
        ] {
            assert!(pixels(&value).is_none());
        }
    }
    #[test]
    fn one_image_obeys_protocol_priority_and_invalid_content_can_fall_back() {
        let mut hints = HashMap::new();
        hints.insert(
            "image-data".into(),
            wire(1, 1, 4, true, 4, vec![1, 2, 3, 4]),
        );
        hints.insert(
            "image-path".into(),
            OwnedValue::from(zbus::zvariant::Str::from("/tmp/fixture.png")),
        );
        assert!(matches!(
            select(&hints, "mail"),
            Some(NotificationImage::Pixels(_))
        ));
        hints.insert("image-data".into(), wire(0, 0, 0, true, 4, vec![]));
        assert!(
            matches!(select(&hints,"mail"),Some(NotificationImage::Source(source)) if source.value()=="/tmp/fixture.png")
        );
        hints.clear();
        hints.insert("icon_data".into(), wire(1, 1, 4, true, 4, vec![1, 2, 3, 4]));
        assert!(
            matches!(select(&hints,"mail"),Some(NotificationImage::Source(source)) if source.value()=="mail")
        );
    }
}
