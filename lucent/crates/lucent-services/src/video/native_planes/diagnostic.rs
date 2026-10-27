//! Test-only decoded metadata. Never reads pixels, side-data payloads, source
//! paths, pointer values or free-form frame dictionaries; no admission policy.
use ffmpeg::util::{chroma, color, frame::side_data::Type};
use ffmpeg_next as ffmpeg;

pub(crate) const PREFIX: &str = "LUCENT_NATIVE_ADMISSION_META ";

pub(crate) fn emit(source: &ffmpeg::frame::Video, phase: &str, guard: Option<&str>) {
    eprintln!("{}{}", PREFIX, snapshot(source, phase, guard));
}

pub(crate) fn snapshot(
    source: &ffmpeg::frame::Video,
    phase: &str,
    guard: Option<&str>,
) -> serde_json::Value {
    // SAFETY: the caller borrows an owned, valid decoded/initialized AVFrame.
    // Only header fields and bounded side-data KIND integers are inspected.
    // No mutable slice, pixel read, address or side-data payload escapes.
    let raw = unsafe { &*source.as_ptr() };
    let count = usize::try_from(raw.nb_side_data).ok();
    let mut kinds = Vec::new();
    let mut null_entries = 0;
    let valid_inventory = count.is_some_and(|n| n <= 64 && (n == 0 || !raw.side_data.is_null()));
    if valid_inventory {
        for index in 0..count.unwrap() {
            // The AVFrame owns its nb_side_data pointer array; valid FFmpeg
            // frames retain these entries while this immutable borrow lives.
            let data = unsafe { *raw.side_data.add(index) };
            if data.is_null() {
                null_entries += 1;
            } else {
                kinds.push(unsafe { (*data).type_ as i32 });
            }
        }
    }
    let sar = source.aspect_ratio();
    serde_json::json!({
        "schema":1,"phase":phase,"rejecting_guard":guard,
        "dimensions":[source.width(),source.height()],
        "pixel_format":{"raw":raw.format,"mapped":format!("{:?}",source.format())},
        "color_space":{"raw":raw.colorspace as i32,"mapped":format!("{:?}",source.color_space())},
        "primaries":{"raw":raw.color_primaries as i32,"mapped":format!("{:?}",source.color_primaries())},
        "range":{"raw":raw.color_range as i32,"mapped":format!("{:?}",source.color_range())},
        "transfer":{"raw":raw.color_trc as i32,"mapped":format!("{:?}",source.color_transfer_characteristic())},
        "chroma":{"raw":raw.chroma_location as i32,"mapped":format!("{:?}",source.chroma_location())},
        "raw_sar":[raw.sample_aspect_ratio.num,raw.sample_aspect_ratio.den],
        "mapped_sar":[sar.0,sar.1],
        "raw_flags":raw.flags,"mapped_flags":source.flags().bits(),
        "interlaced":source.is_interlaced(),"corrupt":source.is_corrupt(),
        "decode_error_flags":raw.decode_error_flags,
        "crop":[raw.crop_left,raw.crop_top,raw.crop_right,raw.crop_bottom],
        "strides":[raw.linesize[0],raw.linesize[1],raw.linesize[2]],
        "buffer_zero_present":!raw.buf[0].is_null(),
        "side_data_count":raw.nb_side_data,"side_data_kind_ids":kinds,
        "side_data_inventory_valid":valid_inventory && null_entries==0,
        "side_data_null_entries":null_entries,
        "blocked_side_data":{
            "icc_profile":source.side_data(Type::IccProfile).is_some(),
            "mastering_display":source.side_data(Type::MasteringDisplayMetadata).is_some(),
            "content_light":source.side_data(Type::ContentLightLevel).is_some(),
            "display_matrix":source.side_data(Type::DisplayMatrix).is_some()
        },
        "checks":{
            "yuv420p":source.format()==ffmpeg::format::Pixel::YUV420P,
            "progressive":!source.is_interlaced(),
            "square_sar":sar==ffmpeg::Rational(1,1),
            "bt709_matrix":source.color_space()==color::Space::BT709,
            "bt709_primaries":source.color_primaries()==color::Primaries::BT709,
            "known_range":matches!(source.color_range(),color::Range::MPEG|color::Range::JPEG),
            "known_transfer":matches!(source.color_transfer_characteristic(),color::TransferCharacteristic::BT709|color::TransferCharacteristic::IEC61966_2_1),
            "known_chroma":matches!(source.chroma_location(),chroma::Location::Left|chroma::Location::Center)
        }
    })
}
