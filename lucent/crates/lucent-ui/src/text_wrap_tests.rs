//! Expected visual orders are stated explicitly, independently of the resolver.
use super::*;

fn font() -> text::Font {
    text::Font::from_bytes(
        include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf").as_slice(),
        Default::default(),
    )
    .unwrap()
}

fn fragment(font: &text::Font, source: &str, body: &str) -> Row {
    let boundaries = TextSelection::boundaries(source).collect::<Vec<_>>();
    let projection = Projection::new(source, &boundaries);
    let bidi = projection.bidi(source);
    let start = source.find(body).unwrap();
    measured_row(
        font,
        &projection,
        Some(&bidi),
        start..start + body.len(),
        20.,
    )
}

fn shaped(font: &text::Font, row: &Row) -> text::ShapedLine {
    text::shape_with_layout(
        font,
        &row.display,
        20.,
        text::LayoutOptions {
            bidi: row.bidi.clone(),
            ..Default::default()
        },
    )
}

#[test]
fn soft_line_keeps_latin_hard_paragraph_before_hebrew_first_wrap() {
    let font = font();
    let source = "abc אבג xyz";
    let row = fragment(&font, source, "אבג xyz");
    let actual = shaped(&font, &row);
    assert_eq!(
        actual
            .glyphs
            .iter()
            .map(|g| g.cluster.start)
            .collect::<Vec<_>>(),
        [4, 2, 0, 6, 7, 8, 9],
        "visual order is Hebrew reversed, then space/xyz; the hard paragraph is LTR"
    );
    assert_ne!(
        actual.glyphs,
        text::shape(&font, &row.display, 20.).glyphs,
        "inferring a fresh RTL paragraph at the wrap must fail this fixture"
    );
}

#[test]
fn override_and_isolate_context_survive_a_soft_line_start() {
    let font = font();
    for (source, body, expected) in [
        (
            "abc \u{202e}abc DEF\u{202c}",
            "abc DEF",
            vec![6, 5, 4, 3, 2, 1, 0],
        ),
        (
            "abc \u{2067}xyz אבג\u{2069} tail",
            "xyz אבג",
            vec![8, 6, 4, 3, 0, 1, 2],
        ),
    ] {
        let row = fragment(&font, source, body);
        let actual = shaped(&font, &row);
        assert_eq!(
            actual
                .glyphs
                .iter()
                .map(|g| g.cluster.start)
                .collect::<Vec<_>>(),
            expected
        );
        assert_ne!(
            actual.glyphs,
            text::shape_directed(&font, body, 20., Some(text::Direction::LeftToRight)).glyphs,
            "inheriting only the hard paragraph's base direction loses override/isolate levels"
        );
    }
}

#[derive(Clone)]
enum Message {
    Edit(TextEdit),
    Scroll,
}

#[test]
fn actual_wrapping_paint_hits_selection_and_candidate_share_resolved_order() {
    let font = font();
    let width = text::width(&font, &format!("{} ", "i".repeat(24)), 20.) + 0.25;
    let layout = Layout::new(Arc::new(vec![font]));
    for (opening, body, closing, expected_letters) in [
        ("", "אבג xyz", "", vec![4, 2, 0, 7, 8, 9]),
        ("\u{202e}", "abc DEF", "\u{202c}", vec![6, 5, 4, 2, 1, 0]),
        ("\u{2067}", "xyz אבג", "\u{2069}", vec![8, 6, 4, 0, 1, 2]),
    ] {
        let mut value = format!("{}{opening} {body}{closing} tail", "i".repeat(24));
        let start = value.find(body).unwrap();
        let mut field = TextFieldState::multiline();
        field
            .update(
                &mut value,
                TextEdit::Select {
                    anchor: start,
                    caret: start + body.len(),
                },
                8192,
                ClipboardTarget::new("test", "wrapped"),
                &mut Effects::<Message>::default(),
                Message::Edit,
            )
            .unwrap();
        let node = Element::text_area(
            &value,
            field.selection(),
            "",
            ScrollSpec::new(0., 24.).unwrap(),
            Message::Edit,
            |_| Message::Scroll,
        )
        .id("wrapped")
        .autofocus()
        .size(width, 400.)
        .font(20.)
        .scroll_style(ScrollStyle {
            gutter: 0.,
            width: 0.,
            ..Default::default()
        });
        let mut input = Interaction::default();
        let scene = layout.build(&node, width, 400., &input, 0.);
        input.synchronize(&scene);
        let hit = scene.hits.iter().find(|hit| hit.id == "wrapped").unwrap();
        let text_hit = hit.text.as_ref().unwrap();
        let area = text_hit.area.as_ref().unwrap();
        let (index, row) = area
            .rows
            .iter()
            .enumerate()
            .find(|(_, row)| row.range.start == start && row.range.end >= start + body.len())
            .unwrap_or_else(|| panic!("fixture {opening:?} {body:?}; width={width}; inner={}; start={start}; rows={:?}", text_hit.inner.w, area.rows.iter().map(|row| (row.range.clone(),row.display.as_ref())).collect::<Vec<_>>()));
        let paint_layout = scene
            .paint
            .iter()
            .find_map(|paint| match paint {
                Paint::Text { text, layout, .. } if text == row.display.as_ref() => {
                    Some(layout.clone())
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(paint_layout.bidi, row.bidi);
        let painted = text::shape_with_layout(&layout.fonts[0], &row.display, 20., paint_layout);
        let actual_letters = painted
            .glyphs
            .iter()
            .filter(|glyph| {
                glyph.cluster.start < body.len()
                    && body[glyph.cluster.start..]
                        .chars()
                        .next()
                        .is_some_and(char::is_alphabetic)
            })
            .map(|glyph| glyph.cluster.start)
            .collect::<Vec<_>>();
        assert_eq!(actual_letters, expected_letters);
        for (source, x) in row.stops.iter() {
            assert!(
                painted
                    .stops
                    .iter()
                    .any(|(byte, px)| row.range.start + byte == *source && px == x),
                "paint and canonical caret stops differ"
            );
        }
        let expected_selection =
            text::selection_fragments(&row.segments, start..start + body.len());
        assert_eq!(expected_selection, painted.selection(0..body.len()));
        for (range, a, b) in row.segments.iter() {
            if range.start >= start && range.end <= start + body.len() && (b - a).abs() > 0.01 {
                let x = text_hit.inner.x + (a + b) / 2.;
                let y = text_hit.inner.y + index as f32 * area.line_height + area.line_height / 2.;
                let byte = text_hit.at_point(x, y).unwrap();
                let distance = row
                    .stops
                    .iter()
                    .map(|(_, stop)| (stop - (a + b) / 2.).abs())
                    .fold(f32::INFINITY, f32::min);
                assert!(
                    row.stops.iter().any(|(source, stop)| *source == byte
                        && ((stop - (a + b) / 2.).abs() - distance).abs() < 0.001),
                    "hit must use a nearest shared visual stop, including zero-width format-control affinities"
                );
            }
        }
        let candidate = text_hit.caret_rect();
        assert!(
            scene
                .paint
                .iter()
                .any(|paint| matches!(paint, Paint::Shape {rect,..} if *rect == candidate)),
            "IME candidate and painted caret differ"
        );
        let messages = input.key(&scene, &Key::Left);
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, Message::Edit(TextEdit::Select { .. }))),
            "visual navigation still emits canonical relative selections"
        );
    }
}

#[test]
fn tab_projection_preserves_segment_separator_and_canonical_byte_span() {
    let font = font();
    let source = "abc אבג\tדהו xyz";
    let row = fragment(&font, source, source);
    assert_eq!(row.display.as_ref(), "abc אבג    דהו xyz");
    assert_eq!(
        shaped(&font, &row)
            .glyphs
            .iter()
            .map(|g| g.cluster.start)
            .collect::<Vec<_>>(),
        [
            0, 1, 2, 3, 8, 6, 4, 10, 11, 12, 13, 18, 16, 14, 20, 21, 22, 23
        ]
    );
    let tab = source.find('\t').unwrap();
    assert!(row.stops.iter().any(|(byte, _)| *byte == tab));
    assert!(row.stops.iter().any(|(byte, _)| *byte == tab + 1));
    let (range, a, b) = row
        .segments
        .iter()
        .find(|(range, _, _)| range.start == tab)
        .unwrap();
    assert_eq!(range.clone(), tab..tab + 1);
    assert!((b - a).abs() > font.metrics(' ', 20.).advance_width * 3.9);
}

#[test]
fn projected_paragraph_boundaries_include_crlf_interior_offsets() {
    let source = "a\tb\r\nאבג xyz";
    let boundaries = TextSelection::boundaries(source).collect::<Vec<_>>();
    let projection = Projection::new(source, &boundaries);
    let original = unicode_bidi::BidiInfo::new(source, None);
    let projected = projection.bidi(source);
    assert_eq!(projected.text, "a    b\r\nאבג xyz");
    for (before, after) in original.paragraphs.iter().zip(&projected.paragraphs) {
        let start = source[..before.range.start].replace('\t', "    ").len();
        let end = source[..before.range.end].replace('\t', "    ").len();
        assert_eq!(after.range, start..end);
        assert_eq!(after.level, before.level);
    }
    assert_eq!(
        projected.original_classes[1..5],
        [unicode_bidi::BidiClass::S; 4]
    );
}

#[test]
fn stale_same_length_resolved_context_cannot_change_different_text() {
    let font = font();
    let row = fragment(&font, "abc \u{202e}abc DEF\u{202c}", "abc DEF");
    let replacement = "uvw XYZ";
    assert_eq!(replacement.len(), row.display.len());
    let actual = text::shape_with_layout(
        &font,
        replacement,
        20.,
        text::LayoutOptions {
            bidi: row.bidi,
            ..Default::default()
        },
    );
    assert_eq!(actual, text::shape(&font, replacement, 20.));
}
