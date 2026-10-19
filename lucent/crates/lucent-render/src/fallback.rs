//! One-glyph fallback preparation. Large immutable fonts remain borrowed tables;
//! no catalog-wide outline parsing or retained per-glyph bitmap map is needed.
use crate::{Error, raster::validate_glyph};
use lucent_ui::text::RasterFace;
use tiny_skia::{FillRule, IntSize, Mask, PathBuilder, Transform};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

pub(super) enum Coverage {
    Prepared(Vec<u8>),
    Outline(Mask),
}
impl std::ops::Deref for Coverage {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Prepared(pixels) => pixels,
            Self::Outline(mask) => mask.data(),
        }
    }
}

// A single outline can have many curves, but cannot grow an unbounded scratch
// path. This is a preparation safety bound, not a UI density or cache budget.
const OUTLINE_COMMAND_LIMIT: usize = 65_536;

fn face(bytes: &[u8], index: u32) -> Result<Face<'_>, Error> {
    Face::parse(bytes, index).map_err(|_| "invalid fallback font tables".into())
}

/// Fontdue-compatible bitmap bearings and dimensions, without preparing other
/// glyphs. All finite bounds and allocation limits are checked before coverage.
pub(super) fn metrics(
    input: RasterFace<'_>,
    id: u16,
    pixels: f32,
) -> Result<fontdue::Metrics, Error> {
    if let RasterFace::Prepared(font) = input {
        return Ok(font.metrics_indexed(id, pixels));
    }
    let RasterFace::Outline { bytes, index } = input else {
        unreachable!()
    };
    let face = face(bytes, index)?;
    if !pixels.is_normal() || pixels <= 0. || id >= face.number_of_glyphs() {
        return Err("invalid fallback glyph request".into());
    }
    let scale = pixels / f32::from(face.units_per_em());
    if !scale.is_normal() || scale <= 0. {
        return Err("invalid fallback glyph scale".into());
    }
    let id = GlyphId(id);
    let mut metrics = fontdue::Metrics {
        advance_width: f32::from(face.glyph_hor_advance(id).unwrap_or(0)) * scale,
        advance_height: f32::from(face.glyph_ver_advance(id).unwrap_or(0)) * scale,
        ..Default::default()
    };
    if let Some(bounds) = face.glyph_bounding_box(id) {
        let left = f32::from(bounds.x_min) * scale;
        let bottom = f32::from(bounds.y_min) * scale;
        let right = f32::from(bounds.x_max) * scale;
        let top = f32::from(bounds.y_max) * scale;
        let coordinates = [left.floor(), bottom.floor(), right.ceil(), top.ceil()];
        if coordinates
            .iter()
            .any(|n| !n.is_finite() || f64::from(*n).abs() > f64::from(i32::MAX))
            || right < left
            || top < bottom
        {
            return Err("fallback glyph bounds exceed the finite raster envelope".into());
        }
        metrics.xmin = coordinates[0] as i32;
        metrics.ymin = coordinates[1] as i32;
        metrics.width = (coordinates[2] - coordinates[0]) as usize;
        metrics.height = (coordinates[3] - coordinates[1]) as usize;
        metrics.bounds = fontdue::OutlineBounds {
            xmin: left,
            ymin: bottom,
            width: right - left,
            height: top - bottom,
        };
    }
    validate_glyph(&metrics)?;
    Ok(metrics)
}

struct Outline {
    path: PathBuilder,
    commands: usize,
    invalid: bool,
}
impl Outline {
    fn command(&mut self, points: &[f32]) -> bool {
        self.commands += 1;
        self.invalid |=
            self.commands > OUTLINE_COMMAND_LIMIT || points.iter().any(|p| !p.is_finite());
        !self.invalid
    }
}
impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.command(&[x, y]) {
            self.path.move_to(x, y);
        }
    }
    fn line_to(&mut self, x: f32, y: f32) {
        if self.command(&[x, y]) {
            self.path.line_to(x, y);
        }
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        if self.command(&[x1, y1, x, y]) {
            self.path.quad_to(x1, y1, x, y);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        if self.command(&[x1, y1, x2, y2, x, y]) {
            self.path.cubic_to(x1, y1, x2, y2, x, y);
        }
    }
    fn close(&mut self) {
        if self.command(&[]) {
            self.path.close();
        }
    }
}

pub(super) fn rasterize(
    input: RasterFace<'_>,
    id: u16,
    pixels: f32,
    metrics: &fontdue::Metrics,
) -> Result<Coverage, Error> {
    validate_glyph(metrics)?;
    if let RasterFace::Prepared(font) = input {
        // Preserve the established primary-face raster path byte for byte.
        return Ok(Coverage::Prepared(font.rasterize_indexed(id, pixels).1));
    }
    let RasterFace::Outline { bytes, index } = input else {
        unreachable!()
    };
    let face = face(bytes, index)?;
    if id >= face.number_of_glyphs() || !pixels.is_normal() || pixels <= 0. {
        return Err("invalid fallback glyph request".into());
    }
    if metrics.width == 0 || metrics.height == 0 {
        return Ok(Coverage::Prepared(vec![]));
    }
    let size = IntSize::from_wh(
        u32::try_from(metrics.width)?,
        u32::try_from(metrics.height)?,
    )
    .ok_or("invalid fallback glyph dimensions")?;
    let mut outline = Outline {
        path: PathBuilder::new(),
        commands: 0,
        invalid: false,
    };
    face.outline_glyph(GlyphId(id), &mut outline)
        .ok_or("fallback glyph outline is unavailable")?;
    if outline.invalid {
        return Err("fallback glyph exceeds outline preparation limits".into());
    }
    let path = outline
        .path
        .finish()
        .ok_or("fallback glyph outline is empty")?;
    let scale = pixels / f32::from(face.units_per_em());
    let transform = Transform::from_row(
        scale,
        0.,
        0.,
        -scale,
        -metrics.xmin as f32,
        metrics.height as f32 + metrics.ymin as f32,
    );
    // The fallible reservation precedes tiny-skia's ownership handoff. The
    // checked metrics/pixel-count contract also bounds all temporary coverage.
    let count = metrics
        .width
        .checked_mul(metrics.height)
        .ok_or("fallback glyph allocation overflow")?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(count)?;
    pixels.resize(count, 0);
    let mut mask = Mask::from_vec(pixels, size).ok_or("invalid fallback glyph coverage")?;
    mask.fill_path(&path, FillRule::Winding, true, transform);
    Ok(Coverage::Outline(mask))
}

#[cfg(test)]
mod tests {
    use super::*;
    const CJK: &[u8] = include_bytes!("../../lucent-ui/assets/NotoSansCJK.ttf");
    #[test]
    fn fallback_metrics_are_checked_before_allocation_and_have_valid_scaled_bearings() {
        let face = face(CJK, 0).unwrap();
        let id = face.glyph_index('語').unwrap().0;
        for size in [16., 32., 64.] {
            let m = metrics(
                RasterFace::Outline {
                    bytes: CJK,
                    index: 0,
                },
                id,
                size,
            )
            .unwrap();
            assert!(m.width > 0 && m.height > 0 && m.advance_width > 0.);
            let pixels = rasterize(
                RasterFace::Outline {
                    bytes: CJK,
                    index: 0,
                },
                id,
                size,
                &m,
            )
            .unwrap();
            assert_eq!(pixels.len(), m.width * m.height);
            assert!(pixels.iter().any(|a| *a > 0));
            assert!(pixels.iter().any(|a| *a > 0 && *a < 255));
        }
        for size in [0., -1., f32::NAN, f32::INFINITY, f32::MAX] {
            assert!(
                metrics(
                    RasterFace::Outline {
                        bytes: CJK,
                        index: 0
                    },
                    id,
                    size
                )
                .is_err()
            );
        }
        assert!(
            metrics(
                RasterFace::Outline {
                    bytes: CJK,
                    index: 0
                },
                u16::MAX,
                32.
            )
            .is_err()
        );
    }
    #[test]
    fn primary_fontdue_geometry_and_coverage_are_unchanged() {
        let font = fontdue::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf").as_slice(),
            Default::default(),
        )
        .unwrap();
        for size in [16., 32., 64.] {
            let id = font.lookup_glyph_index('W');
            let expected = font.rasterize_indexed(id, size);
            let m = metrics(RasterFace::Prepared(&font), id, size).unwrap();
            assert_eq!(m, expected.0);
            assert_eq!(
                &*rasterize(RasterFace::Prepared(&font), id, size, &m).unwrap(),
                expected.1.as_slice()
            );
        }
    }
}
