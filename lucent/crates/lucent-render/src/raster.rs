//! Cached CPU raster work. The presentation framebuffer stays at output density.
use lucent_ui::text::{glyph_positions, line_metrics, width};

/// Area reduction on premultiplied RGBA, including odd-sized edge texels.
/// Used for texture mip levels and 2× text coverage supersampling.
pub fn reduce(bytes: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (x0, x1) = (x * width / w, (x + 1) * width / w);
            let (y0, y1) = (y * height / h, (y + 1) * height / h);
            let samples = (x1 - x0) * (y1 - y0);
            let mut channels = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = ((sy * width + sx) * 4) as usize;
                    for c in 0..4 {
                        channels[c] += u32::from(bytes[at + c]);
                    }
                }
            }
            let at = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                out[at + c] = ((channels[c] + samples / 2) / samples) as u8;
            }
        }
    }
    (out, w, h)
}

/// Rasterize coverage at twice the physical pixel density, then area-resolve once.
/// Glyph advances are computed in logical coordinates, so scale does not reflow text.
pub fn text(font: &fontdue::Font, text: &str, size: f32, scale: u32) -> (Vec<u8>, u32, u32) {
    let scale = scale.max(1);
    let density = scale as f32 * 2.;
    let logical_w = (width(font, text, size).ceil() + 2.).clamp(1., 4096.);
    let metrics = line_metrics(font, size);
    let logical_h = (metrics.height * text.lines().count().max(1) as f32)
        .ceil()
        .clamp(1., 1024.);
    // Bound allocation for pathological strings/scale; normal UI text is well below this.
    let w = ((logical_w as u32 * scale).min(4096) * 2).max(2);
    let h = ((logical_h as u32 * scale).min(1024) * 2).max(2);
    let mut pixels = vec![0; (w * h * 4) as usize];
    for (line_index, line) in text.lines().enumerate() {
        for (c, x) in glyph_positions(font, line, size) {
            if x * density >= w as f32 {
                break;
            }
            let (glyph, bitmap) = font.rasterize(c, size * density);
            let left = (x * density).round() as i32 + glyph.xmin;
            let top = ((line_index as f32 * metrics.height + metrics.baseline) * density).round()
                as i32
                - glyph.height as i32
                - glyph.ymin;
            for row in 0..glyph.height {
                for col in 0..glyph.width {
                    let (px, py) = (left + col as i32, top + row as i32);
                    if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 {
                        continue;
                    }
                    let a = bitmap[row * glyph.width + col];
                    let at = ((py as u32 * w + px as u32) * 4) as usize;
                    for channel in &mut pixels[at..at + 4] {
                        *channel = a.saturating_add(
                            (u16::from(*channel) * (255 - u16::from(a)) / 255) as u8,
                        );
                    }
                }
            }
        }
    }
    reduce(&pixels, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mip_reduction_preserves_alpha_and_odd_edges() {
        let pixels = [255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0];
        let (mip, w, h) = reduce(&pixels, 3, 1);
        assert_eq!((w, h), (1, 1));
        assert_eq!(mip, [85, 0, 0, 85]);
    }
    #[test]
    fn density_changes_resolution_without_reflow_or_losing_coverage() {
        let font = fontdue::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .unwrap();
        let (a, w, h) = text(&font, "AV — Lucent", 14., 1);
        let (b, w2, h2) = text(&font, "AV — Lucent", 14., 2);
        assert_eq!((w2, h2), (w * 2, h * 2));
        let coverage = |pixels: &[u8]| {
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| f64::from(p[3]) / 255.)
                .sum::<f64>()
        };
        let ratio = coverage(&b) / (coverage(&a) * 4.);
        assert!((0.9..1.1).contains(&ratio), "coverage ratio {ratio}");
        assert!(a.as_chunks::<4>().0.iter().any(|p| p[3] > 0 && p[3] < 255));
        assert!(
            b.as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[0] == p[3] && p[1] == p[3] && p[2] == p[3])
        );
    }
}
