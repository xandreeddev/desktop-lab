//! Immutable font data and one shared logical shaping model.
//!
//! Unicode bidi resolves visual runs; rustybuzz substitutes and positions glyphs.
//! Layout, caret stops and rasterization consume the same source-indexed result.
use lucent_api::TextSelection;
use std::{
    ops::{Deref, Range},
    sync::{Arc, OnceLock},
};
pub const LINE_HEIGHT: f32 = 1.3;

/// A font keeps the source tables required by the shaper, alongside its rasterizer.
#[derive(Clone, Debug)]
pub struct Font {
    raster: fontdue::Font,
    bytes: Arc<[u8]>,
    index: u32,
}
impl Font {
    pub fn from_bytes<Data: Deref<Target = [u8]>>(
        data: Data,
        settings: fontdue::FontSettings,
    ) -> Result<Self, &'static str> {
        let bytes: Arc<[u8]> = Arc::from(data.deref());
        let raster = fontdue::Font::from_bytes(bytes.clone(), settings)?;
        Ok(Self {
            raster,
            bytes,
            index: settings.collection_index,
        })
    }
    /// Zero is the application's prepared face. Fallbacks borrow their source
    /// tables; selecting one never eagerly prepares its complete glyph catalog.
    pub fn raster_face(&self, id: usize) -> RasterFace<'_> {
        if id == 0 {
            RasterFace::Prepared(&self.raster)
        } else {
            RasterFace::Outline {
                bytes: fallbacks()[id - 1].bytes,
                index: 0,
            }
        }
    }
    fn shaping_face(&self, id: usize) -> rustybuzz::Face<'_> {
        if id == 0 {
            rustybuzz::Face::from_slice(&self.bytes, self.index).expect("validated font tables")
        } else {
            fallbacks()[id - 1].tables().clone()
        }
    }
    fn choose(&self, grapheme: &str) -> usize {
        let supports = |font: &Font| {
            grapheme.chars().all(|c| {
                c.is_control()
                    || matches!(c, '\u{200c}' | '\u{200d}' | '\u{fe0e}' | '\u{fe0f}')
                    || font.has_glyph(c)
            })
        };
        if supports(self) {
            return 0;
        }
        fallbacks()
            .iter()
            .position(|fallback| {
                grapheme.chars().all(|c| {
                    c.is_control()
                        || matches!(c, '\u{200c}' | '\u{200d}' | '\u{fe0e}' | '\u{fe0f}')
                        || fallback.tables().glyph_index(c).is_some()
                })
            })
            .map_or(0, |at| at + 1)
    }
}
impl Deref for Font {
    type Target = fontdue::Font;
    fn deref(&self) -> &Self::Target {
        &self.raster
    }
}
/// Borrowed raster input. The renderer owns outline preparation and coverage;
/// this enum contains no GPU resource or growing per-glyph cache.
#[derive(Clone, Copy)]
pub enum RasterFace<'a> {
    Prepared(&'a fontdue::Font),
    Outline { bytes: &'a [u8], index: u32 },
}
struct Fallback {
    bytes: &'static [u8],
    tables: OnceLock<rustybuzz::Face<'static>>,
}
impl Fallback {
    const fn new(bytes: &'static [u8]) -> Self {
        Self {
            bytes,
            tables: OnceLock::new(),
        }
    }
    fn tables(&self) -> &rustybuzz::Face<'static> {
        self.tables.get_or_init(|| {
            rustybuzz::Face::from_slice(self.bytes, 0).expect("bundled fallback tables")
        })
    }
}
fn fallbacks() -> &'static [Fallback] {
    static FONTS: [Fallback; 5] = [
        Fallback::new(include_bytes!("../assets/NotoSansArabic-Regular.ttf")),
        Fallback::new(include_bytes!("../assets/NotoSansDevanagari-Regular.ttf")),
        Fallback::new(include_bytes!("../assets/NotoSansHebrew-Regular.ttf")),
        Fallback::new(include_bytes!("../assets/NotoSansCJK.ttf")),
        Fallback::new(include_bytes!("../assets/NotoSansHangul.ttf")),
    ];
    &FONTS
}

#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub face: usize,
    pub id: u16,
    pub cluster: Range<usize>,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedLine {
    pub glyphs: Vec<Glyph>,
    /// Stops are in visual left-to-right order, with canonical UTF-8 byte offsets.
    pub stops: Vec<(usize, f32)>,
    pub width: f32,
    pub segments: Vec<(Range<usize>, f32, f32)>,
}
impl ShapedLine {
    pub fn x(&self, byte: usize) -> f32 {
        self.stops
            .iter()
            .find(|(at, _)| *at == byte)
            .map_or(0., |(_, x)| *x)
    }
    /// A logical selection can cover disjoint visual fragments in mixed text.
    pub fn selection(&self, range: Range<usize>) -> Vec<(f32, f32)> {
        let mut fragments: Vec<(f32, f32)> = self
            .segments
            .iter()
            .filter(|(segment, _, _)| segment.start < range.end && segment.end > range.start)
            .map(|(_, a, b)| (a.min(*b), a.max(*b)))
            .collect();
        fragments.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut joined: Vec<(f32, f32)> = Vec::new();
        for (a, b) in fragments {
            if let Some(last) = joined.last_mut().filter(|last| a <= last.1 + 0.01) {
                last.1 = last.1.max(b);
            } else {
                joined.push((a, b));
            }
        }
        joined
    }
}

/// Retained for explicit legacy ASCII metrics; Unicode callers use `shape`.
pub fn glyph_positions<'a>(
    font: &'a Font,
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

pub fn shape(font: &Font, line: &str, size: f32) -> ShapedLine {
    if line.is_ascii() {
        let mut result = ShapedLine::default();
        let boundaries: Vec<_> = TextSelection::boundaries(line).collect();
        for ((byte, c), (_, x)) in line.char_indices().zip(glyph_positions(font, line, size)) {
            let advance = font.metrics(c, size).advance_width;
            result.glyphs.push(Glyph {
                face: 0,
                id: font.lookup_glyph_index(c),
                cluster: byte..byte + c.len_utf8(),
                x,
                y: 0.,
                advance,
            });
            if boundaries.binary_search(&byte).is_ok() {
                result.stops.push((byte, x));
            }
            result.width = x + advance;
        }
        result.stops.push((line.len(), result.width));
        for pair in result.stops.windows(2) {
            result
                .segments
                .push((pair[0].0..pair[1].0, pair[0].1, pair[1].1));
        }
        return result;
    }
    let bidi = unicode_bidi::BidiInfo::new(line, None);
    let mut result = ShapedLine::default();
    for paragraph in &bidi.paragraphs {
        let (levels, runs) = bidi.visual_runs(paragraph, paragraph.range.clone());
        for run in runs {
            let rtl = levels[run.start].is_rtl();
            let boundaries: Vec<_> = TextSelection::boundaries(&line[run.clone()])
                .map(|b| b + run.start)
                .collect();
            let mut spans: Vec<(Range<usize>, usize)> = Vec::new();
            for pair in boundaries.windows(2) {
                let face = font.choose(&line[pair[0]..pair[1]]);
                if let Some((range, _)) = spans.last_mut().filter(|(_, previous)| *previous == face)
                {
                    range.end = pair[1];
                } else {
                    spans.push((pair[0]..pair[1], face));
                }
            }
            if rtl {
                spans.reverse();
            }
            for (range, face_id) in spans {
                let face = font.shaping_face(face_id);
                let scale = size / face.units_per_em() as f32;
                let mut buffer = rustybuzz::UnicodeBuffer::new();
                buffer.push_str(&line[range.clone()]);
                buffer.set_direction(if rtl {
                    rustybuzz::Direction::RightToLeft
                } else {
                    rustybuzz::Direction::LeftToRight
                });
                buffer.guess_segment_properties();
                let output = rustybuzz::shape(&face, &[], buffer);
                let mut clusters: Vec<_> = output
                    .glyph_infos()
                    .iter()
                    .map(|i| range.start + i.cluster as usize)
                    .collect();
                clusters.push(range.end);
                clusters.sort_unstable();
                clusters.dedup();
                let base = result.width;
                let mut x = 0.;
                let mut extents: Vec<(Range<usize>, f32, f32)> = Vec::new();
                for (info, position) in output.glyph_infos().iter().zip(output.glyph_positions()) {
                    let start = range.start + info.cluster as usize;
                    let end = clusters
                        .iter()
                        .copied()
                        .find(|at| *at > start)
                        .unwrap_or(range.end);
                    let advance = position.x_advance as f32 * scale;
                    result.glyphs.push(Glyph {
                        face: face_id,
                        id: info.glyph_id as u16,
                        cluster: start..end,
                        x: base + x + position.x_offset as f32 * scale,
                        y: -position.y_offset as f32 * scale,
                        advance,
                    });
                    if let Some((_, a, b)) = extents
                        .iter_mut()
                        .find(|(range, _, _)| range.start == start)
                    {
                        *a = a.min(x);
                        *b = b.max(x + advance);
                    } else {
                        extents.push((start..end, x, x + advance));
                    }
                    x += advance;
                }
                for (cluster, a, b) in extents {
                    let stops: Vec<_> = TextSelection::boundaries(&line[cluster.clone()])
                        .map(|at| at + cluster.start)
                        .collect();
                    let count = stops.len().saturating_sub(1).max(1) as f32;
                    for pair in stops.windows(2).enumerate() {
                        let (index, pair) = pair;
                        let a_fraction = index as f32 / count;
                        let b_fraction = (index + 1) as f32 / count;
                        result.segments.push((
                            pair[0]..pair[1],
                            base + if rtl {
                                b - (b - a) * a_fraction
                            } else {
                                a + (b - a) * a_fraction
                            },
                            base + if rtl {
                                b - (b - a) * b_fraction
                            } else {
                                a + (b - a) * b_fraction
                            },
                        ));
                    }
                    for (index, byte) in stops.into_iter().enumerate() {
                        let fraction = index as f32 / count;
                        result.stops.push((
                            byte,
                            base + if rtl {
                                b - (b - a) * fraction
                            } else {
                                a + (b - a) * fraction
                            },
                        ));
                    }
                }
                result.width += x;
            }
        }
    }
    result
        .stops
        .sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    result.stops.dedup();
    if result.stops.is_empty() {
        result.stops.push((0, 0.));
    }
    result
}
pub fn width(font: &Font, text: &str, size: f32) -> f32 {
    text.lines()
        .map(|line| {
            if line.is_ascii() {
                glyph_positions(font, line, size)
                    .last()
                    .map_or(0., |(c, x)| x + font.metrics(c, size).advance_width)
            } else {
                shape(font, line, size).width
            }
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
pub fn line_metrics(font: &Font, size: f32) -> LineMetrics {
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

#[cfg(test)]
mod international_tests {
    use super::*;
    fn font() -> Font {
        Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf").as_slice(),
            Default::default(),
        )
        .unwrap()
    }
    #[test]
    fn arabic_shapes_contextual_forms_and_ligatures_with_visual_source_stops() {
        let font = font();
        let value = "سلام";
        let shaped = shape(&font, value, 24.);
        assert!(shaped.glyphs.iter().all(|g| g.id != 0));
        assert!(
            shaped.glyphs.len() < value.chars().count(),
            "lam alef ligature must shape"
        );
        assert!(shaped.stops.windows(2).all(|p| p[0].1 <= p[1].1));
        assert!(shaped.x(0) > shaped.x(value.len()));
        for boundary in TextSelection::boundaries(value) {
            assert!(shaped.stops.iter().any(|(byte, _)| *byte == boundary));
        }
    }
    #[test]
    fn indic_conjunct_and_marks_use_shaped_glyph_positions() {
        let font = font();
        let value = "क्षि";
        let shaped = shape(&font, value, 24.);
        assert!(shaped.glyphs.iter().all(|g| g.id != 0));
        assert!(shaped.glyphs.iter().any(|g| g.cluster.len() > 3));
        assert_eq!(
            shaped.stops.iter().map(|(at, _)| *at).collect::<Vec<_>>(),
            TextSelection::boundaries(value).collect::<Vec<_>>()
        );
        assert!(shaped.width > 0.);
    }
    #[test]
    fn mixed_bidi_selection_is_disjoint_and_stops_cover_graphemes() {
        let font = font();
        let value = "abc אבג def";
        let shaped = shape(&font, value, 20.);
        for boundary in TextSelection::boundaries(value) {
            assert!(shaped.stops.iter().any(|(byte, _)| *byte == boundary));
        }
        let hebrew = value.find('א').unwrap();
        assert!(shaped.x(hebrew) > shaped.x(hebrew + "אב".len()));
        assert_eq!(shaped.selection(0..hebrew + "א".len()).len(), 2);
        assert!(shaped.glyphs.iter().all(|g| g.id != 0));
    }
    #[test]
    fn cjk_fallback_covers_full_common_ideographs_and_kana() {
        let font = font();
        let value = "日本語 中文 あいうえお アイウエオ 龍 龥 한국어 한글";
        let shaped = shape(&font, value, 20.);
        assert!(shaped.glyphs.iter().all(|glyph| glyph.id != 0));
        assert!(shaped.glyphs.iter().any(|glyph| glyph.face == 4));
        assert!(shaped.width > 0.);
    }
    #[test]
    fn ascii_keeps_exact_legacy_pair_positions_and_advance() {
        let font = font();
        let value = "AV ffi, Lucent 123";
        let shaped = shape(&font, value, 16.);
        for (glyph, (_, x)) in shaped.glyphs.iter().zip(glyph_positions(&font, value, 16.)) {
            assert_eq!(glyph.x, x);
        }
        assert_eq!(shaped.glyphs.len(), value.len());
    }
}
