//! Private FFmpeg ownership adapter. Only initialized visible YUV samples leave
//! here. Row padding is never assumed initialized merely because it is in bounds.
use super::*;
use lucent_domain::video_frame::{Bt709, Bytes, ChromaSiting, Plane, Range, Transfer, Yuv420Frame};

struct ReferencedFrame(ffmpeg::frame::Video);
struct PackedOwner {
    frame: Arc<ReferencedFrame>,
    plane: usize,
    length: usize,
}
impl AsRef<[u8]> for PackedOwner {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: admit validates a reference-counted AVBuffer containing this
        // span. A packed span consists ONLY of initialized decoded samples,
        // with no row gaps. The private AVFrame owns references to its buffers,
        // is never mutated, and remains alive through this owner. Decoder reuse
        // must retain the referenced buffer rather than mutate this publication.
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
/// Only fully admitted progressive, explicit-metadata 8-bit YUV420 is native.
/// None means capability fallback, not a fabricated color/default assumption.
pub(super) fn admit(
    source: &ffmpeg::frame::Video,
) -> Result<Option<(Yuv420Frame, PlanePublication)>> {
    if source.format() != ffmpeg::format::Pixel::YUV420P
        || source.is_interlaced()
        || source.aspect_ratio() != ffmpeg::Rational(1, 1)
    {
        return Ok(None);
    }
    let Some(color) = color(source) else {
        return Ok(None);
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
        return Ok(None);
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
        return Ok(None);
    }
    let sizes = [
        (source.width(), source.height()),
        (source.width().div_ceil(2), source.height().div_ceil(2)),
        (source.width().div_ceil(2), source.height().div_ceil(2)),
    ];
    for (index, (width, height)) in sizes.into_iter().enumerate() {
        if raw.linesize[index] <= 0 {
            return Ok(None);
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
            return Ok(None);
        }
        let buffer = unsafe { &*buffer };
        let offset = (raw.data[index] as usize)
            .checked_sub(buffer.data as usize)
            .ok_or_else(failed)?;
        if offset.checked_add(span).is_none_or(|end| end > buffer.size) {
            return Err(failed());
        }
    }
    let mut retained = ffmpeg::frame::Video::empty();
    // SAFETY: destination is newly allocated/unreferenced. Source has real
    // AVBufferRefs, so av_frame_ref references data rather than copying it.
    // Video::clone is deliberately NOT used: ffmpeg-next implements it as copy.
    if unsafe { retained.as_ptr().is_null() }
        || unsafe { ffmpeg::ffi::av_frame_ref(retained.as_mut_ptr(), source.as_ptr()) } < 0
    {
        return Err(failed());
    }
    let owner = Arc::new(ReferencedFrame(retained));
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
    let frame = Yuv420Frame::new(source.width(), source.height(), planes, color)
        .map_err(|e| DomainError::Invalid(e.to_string()))?;
    Ok(Some((frame, publication)))
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn native_packed_yuv_uses_references_and_survives_source_drop() {
        let source = initialized(64, 4);
        let pointers = unsafe { (*source.as_ptr()).data };
        let (frame, stats) = admit(&source).unwrap().unwrap();
        assert_eq!(stats.copied_bytes, 0);
        assert_eq!(stats.referenced_bytes, 64 * 4 * 3 / 2);
        for (plane, pointer) in frame.planes().iter().zip(pointers) {
            assert_eq!(plane.bytes().as_ptr(), pointer as *const u8);
        }
        drop(source);
        assert_eq!(frame.planes()[0].row(3).unwrap(), &[64; 64]);
        assert_eq!(frame.planes()[2].row(1).unwrap(), &[192; 32]);
    }
    #[test]
    fn native_odd_padded_rows_copy_only_initialized_visible_samples() {
        let source = initialized(5, 3);
        let (frame, stats) = admit(&source).unwrap().unwrap();
        assert_eq!(stats.referenced_bytes, 0);
        assert_eq!(stats.copied_bytes, 27);
        assert_eq!(frame.byte_len(), 27);
        for (plane, value) in frame.planes().iter().zip([64, 128, 192]) {
            assert_eq!(plane.stride(), plane.dimensions().0);
            assert!(plane.bytes().iter().all(|v| *v == value));
        }
    }
    #[test]
    fn native_frame_reference_prevents_later_writes_from_mutating_publication() {
        let mut source = initialized(64, 4);
        let (frame, _) = admit(&source).unwrap().unwrap();
        // The retained AVFrame must make FFmpeg allocate writable storage for
        // the producer, rather than modify the already published visible bytes.
        unsafe {
            assert_eq!(ffmpeg::ffi::av_frame_make_writable(source.as_mut_ptr()), 0);
            std::ptr::write_bytes((*source.as_mut_ptr()).data[0], 220, 64);
        }
        assert_eq!(frame.planes()[0].row(0).unwrap(), &[64; 64]);
    }
    #[test]
    fn native_unknown_tags_alpha_crop_or_negative_stride_keep_capability_fallback() {
        let mut source = initialized(64, 4);
        source.set_color_space(ffmpeg::color::Space::Unspecified);
        assert!(admit(&source).unwrap().is_none());
        let mut source = initialized(64, 4);
        unsafe {
            (*source.as_mut_ptr()).crop_left = 1;
        }
        assert!(admit(&source).unwrap().is_none());
        let mut source = initialized(64, 4);
        unsafe {
            (*source.as_mut_ptr()).linesize[0] = -64;
        }
        assert!(admit(&source).unwrap().is_none());
        let rgba = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, 4, 4);
        assert!(admit(&rgba).unwrap().is_none());
    }
    #[test]
    fn native_plane_bounds_are_rejected_before_a_slice_is_constructed() {
        let mut source = initialized(64, 4);
        unsafe {
            let original = (*source.as_ptr()).data[0];
            let buffer = ffmpeg::ffi::av_frame_get_plane_buffer(source.as_ptr(), 0);
            (*source.as_mut_ptr()).data[0] = (*buffer).data.add((*buffer).size - 1);
            assert!(admit(&source).is_err());
            (*source.as_mut_ptr()).data[0] = original;
        }
    }
}
