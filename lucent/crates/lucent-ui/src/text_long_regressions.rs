//! A deliberately slow reference preserves the old cluster-search behavior.
//! It consumes independent raw rustybuzz output, so a faster collector must keep
//! glyph positions, source stops, selection coverage and ink metrics identical.
use super::*;

fn reference(font: &Font, value: &str, script: rustybuzz::Script, rtl: bool) -> ShapedLine {
    let bounds = TextSelection::boundaries(value).collect::<Vec<_>>();
    let face_id = font.choose(&value[bounds[0]..bounds[1]]);
    for pair in bounds.windows(2) {
        assert_eq!(font.choose(&value[pair[0]..pair[1]]), face_id);
    }
    let face = font.shaping_face(face_id);
    let scale = 24. / face.units_per_em() as f32;
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(value);
    buffer.set_script(script);
    buffer.set_direction(if rtl {
        rustybuzz::Direction::RightToLeft
    } else {
        rustybuzz::Direction::LeftToRight
    });
    buffer.guess_segment_properties();
    let output = rustybuzz::shape(&face, &[], buffer);
    let mut ends = output
        .glyph_infos()
        .iter()
        .map(|info| info.cluster as usize)
        .collect::<Vec<_>>();
    ends.push(value.len());
    ends.sort_unstable();
    ends.dedup();
    let mut result = ShapedLine::default();
    let mut extents: Vec<(Range<usize>, f32, f32)> = vec![];
    for (info, position) in output.glyph_infos().iter().zip(output.glyph_positions()) {
        let start = info.cluster as usize;
        let end = ends.iter().copied().find(|end| *end > start).unwrap();
        let advance = position.x_advance as f32 * scale;
        result.glyphs.push(Glyph {
            face: face_id,
            id: info.glyph_id as u16,
            cluster: start..end,
            x: result.width + position.x_offset as f32 * scale,
            y: -position.y_offset as f32 * scale,
            advance,
        });
        if let Some((_, a, b)) = extents
            .iter_mut()
            .find(|(range, _, _)| range.start == start)
        {
            *a = a.min(result.width);
            *b = b.max(result.width + advance);
        } else {
            extents.push((start..end, result.width, result.width + advance));
        }
        result.width += advance;
    }
    for (range, a, b) in extents {
        let boundaries = TextSelection::boundaries(&value[range.clone()])
            .map(|byte| range.start + byte)
            .collect::<Vec<_>>();
        let count = boundaries.len().saturating_sub(1).max(1) as f32;
        let at = |index: usize| {
            let fraction = index as f32 / count;
            if rtl {
                b - (b - a) * fraction
            } else {
                a + (b - a) * fraction
            }
        };
        for (index, pair) in boundaries.windows(2).enumerate() {
            result
                .segments
                .push((pair[0]..pair[1], at(index), at(index + 1)));
        }
        result.stops.extend(
            boundaries
                .into_iter()
                .enumerate()
                .map(|(i, byte)| (byte, at(i))),
        );
    }
    result
        .stops
        .sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    result.stops.dedup();
    result
}

#[test]
fn long_monotone_runs_keep_exact_glyph_cluster_caret_selection_and_ink_metrics() {
    let font = Font::from_bytes(
        include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf").as_slice(),
        Default::default(),
    )
    .unwrap();
    for (unit, script, rtl) in [
        ("日本語龍", rustybuzz::script::HAN, false),
        ("cafe\u{301}", rustybuzz::script::LATIN, false),
        ("मस्तेक्षि", rustybuzz::script::DEVANAGARI, false),
        ("مَرْحَبًا", rustybuzz::script::ARABIC, true),
        ("שָׁלוֹם", rustybuzz::script::HEBREW, true),
    ] {
        let value = unit.repeat(96);
        let expected = reference(&font, &value, script, rtl);
        let direction = Some(if rtl {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        });
        let actual = shape_directed(&font, &value, 24., direction);
        assert_eq!(actual.width, expected.width);
        assert_eq!(actual.glyphs.len(), expected.glyphs.len());
        for (index, (observed, original)) in actual.glyphs.iter().zip(&expected.glyphs).enumerate()
        {
            assert_eq!(observed, original, "{script:?} glyph {index}");
        }
        assert_eq!(actual.stops.len(), expected.stops.len());
        for (index, (observed, original)) in actual.stops.iter().zip(&expected.stops).enumerate() {
            assert_eq!(observed, original, "{script:?} visual stop {index}");
        }
        assert_eq!(actual.segments.len(), expected.segments.len());
        for (index, (observed, original)) in
            actual.segments.iter().zip(&expected.segments).enumerate()
        {
            assert_eq!(observed, original, "{script:?} selection segment {index}");
        }
        let base = line_metrics(&font, 24.);
        let (mut ascent, mut descent) = (base.ascent, base.descent);
        for glyph in &expected.glyphs {
            let face = font.shaping_face(glyph.face);
            if let Some(bounds) = face.glyph_bounding_box(rustybuzz::ttf_parser::GlyphId(glyph.id))
            {
                let scale = 24. / face.units_per_em() as f32;
                ascent = ascent.max(f32::from(bounds.y_max) * scale - glyph.y);
                descent = descent.min(f32::from(bounds.y_min) * scale - glyph.y);
            }
        }
        let height = base.height.max((ascent - descent).ceil());
        assert_eq!(
            run_metrics(&font, &value, 24., direction),
            LineMetrics {
                height,
                baseline: (height - ascent + descent) / 2. + ascent,
                ascent,
                descent,
            }
        );
        for boundary in TextSelection::boundaries(&value) {
            assert!(actual.stops.iter().any(|(byte, _)| *byte == boundary));
        }
    }
}
