//! Backend input validation and geometry-buffer sizing. No client layout policy.
use crate::{Error, raster};
use lucent_api::{ClipShape, Color, Rect};
use lucent_ui::Paint;

pub(super) const INSTANCE_BYTES: u64 = 96;
pub(super) const CLIP_BYTES: u64 = 32;
// Indices are encoded in f32 instance attributes, which exactly represent
// integers only through 2^24. Geometry uses the same finite numeric envelope.
const EXACT_INTEGER_LIMIT: usize = 1 << 24;
const COORDINATE_LIMIT: f32 = EXACT_INTEGER_LIMIT as f32;

#[derive(Clone, Copy, Debug)]
pub(super) struct Geometry {
    pub instances: BufferSize,
    pub clips: BufferSize,
    pub clip_count: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BufferSize {
    pub capacity: usize,
    pub bytes: u64,
}
impl BufferSize {
    pub fn new(count: usize, current: usize, stride: u64, limit: u64) -> Result<Self, Error> {
        let count = count.max(1);
        let maximum = (limit / stride).min(isize::MAX as u64 / stride);
        if count as u64 > maximum || current as u64 > maximum {
            return Err("scene geometry exceeds device or host buffer limits".into());
        }
        let capacity = if count <= current {
            current
        } else {
            // Geometric growth must not reject an otherwise valid exact fit.
            count
                .checked_next_power_of_two()
                .filter(|n| *n as u64 <= maximum)
                .unwrap_or(count)
        };
        Ok(Self {
            capacity,
            bytes: capacity as u64 * stride,
        })
    }
}

fn scalar(value: f32) -> Result<(), Error> {
    if !value.is_finite() || value.abs() > COORDINATE_LIMIT {
        return Err("scene geometry is non-finite or exceeds the f32 coordinate envelope".into());
    }
    Ok(())
}
fn positive(value: f32) -> Result<(), Error> {
    scalar(value)?;
    if value < 0. {
        return Err("negative scene dimension or effect extent".into());
    }
    Ok(())
}
fn rect(rect: Rect) -> Result<(), Error> {
    scalar(rect.x)?;
    scalar(rect.y)?;
    positive(rect.w)?;
    positive(rect.h)?;
    scalar(rect.x + rect.w)?;
    scalar(rect.y + rect.h)
}
fn color(color: Color) -> Result<(), Error> {
    if [color.0, color.1, color.2, color.3]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > COORDINATE_LIMIT)
    {
        return Err("scene color exceeds the finite numeric envelope".into());
    }
    Ok(())
}

pub(super) fn validate(
    paint: &[Paint],
    scale: f32,
    fonts: &[lucent_ui::text::Font],
    limits: &wgpu::Limits,
    current_instances: usize,
    current_clips: usize,
) -> Result<Geometry, Error> {
    if paint.len() > u32::MAX as usize {
        return Err("scene instance count exceeds draw indexing".into());
    }
    let instances = BufferSize::new(
        paint.len(),
        current_instances,
        INSTANCE_BYTES,
        limits.max_buffer_size,
    )?;
    let mut clip_count = 0_usize;
    for (index, primitive) in paint.iter().enumerate() {
        let checked = || -> Result<_, Error> {
            let (bounds, clip, shade) = match primitive {
                Paint::Shape {
                    rect,
                    clip,
                    color,
                    radius,
                } => {
                    positive(*radius)?;
                    (*rect, clip, *color)
                }
                Paint::Outline {
                    rect,
                    clip,
                    color,
                    radius,
                    width,
                } => {
                    positive(*radius)?;
                    positive(*width)?;
                    (*rect, clip, *color)
                }
                Paint::Shadow {
                    rect,
                    clip,
                    color,
                    radius,
                    blur,
                } => {
                    positive(*radius)?;
                    positive(*blur)?;
                    (*rect, clip, *color)
                }
                Paint::Image {
                    rect: bounds,
                    clip,
                    data,
                    uv,
                    radius,
                    opacity,
                    tint,
                } => {
                    positive(*radius)?;
                    positive(*opacity)?;
                    rect(*uv)?;
                    if data.width() > limits.max_texture_dimension_2d
                        || data.height() > limits.max_texture_dimension_2d
                    {
                        return Err("image exceeds device texture dimension limit".into());
                    }
                    color(*tint)?;
                    (*bounds, clip, tint.alpha(tint.3 * *opacity))
                }
                Paint::Text {
                    rect,
                    clip,
                    text,
                    size,
                    face,
                    color,
                    ..
                } => {
                    if !text.is_empty() {
                        raster::validate_request(text, *size, scale)?;
                        if fonts.get(*face).or_else(|| fonts.first()).is_none() {
                            return Err("text requires at least one font".into());
                        }
                    }
                    (*rect, clip, *color)
                }
            };
            rect(bounds)?;
            rect(clip.bounds)?;
            color(shade)?;
            for shape in clip.shapes() {
                match shape {
                    ClipShape::Rounded(rounded) => {
                        rect(rounded.rect)?;
                        positive(rounded.radius)?;
                    }
                    ClipShape::Diagonal(diagonal) => {
                        rect(diagonal.rect)?;
                        scalar(diagonal.rect.w + diagonal.rect.h)?;
                        if !diagonal.progress().is_finite()
                            || !(0. ..=1.).contains(&diagonal.progress())
                        {
                            return Err(
                                "diagonal clip progress exceeds its normalized range".into()
                            );
                        }
                    }
                }
            }
            Ok(clip.shapes().len())
        };
        clip_count = clip_count
            .checked_add(checked().map_err(|e| format!("scene primitive {index}: {e}"))?)
            .ok_or("scene clip count overflow")?;
        if clip_count > EXACT_INTEGER_LIMIT {
            return Err("scene clip count exceeds exact shader indexing".into());
        }
    }
    let clips = BufferSize::new(
        clip_count,
        current_clips,
        CLIP_BYTES,
        limits
            .max_buffer_size
            .min(limits.max_storage_buffer_binding_size),
    )?;
    Ok(Geometry {
        instances,
        clips,
        clip_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_growth_respects_exact_device_and_host_limits() {
        assert_eq!(
            BufferSize::new(3, 1, 96, 288).unwrap(),
            BufferSize {
                capacity: 3,
                bytes: 288
            }
        );
        assert_eq!(
            BufferSize::new(3, 1, 96, 384).unwrap(),
            BufferSize {
                capacity: 4,
                bytes: 384
            }
        );
        assert_eq!(BufferSize::new(2, 4, 96, 384).unwrap().capacity, 4);
        assert!(BufferSize::new(4, 1, 96, 383).is_err());
        assert!(BufferSize::new(usize::MAX, 1, 96, u64::MAX).is_err());
        assert!(BufferSize::new(1, usize::MAX, 32, u64::MAX).is_err());
    }

    #[test]
    fn non_finite_and_extreme_geometry_fails_before_preparation() {
        let limits = wgpu::Limits::default();
        let shape = |value| Paint::Shape {
            rect: Rect::new(value, 0., 16., 16.),
            clip: Rect::new(0., 0., 32., 32.).into(),
            color: Color::hex(0xffffff),
            radius: 2.,
        };
        for value in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
            COORDINATE_LIMIT,
        ] {
            assert!(validate(&[shape(value)], 1., &[], &limits, 1, 1).is_err());
        }
        assert!(validate(&[shape(-8.)], 1., &[], &limits, 1, 1).is_ok());
        assert!(positive(-1.).is_err());
        assert!(color(Color(0., f32::NAN, 0., 1.)).is_err());
    }

    #[test]
    fn nested_clip_storage_uses_binding_limit_not_only_buffer_limit() {
        let limits = wgpu::Limits {
            max_buffer_size: 1024,
            max_storage_buffer_binding_size: 32,
            ..Default::default()
        };
        let bounds = Rect::new(0., 0., 16., 16.);
        let shape = Paint::Shape {
            rect: bounds,
            clip: lucent_api::Clip::from(bounds)
                .with_rounded(bounds, 2.)
                .with_rounded(bounds, 3.),
            radius: 0.,
            color: Color::hex(0xffffff),
        };
        assert!(validate(&[shape], 1., &[], &limits, 1, 1).is_err());
    }

    #[test]
    fn diagonal_clips_share_shape_storage_and_geometry_validation() {
        let bounds = Rect::new(0., 0., 16., 16.);
        let paint = |clip| Paint::Shape {
            rect: bounds,
            clip,
            radius: 0.,
            color: Color::hex(0xffffff),
        };
        let clip = lucent_api::Clip::from(bounds)
            .with_rounded(bounds, 2.)
            .with_diagonal_reveal(bounds, 0.5);
        let geometry = validate(
            &[paint(clip.clone())],
            1.,
            &[],
            &wgpu::Limits::default(),
            1,
            1,
        )
        .unwrap();
        assert_eq!(geometry.clip_count, 2);
        let bounded = wgpu::Limits {
            max_storage_buffer_binding_size: CLIP_BYTES,
            ..Default::default()
        };
        assert!(validate(&[paint(clip)], 1., &[], &bounded, 1, 1).is_err());
        let invalid = lucent_api::Clip::from(bounds)
            .with_diagonal_reveal(Rect::new(f32::NAN, 0., 16., 16.), 0.5);
        assert!(validate(&[paint(invalid)], 1., &[], &wgpu::Limits::default(), 1, 1).is_err());
    }

    #[test]
    fn missing_fonts_and_oversized_images_are_rejected_at_the_boundary() {
        let limits = wgpu::Limits {
            max_texture_dimension_2d: 16,
            ..Default::default()
        };
        let bounds = Rect::new(0., 0., 16., 16.);
        let text = Paint::Text {
            rect: bounds,
            clip: bounds.into(),
            text: "hello".into(),
            size: 14.,
            face: 0,
            color: Color::hex(0xffffff),
            align: lucent_api::Align::Start,
        };
        assert!(
            validate(&[text], 1., &[], &limits, 1, 1)
                .unwrap_err()
                .to_string()
                .contains("font")
        );
        let image = Paint::Image {
            rect: bounds,
            clip: bounds.into(),
            data: std::sync::Arc::new(
                lucent_api::ImageData::from_rgba("oversized", 17, 1, vec![255; 68]).unwrap(),
            ),
            uv: Rect::new(0., 0., 1., 1.),
            radius: 0.,
            opacity: 1.,
            tint: Color::hex(0xffffff),
        };
        assert!(
            validate(&[image], 1., &[], &limits, 1, 1)
                .unwrap_err()
                .to_string()
                .contains("texture")
        );
    }
}
