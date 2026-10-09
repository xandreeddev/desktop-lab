//! Shared logical text advances for layout, caret placement and rasterization.
//! Pair kerning is supported; complex-script shaping and IME remain separate work.
pub const LINE_HEIGHT: f32 = 1.3;

pub fn glyph_positions<'a>(
    font: &'a fontdue::Font,
    line: &'a str,
    size: f32,
) -> impl Iterator<Item = (char, f32)> + 'a {
    let mut previous = None;
    let mut x = 0.;
    line.chars().map(move |c| {
        if let Some(p) = previous {
            x += font.horizontal_kern(p, c, size).unwrap_or(0.);
        }
        let at = x;
        x += font.metrics(c, size).advance_width;
        previous = Some(c);
        (c, at)
    })
}

pub fn width(font: &fontdue::Font, text: &str, size: f32) -> f32 {
    text.lines()
        .map(|line| {
            glyph_positions(font, line, size)
                .last()
                .map(|(c, x)| x + font.metrics(c, size).advance_width)
                .unwrap_or(0.)
        })
        .fold(0., f32::max)
}

#[derive(Clone, Copy, Debug)]
pub struct LineMetrics {
    pub height: f32,
    pub baseline: f32,
    pub ascent: f32,
    pub descent: f32,
}
/// Shared baseline for glyph rasterization, vertically centered fields and carets.
pub fn line_metrics(font: &fontdue::Font, size: f32) -> LineMetrics {
    let metrics = font
        .horizontal_line_metrics(size)
        .expect("horizontal font metrics");
    let height = (size * LINE_HEIGHT)
        .max(metrics.ascent - metrics.descent)
        .ceil();
    LineMetrics {
        height,
        baseline: (height - metrics.ascent + metrics.descent) / 2. + metrics.ascent,
        ascent: metrics.ascent,
        descent: metrics.descent,
    }
}
