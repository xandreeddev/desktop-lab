//! Private FFmpeg ownership adapter. Only initialized visible YUV samples leave
//! here. Row padding is never assumed initialized merely because it is in bounds.
use super::*;
use lucent_domain::video_frame::{Bt709, Bytes, ChromaSiting, Plane, Range, Transfer, Yuv420Frame};

#[cfg(test)]
pub(super) mod diagnostic;

struct DecodedFrameOwner(ffmpeg::frame::Video);
struct PackedOwner {
    frame: Arc<DecodedFrameOwner>,
    plane: usize,
    length: usize,
}
impl AsRef<[u8]> for PackedOwner {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: admit validates a reference-counted AVBuffer containing this
        // span. A packed span consists ONLY of initialized decoded samples,
        // with no row gaps. Admission CONSUMES the only Rust Video handle and
        // moves it into this private immutable owner. No caller retains a safe
        // data_mut path. FFmpeg decoder reuse must respect its AVBufferRefs.
        unsafe {
            std::slice::from_raw_parts((*self.frame.0.as_ptr()).data[self.plane], self.length)
        }
    }
}
fn color(frame: &ffmpeg::frame::Video) -> Option<Bt709> {
    use ffmpeg::util::{chroma, color};
    if frame.color_space() != color::Space::BT709
        || frame.color_primaries() != color::Primaries::BT709
    {
        return None;
    }
    let range = match frame.color_range() {
        color::Range::MPEG => Range::Limited,
        color::Range::JPEG => Range::Full,
        _ => return None,
    };
    let transfer = match frame.color_transfer_characteristic() {
        color::TransferCharacteristic::BT709 => Transfer::Bt709,
        color::TransferCharacteristic::IEC61966_2_1 => Transfer::Srgb,
        _ => return None,
    };
    let chroma = match frame.chroma_location() {
        chroma::Location::Left => ChromaSiting::Left,
        chroma::Location::Center => ChromaSiting::Center,
        _ => return None,
    };
    Some(Bt709 {
        range,
        transfer,
        chroma,
    })
}
pub(super) enum Admission {
    Native(Yuv420Frame, PlanePublication),
    Rgba(ffmpeg::frame::Video),
}
/// Admission consumes the mutable decoder-frame handle. Returning immutable
/// views while retaining a caller's Video would be unsound: ffmpeg-next's SAFE
/// data_mut does not perform COW. Unsupported capability returns the SAME owned
/// frame for RGBA conversion, without cloning or guessing color metadata.
pub(super) fn admit(source: ffmpeg::frame::Video) -> Result<Admission> {
    if source.format() != ffmpeg::format::Pixel::YUV420P
        || source.is_interlaced()
        || source.aspect_ratio() != ffmpeg::Rational(1, 1)
    {
        #[cfg(test)]
        diagnostic::emit(&source, "admission-rejected", Some("format-interlace-sar"));
        return Ok(Admission::Rgba(source));
    }
    let Some(color) = color(&source) else {
        #[cfg(test)]
        diagnostic::emit(&source, "admission-rejected", Some("color"));
        return Ok(Admission::Rgba(source));
    };
    use ffmpeg::util::frame::side_data::Type;
    if [
        Type::IccProfile,
        Type::MasteringDisplayMetadata,
        Type::ContentLightLevel,
        Type::DisplayMatrix,
    ]
    .iter()
    .any(|kind| source.side_data(*kind).is_some())
    {
        #[cfg(test)]
        diagnostic::emit(&source, "admission-rejected", Some("blocked-side-data"));
        return Ok(Admission::Rgba(source));
    }
    dimensions(source.width(), source.height())?;
    // SAFETY: source is an owned frame returned by the decoder (or an initialized
    // adapter test fixture); the pointer is used only while source is borrowed.
    let raw = unsafe { &*source.as_ptr() };
    if raw.crop_left != 0
        || raw.crop_top != 0
        || raw.crop_right != 0
        || raw.crop_bottom != 0
        || raw.decode_error_flags != 0
        || source.is_corrupt()
        || raw.buf[0].is_null()
    {
        #[cfg(test)]
        diagnostic::emit(&source, "admission-rejected", Some("crop-errors-buffer"));
        return Ok(Admission::Rgba(source));
    }
    let sizes = [
        (source.width(), source.height()),
        (source.width().div_ceil(2), source.height().div_ceil(2)),
        (source.width().div_ceil(2), source.height().div_ceil(2)),
    ];
    for (index, (width, height)) in sizes.into_iter().enumerate() {
        if raw.linesize[index] <= 0 {
            #[cfg(test)]
            diagnostic::emit(&source, "admission-rejected", Some("nonpositive-stride"));
            return Ok(Admission::Rgba(source));
        }
        let stride = raw.linesize[index] as usize;
        if stride < width as usize {
            return Err(failed());
        }
        let span = (height as usize - 1)
            .checked_mul(stride)
            .and_then(|v| v.checked_add(width as usize))
            .ok_or_else(failed)?;
        // SAFETY: the borrowed frame is valid; this read-only API identifies its
        // reference-counted plane owner. Bounds do NOT imply initialized padding.
        let buffer =
            unsafe { ffmpeg::ffi::av_frame_get_plane_buffer(source.as_ptr(), index as i32) };
        if buffer.is_null() || raw.data[index].is_null() {
            #[cfg(test)]
            diagnostic::emit(&source, "admission-rejected", Some("plane-owner"));
            return Ok(Admission::Rgba(source));
        }
        let buffer = unsafe { &*buffer };
        let offset = (raw.data[index] as usize)
            .checked_sub(buffer.data as usize)
            .ok_or_else(failed)?;
        if offset.checked_add(span).is_none_or(|end| end > buffer.size) {
            return Err(failed());
        }
    }
    #[cfg(test)]
    diagnostic::emit(&source, "admission-accepted", None);
    let (width, height) = (source.width(), source.height());
    // Move the existing, reference-counted decoded AVFrame into its private
    // immutable owner. av_frame_ref would leave a mutable alias in a borrowed
    // caller; Video::clone would deep-copy pixels. Neither is needed here.
    let owner = Arc::new(DecodedFrameOwner(source));
    let mut planes = Vec::with_capacity(3);
    let mut publication = PlanePublication::default();
    for (index, (width, height)) in sizes.into_iter().enumerate() {
        let stride = raw.linesize[index] as usize;
        let length = width as usize * height as usize;
        let bytes = if stride == width as usize {
            publication.referenced_bytes += length;
            Bytes::from_owner(PackedOwner {
                frame: owner.clone(),
                plane: index,
                length,
            })
        } else {
            let mut packed = Vec::new();
            packed.try_reserve_exact(length).map_err(|_| failed())?;
            for row in 0..height as usize {
                // SAFETY: only visible samples in this decoder-initialized row
                // are read. Padding and bytes after the final visible row are
                // excluded, even if FFmpeg allocated them without initialization.
                let visible = unsafe {
                    std::slice::from_raw_parts(raw.data[index].add(row * stride), width as usize)
                };
                packed.extend_from_slice(visible);
            }
            publication.copied_bytes += length;
            Bytes::from(packed)
        };
        planes.push(
            Plane::new(bytes, width, height, width)
                .map_err(|e| DomainError::Invalid(e.to_string()))?,
        );
    }
    let planes: [Plane; 3] = planes.try_into().map_err(|_| failed())?;
    let frame = Yuv420Frame::new(width, height, planes, color)
        .map_err(|e| DomainError::Invalid(e.to_string()))?;
    Ok(Admission::Native(frame, publication))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn native(source: ffmpeg::frame::Video) -> (Yuv420Frame, PlanePublication) {
        match admit(source).unwrap() {
            Admission::Native(frame, publication) => (frame, publication),
            Admission::Rgba(_) => panic!("known fixture must admit native ownership"),
        }
    }
    fn initialized(width: u32, height: u32) -> ffmpeg::frame::Video {
        let mut frame = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::YUV420P, width, height);
        frame.set_color_space(ffmpeg::color::Space::BT709);
        frame.set_color_primaries(ffmpeg::color::Primaries::BT709);
        frame.set_color_range(ffmpeg::color::Range::MPEG);
        frame
            .set_color_transfer_characteristic(ffmpeg::color::TransferCharacteristic::IEC61966_2_1);
        // FFmpeg exposes no safe chroma-location setter; initialize header-only
        // fields before publication. Visible rows are initialized with raw writes
        // without ever constructing a Rust slice over uninitialized padding.
        unsafe {
            let raw = &mut *frame.as_mut_ptr();
            raw.chroma_location = ffmpeg::ffi::AVChromaLocation::AVCHROMA_LOC_LEFT;
            raw.sample_aspect_ratio = ffmpeg::ffi::AVRational { num: 1, den: 1 };
            for (plane, (w, h)) in [
                (width, height),
                (width.div_ceil(2), height.div_ceil(2)),
                (width.div_ceil(2), height.div_ceil(2)),
            ]
            .into_iter()
            .enumerate()
            {
                assert!(raw.linesize[plane] >= w as i32 && !raw.data[plane].is_null());
                for row in 0..h as usize {
                    std::ptr::write_bytes(
                        raw.data[plane].add(row * raw.linesize[plane] as usize),
                        [64, 128, 192][plane],
                        w as usize,
                    );
                }
            }
        }
        frame
    }
    #[test]
    fn native_diagnostic_reports_unknown_sar_and_only_bounded_metadata() {
        let mut source = initialized(5, 3);
        unsafe {
            (*source.as_mut_ptr()).sample_aspect_ratio = ffmpeg::ffi::AVRational { num: 0, den: 1 };
        }
        source
            .new_side_data(ffmpeg::util::frame::side_data::Type::IccProfile, 4)
            .unwrap();
        let value =
            diagnostic::snapshot(&source, "admission-rejected", Some("format-interlace-sar"));
        assert_eq!(value["raw_sar"], serde_json::json!([0, 1]));
        assert_eq!(value["mapped_sar"], serde_json::json!([0, 1]));
        assert_eq!(value["checks"]["square_sar"], false);
        assert_eq!(value["checks"]["yuv420p"], true);
        assert_eq!(value["blocked_side_data"]["icc_profile"], true);
        assert_eq!(value["side_data_kind_ids"].as_array().unwrap().len(), 1);
        assert!(value.to_string().len() < 16384);
        for key in [
            "pointer", "address", "path", "pixels", "payload", "metadata",
        ] {
            assert!(!value.as_object().unwrap().contains_key(key));
        }
    }

    #[test]
    fn native_packed_yuv_moves_the_only_rust_frame_owner_and_survives_adapter_return() {
        let source = initialized(64, 4);
        let pointers = unsafe { (*source.as_ptr()).data };
        let (frame, stats) = native(source);
        assert_eq!(stats.copied_bytes, 0);
        assert_eq!(stats.referenced_bytes, 64 * 4 * 3 / 2);
        for (plane, pointer) in frame.planes().iter().zip(pointers) {
            assert_eq!(plane.bytes().as_ptr(), pointer as *const u8);
        }
        assert_eq!(frame.planes()[0].row(3).unwrap(), &[64; 64]);
        assert_eq!(frame.planes()[2].row(1).unwrap(), &[192; 32]);
    }
    #[test]
    fn native_odd_padded_rows_copy_only_initialized_visible_samples() {
        let source = initialized(5, 3);
        let (frame, stats) = native(source);
        assert_eq!(stats.referenced_bytes, 0);
        assert_eq!(stats.copied_bytes, 27);
        assert_eq!(frame.byte_len(), 27);
        for (plane, value) in frame.planes().iter().zip([64, 128, 192]) {
            assert_eq!(plane.stride(), plane.dimensions().0);
            assert!(plane.bytes().iter().all(|v| *v == value));
        }
    }
    #[test]
    fn native_consumed_owner_leaves_only_immutable_views_until_the_last_clone_drops() {
        let (frame, _) = native(initialized(64, 4));
        let first = frame.planes()[0].bytes().clone();
        let last = first.clone();
        assert_eq!(first.as_ptr(), last.as_ptr());
        drop(frame);
        assert_eq!(first.as_ref(), &[64; 64 * 4]);
        drop(first);
        assert_eq!(last.as_ref(), &[64; 64 * 4]);
        // There is deliberately no mutable Video handle left: admission moved
        // it, and neither the domain value nor Bytes exposes the private owner.
    }
    #[test]
    fn native_unknown_tags_alpha_crop_or_negative_stride_keep_capability_fallback() {
        let mut source = initialized(64, 4);
        source.set_color_space(ffmpeg::color::Space::Unspecified);
        let original = unsafe { (source.as_ptr(), (*source.as_ptr()).data) };
        let Admission::Rgba(recovered) = admit(source).unwrap() else {
            panic!("unknown metadata must return the owned source");
        };
        assert_eq!(unsafe { recovered.as_ptr() }, original.0);
        assert_eq!(unsafe { (*recovered.as_ptr()).data }, original.1);
        let mut source = initialized(64, 4);
        unsafe {
            (*source.as_mut_ptr()).crop_left = 1;
        }
        assert!(matches!(admit(source).unwrap(), Admission::Rgba(_)));
        let mut source = initialized(64, 4);
        unsafe {
            (*source.as_mut_ptr()).linesize[0] = -64;
        }
        assert!(matches!(admit(source).unwrap(), Admission::Rgba(_)));
        let rgba = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, 4, 4);
        assert!(matches!(admit(rgba).unwrap(), Admission::Rgba(_)));
    }
    #[test]
    fn native_plane_bounds_are_rejected_before_a_slice_is_constructed() {
        let mut source = initialized(64, 4);
        unsafe {
            let buffer = ffmpeg::ffi::av_frame_get_plane_buffer(source.as_ptr(), 0);
            (*source.as_mut_ptr()).data[0] = (*buffer).data.add((*buffer).size - 1);
            // AVFrame release owns the unchanged AVBufferRef; it never frees
            // this deliberately invalid visible-data pointer independently.
            assert!(admit(source).is_err());
        }
    }
}
