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
