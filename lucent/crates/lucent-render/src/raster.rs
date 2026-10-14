//! Cached CPU raster work. The presentation framebuffer stays at output density.
use crate::Error;
use lucent_ui::text::{glyph_positions, line_metrics, width};

// Existing physical text-mask limits, before the 2× coverage supersample.
const MAX_WIDTH: u32 = 4096;
const MAX_HEIGHT: u32 = 1024;
const SUPERSAMPLE: u32 = 2;
const MAX_GLYPH_DIMENSION: usize = (MAX_WIDTH * SUPERSAMPLE) as usize;
const MAX_GLYPH_PIXELS: usize = (MAX_WIDTH * MAX_HEIGHT * SUPERSAMPLE * SUPERSAMPLE) as usize;
const MAX_TEXT_BYTES: usize = 64 * 1024;

pub(super) fn validate_request(text: &str, size: f32, scale: f32) -> Result<(), Error> {
    let pixels = size * scale * SUPERSAMPLE as f32;
    if !scale.is_normal()
        || scale <= 0.
        || !size.is_normal()
        || size <= 0.
        || !pixels.is_normal()
        || pixels > MAX_GLYPH_DIMENSION as f32
    {
        return Err("text size or scale exceeds the raster envelope".into());
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err("text primitive exceeds the 64 KiB preparation limit".into());
    }
    Ok(())
}

fn validate_glyph(glyph: &fontdue::Metrics) -> Result<(), Error> {
    if glyph.width > MAX_GLYPH_DIMENSION
        || glyph.height > MAX_GLYPH_DIMENSION
        || glyph
            .width
            .checked_mul(glyph.height)
            .is_none_or(|n| n > MAX_GLYPH_PIXELS)
        || !glyph.advance_width.is_finite()
        || !glyph.advance_height.is_finite()
    {
        return Err("glyph bitmap exceeds the raster envelope".into());
    }
    Ok(())
}

/// Rasterize coverage at twice the physical pixel density, then area-resolve once.
/// Glyph advances are computed in logical coordinates, so scale does not reflow text.
/// Returns one alpha byte per pixel; the renderer supplies premultiplied white.
pub fn text(
    font: &fontdue::Font,
    text: &str,
    size: f32,
    scale: f32,
    texture_limit: u32,
) -> Result<(Vec<u8>, u32, u32, f32), Error> {
    validate_request(text, size, scale)?;
    let density = scale * SUPERSAMPLE as f32;
    if font.horizontal_line_metrics(size).is_none() {
        return Err("font has no horizontal line metrics".into());
    }
    let measured_width = width(font, text, size);
    let metrics = line_metrics(font, size);
    if !measured_width.is_finite()
        || !metrics.height.is_finite()
        || metrics.height <= 0.
        || !metrics.baseline.is_finite()
    {
        return Err("invalid text layout metrics".into());
    }
    // A glyph's ink can overhang its advance, especially in italic faces.
    // Keep layout advances unchanged and return the mask origin separately.
    let mut left_edge = 0f32;
    let mut right_edge = measured_width * density;
    for line in text.lines() {
        for (c, x) in glyph_positions(font, line, size) {
            if x * density >= MAX_WIDTH as f32 * density {
                break;
            }
            let glyph = font.metrics(c, size * density);
            validate_glyph(&glyph)?;
            let left = (x * density).round() + glyph.xmin as f32;
            left_edge = left_edge.min(left);
            right_edge = right_edge.max(left + glyph.width as f32);
        }
    }
    let left_padding = (-left_edge / SUPERSAMPLE as f32).ceil() * SUPERSAMPLE as f32;
    let logical_w = ((right_edge + left_padding) / density)
        .ceil()
        .max(measured_width.ceil())
        + 2.;
    let logical_w = logical_w.clamp(1., MAX_WIDTH as f32);
    let logical_h = (metrics.height * text.lines().count().max(1) as f32)
        .ceil()
        .clamp(1., MAX_HEIGHT as f32);
    let w = ((logical_w as f64 * f64::from(scale))
        .ceil()
        .min(f64::from(MAX_WIDTH)) as u32
        * SUPERSAMPLE)
        .max(2);
    let h = ((logical_h as f64 * f64::from(scale))
        .ceil()
        .min(f64::from(MAX_HEIGHT)) as u32
        * SUPERSAMPLE)
        .max(2);
    if w / SUPERSAMPLE > texture_limit || h / SUPERSAMPLE > texture_limit {
        return Err("text mask exceeds device texture dimension limit".into());
    }
    // Inspect every bitmap that will be rasterized before allocating coverage.
    // fontdue's bitmap allocation is infallible, so bound its dimensions first.
    let mut glyphs = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        for (c, x) in glyph_positions(font, line, size) {
            if !x.is_finite() {
                return Err("invalid glyph position".into());
            }
            if x * density >= w as f32 {
                break;
            }
            let glyph = font.metrics(c, size * density);
            validate_glyph(&glyph)?;
            let left = f64::from((x * density).round() + left_padding) + f64::from(glyph.xmin);
            let top = f64::from(
                ((line_index as f32 * metrics.height + metrics.baseline) * density).round(),
            ) - glyph.height as f64
                - f64::from(glyph.ymin);
            if !left.is_finite() || !top.is_finite() {
                return Err("invalid glyph position".into());
            }
            if glyph.width == 0
                || glyph.height == 0
                || left >= f64::from(w)
                || top >= f64::from(h)
                || left + glyph.width as f64 <= 0.
                || top + glyph.height as f64 <= 0.
            {
                continue;
            }
            // Intersection plus bounded glyph dimensions makes these casts safe.
            glyphs.try_reserve(1)?;
            glyphs.push((c, glyph, left as i64, top as i64));
        }
    }
    let mut pixels = Vec::new();
    pixels.try_reserve_exact((w * h) as usize)?;
    pixels.resize((w * h) as usize, 0);
    for (c, glyph, left, top) in glyphs {
        let (_, bitmap) = font.rasterize(c, size * density);
        let x0 = left.max(0);
        let y0 = top.max(0);
        let x1 = (left + glyph.width as i64).min(i64::from(w));
        let y1 = (top + glyph.height as i64).min(i64::from(h));
        for py in y0..y1 {
            for px in x0..x1 {
                let a = bitmap[(py - top) as usize * glyph.width + (px - left) as usize];
                let at = (py as u32 * w + px as u32) as usize;
                pixels[at] =
                    a.saturating_add((u16::from(pixels[at]) * (255 - u16::from(a)) / 255) as u8);
            }
        }
    }
    let (pixels, width, height) = lucent_api::reduce_alpha(&pixels, w, h)?;
    Ok((pixels, width, height, -left_padding / density))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fractional_text_keeps_logical_advance_and_bounds_coverage_allocations() {
        let font = fontdue::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
            Default::default(),
        )
        .unwrap();
        let logical = width(&font, "Lucent", 16.);
        for scale in [1.25, 1.5, 1.75, 2.25] {
            let (mask, w, h, origin) = text(&font, "Lucent", 16., scale, 8192).unwrap();
            assert_eq!(mask.len(), (w * h) as usize);
            assert!(mask.iter().any(|&coverage| coverage > 0));
            assert!(w as f32 / scale + origin >= logical);
            assert!(w <= MAX_WIDTH && h <= MAX_HEIGHT);
        }
        for scale in [0., -1., f32::NAN, f32::INFINITY, f32::MAX] {
            assert!(text(&font, "Lucent", 16., scale, 8192).is_err());
        }
    }
    #[test]
    fn italic_ink_overhang_is_inside_the_mask_without_changing_advance() {
        let font = fontdue::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentItalic.ttf") as &[u8],
            Default::default(),
        )
        .unwrap();
        for scale in [1, 2] {
            let value = "fj";
            let size = 32.;
            let (pixels, w, h, origin) = text(&font, value, size, scale as f32, 8192).unwrap();
            let density = scale as f32 * SUPERSAMPLE as f32;
            let right = glyph_positions(&font, value, size)
                .map(|(c, x)| {
                    let glyph = font.metrics(c, size * density);
                    (x * density).round() + glyph.xmin as f32 + glyph.width as f32
                })
                .fold(0., f32::max)
                / density;
            assert!(w as f32 / scale as f32 + origin > right);
            assert!(origin <= 0.);
            assert_eq!(origin * scale as f32, (origin * scale as f32).round());
            assert!(pixels.iter().any(|&coverage| coverage > 0));
            assert_eq!(pixels.len(), (w * h) as usize);
        }
    }
    #[test]
    fn mip_reduction_preserves_alpha_and_odd_edges() {
        let pixels = [255, 0, 0];
        let (mip, w, h) = lucent_api::reduce_alpha(&pixels, 3, 1).unwrap();
        assert_eq!((w, h), (1, 1));
        assert_eq!(mip, [85]);
    }
    #[test]
    fn density_changes_resolution_without_reflow_or_losing_coverage() {
        let font = fontdue::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .unwrap();
        let (a, w, h, x) = text(&font, "AV — Lucent", 14., 1., 8192).unwrap();
        let (b, w2, h2, x2) = text(&font, "AV — Lucent", 14., 2., 8192).unwrap();
        assert_eq!((w2, h2), (w * 2, h * 2));
        assert_eq!(x, x2);
        let coverage = |pixels: &[u8]| pixels.iter().map(|p| f64::from(*p) / 255.).sum::<f64>();
        let ratio = coverage(&b) / (coverage(&a) * 4.);
        assert!((0.9..1.1).contains(&ratio), "coverage ratio {ratio}");
        assert_eq!(a.len(), (w * h) as usize);
        assert_eq!(b.len(), (w2 * h2) as usize);
        assert!(a.iter().any(|a| *a > 0 && *a < 255));
    }

    #[test]
    fn invalid_text_requests_and_glyphs_fail_before_bitmap_allocation() {
        let font = fontdue::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .unwrap();
        for size in [0., -1., f32::NAN, f32::INFINITY, f32::MAX] {
            assert!(text(&font, "W", size, 1., 8192).is_err());
        }
        assert!(text(&font, "W", 14., 0., 8192).is_err());
        assert!(text(&font, "W", 14., u32::MAX as f32, 8192).is_err());
        assert!(text(&font, "W", 4096., 1., 8192).is_err());
        assert!(text(&font, &"a".repeat(MAX_TEXT_BYTES + 1), 14., 1., 8192).is_err());
        assert!(text(&font, "width", 14., 1., 1).is_err());
        assert!(text(&font, "still usable", 14., 1., 8192).is_ok());
    }
}
