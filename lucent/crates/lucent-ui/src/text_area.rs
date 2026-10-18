//! Two-dimensional ordinary text geometry. One row map owns source offsets,
//! wrapping, pointer placement, caret painting and visual navigation.
use super::*;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub range: Range<usize>,
    pub stops: Arc<[(usize, f32)]>,
    display: String,
}
impl Row {
    fn at(&self, x: f32) -> usize {
        self.stops
            .iter()
            .min_by(|(_, a), (_, b)| (a - x).abs().total_cmp(&(b - x).abs()))
            .map_or(self.range.start, |(at, _)| *at)
    }
    fn visible_display(&self, font: &fontdue::Font, size: f32, width: f32) -> String {
        if self.stops.last().is_none_or(|(_, end)| *end <= width) {
            return self.display.clone();
        }
        // A whole grapheme may be wider than its viewport. Clipping geometry
        // alone must not rasterize a huge hidden texture for that row. Keep the
        // exact prefix through the last glyph whose actual ink intersects the
        // visible horizontal span; source/selection geometry stays untouched.
        let end = self
            .display
            .char_indices()
            .zip(text::glyph_positions(font, &self.display, size))
            .filter_map(|((byte, c), (_, x))| {
                let metrics = font.metrics(c, size);
                let left = x + metrics.xmin as f32;
                (left < width && left + metrics.width as f32 > 0.).then_some(byte + c.len_utf8())
            })
            .last()
            .unwrap_or(0);
        self.display[..end].to_owned()
    }
    fn x(&self, byte: usize) -> f32 {
        self.stops
            .iter()
            .find(|(at, _)| *at == byte)
            .map_or(0., |(_, x)| *x)
    }
}
#[derive(Clone)]
pub(super) struct AreaGeometry {
    pub rows: Arc<[Row]>,
    pub line_height: f32,
    pub caret_row: usize,
    pub content_height: f32,
}
/// Transient visual affinity and vertical goal; never canonical text state.
#[derive(Clone, PartialEq)]
pub(super) struct Navigation {
    id: String,
    value: Arc<str>,
    anchor: usize,
    caret: usize,
    row: usize,
    goal_x: Option<f32>,
    width: f32,
    height: f32,
    line_height: f32,
    rows: Arc<[Row]>,
}
impl Navigation {
    pub fn compatible<M>(&self, hit: &Hit<M>) -> bool {
        hit.id == self.id
            && hit.text.as_ref().is_some_and(|text| {
                text.value == self.value
                    && text.selection.anchor() == self.anchor
                    && text.selection.caret() == self.caret
                    && text.inner.w == self.width
                    && text.inner.h == self.height
                    && text.area.as_ref().is_some_and(|area| {
                        area.line_height == self.line_height && area.rows == self.rows
                    })
            })
    }
    pub fn affinity(
        &self,
        id: &str,
        value: &str,
        selection: TextSelection,
        inner: Rect,
        line_height: f32,
        rows: &[Row],
    ) -> Option<usize> {
        (self.id == id
            && self.value.as_ref() == value
            && self.anchor == selection.anchor()
            && self.caret == selection.caret()
            && self.width == inner.w
            && self.height == inner.h
            && self.line_height == line_height
            && self.rows.as_ref() == rows)
            .then_some(self.row)
    }
}
fn display_grapheme(grapheme: &str) -> std::borrow::Cow<'_, str> {
    if grapheme == "\t" {
        std::borrow::Cow::Borrowed("    ")
    } else {
        std::borrow::Cow::Borrowed(grapheme)
    }
}
fn measured_row(font: &fontdue::Font, value: &str, range: Range<usize>, size: f32) -> Row {
    let mut display = String::new();
    let mut stops = Vec::new();
    let mut x = 0.;
    let mut previous = None;
    let boundaries: Vec<_> = TextSelection::boundaries(&value[range.clone()]).collect();
    for pair in boundaries.windows(2) {
        let at = range.start + pair[0];
        let text = display_grapheme(&value[at..range.start + pair[1]]);
        // The first glyph's kerning belongs to this grapheme boundary, matching
        // the same pair positioning used by the rasterizer for this row.
        if let (Some(p), Some(c)) = (previous, text.chars().next()) {
            x += font.horizontal_kern(p, c, size).unwrap_or(0.);
        }
        stops.push((at, x));
        for (index, c) in text.chars().enumerate() {
            if index > 0
                && let Some(p) = previous
            {
                x += font.horizontal_kern(p, c, size).unwrap_or(0.);
            }
            x += font.metrics(c, size).advance_width;
            previous = Some(c);
        }
        display.push_str(&text);
    }
    stops.push((range.end, x));
    Row {
        range,
        stops: stops.into(),
        display,
    }
}
fn rows(font: &fontdue::Font, value: &str, size: f32, width: f32) -> Vec<Row> {
    let boundaries: Vec<_> = TextSelection::boundaries(value).collect();
    let mut result = Vec::new();
    let mut start = 0;
    while start + 1 < boundaries.len() {
        let mut index = start;
        let mut x = 0.;
        let mut previous = None;
        let mut whitespace = None;
        let mut hard_end = None;
        while index + 1 < boundaries.len() {
            let grapheme = &value[boundaries[index]..boundaries[index + 1]];
            if grapheme.contains(['\r', '\n']) {
                hard_end = Some(index);
                break;
            }
            let mut end = x;
            let mut last = previous;
            for c in display_grapheme(grapheme).chars() {
                if let Some(p) = last {
                    end += font.horizontal_kern(p, c, size).unwrap_or(0.);
                }
                end += font.metrics(c, size).advance_width;
                last = Some(c);
            }
            // Always admit one whole grapheme, even in a zero-width viewport.
            if end > width && index > start {
                break;
            }
            x = end;
            previous = last;
            index += 1;
            if grapheme.chars().all(char::is_whitespace) {
                whitespace = Some(index);
            }
        }
        let end = if hard_end.is_some() || index + 1 == boundaries.len() {
            index
        } else {
            whitespace.filter(|last| *last > start).unwrap_or(index)
        };
        result.push(measured_row(
            font,
            value,
            boundaries[start]..boundaries[end],
            size,
        ));
        start = if hard_end == Some(end) { end + 1 } else { end };
    }
    // Empty values and trailing hard lines are real editable rows.
    if result.is_empty() || value.ends_with(['\r', '\n']) {
        result.push(measured_row(font, value, value.len()..value.len(), size));
    }
    result
}
#[allow(clippy::too_many_arguments)]
pub(super) fn geometry(
    font: &fontdue::Font,
    value: &str,
    selection: TextSelection,
    size: f32,
    inner: Rect,
    spec: ScrollSpec,
    id: &str,
    navigation: Option<&Navigation>,
) -> TextHit {
    let selection = selection.reconciled(value);
    let line_height = text::line_metrics(font, size).height;
    let rows = rows(font, value, size, inner.w);
    let default = rows
        .iter()
        .rposition(|row| row.range.start <= selection.caret())
        .unwrap_or(0);
    let caret_row = navigation
        .and_then(|nav| nav.affinity(id, value, selection, inner, line_height, &rows))
        .filter(|row| {
            rows.get(*row).is_some_and(|row| {
                row.range.contains(&selection.caret()) || row.range.end == selection.caret()
            })
        })
        .unwrap_or(default);
    let content_height = rows.len() as f32 * line_height;
    TextHit {
        value: Arc::from(value),
        selection,
        inner,
        scroll: spec.offset().min((content_height - inner.h).max(0.)),
        boundaries: Arc::from([]),
        area: Some(Arc::new(AreaGeometry {
            rows: rows.into(),
            line_height,
            caret_row,
            content_height,
        })),
    }
}
pub(super) fn at(hit: &TextHit, x: f32, y: f32) -> Option<(usize, usize)> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let area = hit.area.as_ref()?;
    let row = (((y - hit.inner.y + hit.scroll) / area.line_height)
        .floor()
        .max(0.) as usize)
        .min(area.rows.len() - 1);
    Some((area.rows[row].at(x - hit.inner.x), row))
}
pub(super) fn pointer_navigation<M>(
    hit: &Hit<M>,
    x: f32,
    y: f32,
    anchor: usize,
) -> Option<Navigation> {
    let text = hit.text.as_ref()?;
    let (caret, row) = at(text, x, y)?;
    Some(Navigation {
        id: hit.id.clone(),
        value: text.value.clone(),
        anchor,
        caret,
        row,
        goal_x: None,
        width: text.inner.w,
        height: text.inner.h,
        line_height: text.area.as_ref()?.line_height,
        rows: text.area.as_ref()?.rows.clone(),
    })
}
pub(super) fn key<M>(
    hit: &Hit<M>,
    input_key: &Key,
    navigation: &mut Option<Navigation>,
) -> Option<TextEdit> {
    let text = hit.text.as_ref()?;
    let area = text.area.as_ref()?;
    let (key, modifiers) = match input_key {
        Key::Modified { key, modifiers } => (key.as_ref(), *modifiers),
        _ => (input_key, KeyModifiers::default()),
    };
    if modifiers.alt || modifiers.logo {
        *navigation = None;
        return None;
    }
    if matches!(key, Key::Enter) && !modifiers.control {
        *navigation = None;
        return Some(TextEdit::Insert("\n".into()));
    }
    let current = navigation.as_ref().filter(|nav| nav.compatible(hit));
    let row = current.map_or(area.caret_row, |nav| nav.row);
    let goal_x = current
        .and_then(|nav| nav.goal_x)
        .unwrap_or_else(|| area.rows[row].x(text.selection.caret()));
    let page = (text.inner.h / area.line_height).floor().max(1.) as usize;
    let target_row = match key {
        Key::Up if !modifiers.control => Some(row.saturating_sub(1)),
        Key::Down if !modifiers.control => Some((row + 1).min(area.rows.len() - 1)),
        Key::PageUp if !modifiers.control => Some(row.saturating_sub(page)),
        Key::PageDown if !modifiers.control => Some((row + page).min(area.rows.len() - 1)),
        _ => None,
    };
    let (caret, target_row, goal) = if let Some(target) = target_row {
        (area.rows[target].at(goal_x), target, Some(goal_x))
    } else {
        match key {
            Key::Home if !modifiers.control => (area.rows[row].range.start, row, None),
            Key::End if !modifiers.control => (area.rows[row].range.end, row, None),
            _ => {
                *navigation = None;
                return super::text_input::key(input_key);
            }
        }
    };
    let anchor = if modifiers.shift {
        text.selection.anchor()
    } else {
        caret
    };
    *navigation = Some(Navigation {
        id: hit.id.clone(),
        value: text.value.clone(),
        anchor,
        caret,
        row: target_row,
        goal_x: goal,
        width: text.inner.w,
        height: text.inner.h,
        line_height: area.line_height,
        rows: area.rows.clone(),
    });
    Some(TextEdit::Select { anchor, caret })
}
pub(super) fn reveal<M>(hit: &Hit<M>, previous: &TextHit) -> Option<M> {
    let text = hit.text.as_ref()?;
    let area = text.area.as_ref()?;
    // Scrolling alone must not pull an unchanged caret back into view.
    if text.value == previous.value
        && text.selection == previous.selection
        && text.inner.w == previous.inner.w
        && text.inner.h == previous.inner.h
        && previous.area.as_ref().is_some_and(|old| {
            old.caret_row == area.caret_row
                && old.line_height == area.line_height
                && old.rows == area.rows
        })
    {
        return None;
    }
    let scroll = hit.scroll.as_ref()?;
    let top = area.caret_row as f32 * area.line_height;
    let value = if top < scroll.offset {
        top
    } else if top + area.line_height > scroll.offset + text.inner.h {
        top + area.line_height - text.inner.h
    } else {
        scroll.offset
    }
    .clamp(0., scroll.maximum);
    (value != scroll.offset).then(|| scroll.event(value))
}
pub(super) fn paint<M>(
    scene: &mut Scene<M>,
    element: &Element<M>,
    hit: &TextHit,
    clip: Clip,
    alpha: f32,
    focused: bool,
    font: &fontdue::Font,
) {
    let Kind::TextArea {
        value, placeholder, ..
    } = &element.kind
    else {
        return;
    };
    let area = hit.area.as_ref().expect("prepared text area");
    let style = &element.style;
    let metrics = text::line_metrics(font, style.font_size);
    let range = hit.selection.range(value);
    let first = ((hit.scroll / area.line_height).floor() as usize).saturating_sub(1);
    let end =
        (((hit.scroll + hit.inner.h) / area.line_height).ceil() as usize + 1).min(area.rows.len());
    for (index, row) in area.rows.iter().enumerate().take(end).skip(first) {
        let y = hit.inner.y + index as f32 * area.line_height - hit.scroll;
        let hard_break_selected = area.rows.get(index + 1).is_some_and(|next| {
            next.range.start > row.range.end
                && range.start <= row.range.end
                && range.end > row.range.end
        });
        if focused
            && ((range.start < row.range.end && range.end > row.range.start) || hard_break_selected)
        {
            let a = row.x(range.start.max(row.range.start).min(row.range.end));
            let b = row.x(range.end.min(row.range.end))
                + if hard_break_selected {
                    font.metrics(' ', style.font_size).advance_width
                } else {
                    0.
                };
            scene.paint.push(Paint::Shape {
                rect: Rect::new(
                    hit.inner.x + a,
                    y + metrics.baseline - metrics.ascent,
                    (b - a).max(0.),
                    metrics.ascent - metrics.descent,
                ),
                clip: clip.clone(),
                color: style
                    .text_selection_color
                    .alpha(style.text_selection_color.3 * alpha),
                radius: 0.,
            });
        }
        let content = if value.is_empty() {
            placeholder.lines().next().unwrap_or("").to_owned()
        } else {
            row.visible_display(font, style.font_size, hit.inner.w)
        };
        if !content.is_empty() {
            scene.paint.push(Paint::Text {
                rect: Rect::new(hit.inner.x, y, hit.inner.w, area.line_height),
                clip: clip.clone(),
                text: content,
                size: style.font_size,
                face: style.font_face,
                color: style.foreground.alpha(
                    style.foreground.3
                        * alpha
                        * if value.is_empty() {
                            style.placeholder_opacity
                        } else {
                            1.
                        },
                ),
                align: Align::Start,
            });
        }
    }
    if focused && area.caret_row >= first && area.caret_row < end {
        let row = &area.rows[area.caret_row];
        scene.paint.push(Paint::Shape {
            rect: Rect::new(
                (hit.inner.x + row.x(hit.selection.caret()))
                    .min(hit.inner.x + (hit.inner.w - style.caret_width).max(0.))
                    .max(hit.inner.x),
                hit.inner.y + area.caret_row as f32 * area.line_height - hit.scroll
                    + metrics.baseline
                    - metrics.ascent,
                style.caret_width,
                metrics.ascent - metrics.descent,
            ),
            clip,
            color: style.foreground.alpha(style.foreground.3 * alpha),
            radius: 0.,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Edit(TextEdit),
        Scroll(ScrollEvent),
        Outer(ScrollEvent),
        Submit,
    }
    fn layout() -> Layout {
        Layout::new(Arc::new(vec![
            fontdue::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
                fontdue::FontSettings::default(),
            )
            .unwrap(),
        ]))
    }
    fn tree(
        value: &str,
        field: &TextFieldState,
        offset: f32,
        width: f32,
        height: f32,
    ) -> Element<Message> {
        Element::text_area(
            value,
            field.selection(),
            "Write",
            ScrollSpec::new(offset, 24.).unwrap(),
            Message::Edit,
            Message::Scroll,
        )
        .id("area")
        .autofocus()
        .size(width, height)
        .font(20.)
        .scroll_style(ScrollStyle {
            gutter: 12.,
            width: 4.,
            ..Default::default()
        })
    }
    fn apply(
        field: &mut TextFieldState,
        value: &mut String,
        offset: &mut f32,
        messages: Vec<Message>,
    ) {
        for message in messages {
            match message {
                Message::Edit(edit) => {
                    field
                        .update(
                            value,
                            edit,
                            8192,
                            ClipboardTarget::new("test", "area"),
                            &mut Effects::default(),
                            Message::Edit,
                        )
                        .unwrap();
                }
                Message::Scroll(event) => *offset = event.offset,
                _ => panic!("unexpected message"),
            }
        }
    }
    fn build(
        layout: &Layout,
        input: &mut Interaction<Message>,
        field: &TextFieldState,
        value: &str,
        offset: f32,
        width: f32,
        height: f32,
    ) -> Scene<Message> {
        let scene = layout.build(
            &tree(value, field, offset, width, height),
            width,
            height,
            input,
            0.,
        );
        input.synchronize(&scene);
        scene
    }
    fn select(field: &mut TextFieldState, value: &mut String, anchor: usize, caret: usize) {
        apply(
            field,
            value,
            &mut 0.,
            vec![Message::Edit(TextEdit::Select { anchor, caret })],
        );
    }
    fn text(scene: &Scene<Message>) -> &TextHit {
        scene.hits[0].text.as_ref().unwrap()
    }
    fn shifted(key: Key) -> Key {
        Key::Modified {
            key: Box::new(key),
            modifiers: KeyModifiers {
                shift: true,
                ..Default::default()
            },
        }
    }
    #[test]
    fn hard_lines_blank_trailing_lines_tabs_and_source_offsets_are_preserved() {
        let layout = layout();
        let font = &layout.fonts[0];
        let source = " a  \r\n\n\tb\r";
        let result = rows(font, source, 20., 1000.);
        assert_eq!(
            result
                .iter()
                .map(|row| &source[row.range.clone()])
                .collect::<Vec<_>>(),
            [" a  ", "", "\tb", ""]
        );
        assert_eq!(result[2].display, "    b");
        assert_eq!(result.last().unwrap().range, source.len()..source.len());
        assert_eq!(rows(font, "", 20., 0.).len(), 1);
        for row in rows(font, "a👩‍💻e\u{301}b", 20., 1.) {
            assert_eq!(
                row.stops.len(),
                2,
                "zero/narrow widths preserve whole graphemes"
            );
        }
        let spaces = rows(font, "aa  bb cc", 20., text::width(font, "aa  ", 20.) + 0.1);
        assert_eq!(spaces[0].display, "aa  ");
        assert_eq!(
            spaces
                .iter()
                .map(|row| row.display.as_str())
                .collect::<String>(),
            "aa  bb cc"
        );
    }
    #[test]
    fn sticky_x_survives_short_rows_and_shift_preserves_direction() {
        let layout = layout();
        let mut input = Interaction::default();
        let mut field = TextFieldState::multiline();
        let mut value = "abcdef\nx\nabcdef".to_owned();
        let mut offset = 0.;
        select(&mut field, &mut value, 5, 5);
        for (key, expected) in [(Key::Down, 8), (Key::Down, 14), (shifted(Key::Up), 8)] {
            let scene = build(&layout, &mut input, &field, &value, offset, 500., 100.);
            let messages = input.key(&scene, &key);
            apply(&mut field, &mut value, &mut offset, messages);
            assert_eq!(field.selection().caret(), expected);
        }
        assert_eq!(field.selection().anchor(), 14);
        assert_eq!(field.selection().selected(&value), "\nabcde");
    }
    #[test]
    fn visual_end_and_pointer_retain_upstream_affinity_at_soft_wrap() {
        let layout = layout();
        let width = text::width(&layout.fonts[0], "ab", 20.) + 0.1 + 12.;
        let mut field = TextFieldState::multiline();
        let mut value = "abcd".to_owned();
        let mut input = Interaction::default();
        let mut offset = 0.;
        let scene = build(&layout, &mut input, &field, &value, offset, width, 100.);
        assert_eq!(text(&scene).area.as_ref().unwrap().rows[0].range, 0..2);
        apply(
            &mut field,
            &mut value,
            &mut offset,
            input.key(&scene, &Key::End),
        );
        let scene = build(&layout, &mut input, &field, &value, offset, width, 100.);
        assert_eq!(field.selection().caret(), 2);
        assert_eq!(text(&scene).area.as_ref().unwrap().caret_row, 0);
        apply(
            &mut field,
            &mut value,
            &mut offset,
            input.key(&scene, &Key::Down),
        );
        assert_eq!(field.selection().caret(), 4);
        let scene = build(&layout, &mut input, &field, &value, offset, width, 100.);
        let row = &text(&scene).area.as_ref().unwrap().rows[0];
        let x = text(&scene).inner.x + row.x(2);
        let y = text(&scene).inner.y + 1.;
        let message = input
            .press(
                &scene,
                x.min(text(&scene).inner.x + text(&scene).inner.w - 0.01),
                y,
            )
            .unwrap();
        apply(&mut field, &mut value, &mut offset, vec![message]);
        let scene = build(&layout, &mut input, &field, &value, offset, width, 100.);
        assert_eq!(field.selection().caret(), 2);
        assert_eq!(text(&scene).area.as_ref().unwrap().caret_row, 0);
        let message = input.release(x, y).unwrap();
        apply(&mut field, &mut value, &mut offset, vec![message]);
        let control_home = Key::Modified {
            key: Box::new(Key::Home),
            modifiers: KeyModifiers {
                control: true,
                ..Default::default()
            },
        };
        let scene = build(&layout, &mut input, &field, &value, offset, width, 100.);
        apply(
            &mut field,
            &mut value,
            &mut offset,
            input.key(&scene, &control_home),
        );
        assert_eq!(field.selection().caret(), 0);
    }
    #[test]
    fn cpu_refresh_routes_burst_edits_against_current_rows_while_frame_is_pending() {
        let layout = layout();
        let mut input = Interaction::default();
        let mut field = TextFieldState::multiline();
        let mut value = "aaaa\nb\ncccc".to_owned();
        let mut offset = 0.;
        let mut demand = FrameDemand::default();
        demand.invalidate();
        assert!(demand.begin()); // A previous frame is waiting for its callback.
        for (key, expected) in [
            (Key::Text("X".into()), 1),
            (Key::Down, 7),
            (Key::Down, 9),
            (shifted(Key::Up), 7),
        ] {
            let previous = build(&layout, &mut input, &field, &value, offset, 500., 48.);
            apply(
                &mut field,
                &mut value,
                &mut offset,
                input.key(&previous, &key),
            );
            let current = build(&layout, &mut input, &field, &value, offset, 500., 48.);
            apply(
                &mut field,
                &mut value,
                &mut offset,
                input.text_caret_reveal(&previous, &current),
            );
            assert_eq!(field.selection().caret(), expected);
            demand.invalidate();
            assert!(
                !demand.begin(),
                "CPU geometry refresh cannot admit another GPU frame"
            );
        }
        assert_eq!(field.selection().anchor(), 9);
        assert_eq!(field.selection().selected(&value), "\nc");
        assert_eq!(value, "Xaaaa\nb\ncccc");
    }
    #[test]
    fn pointer_drag_is_two_dimensional_and_scrollbar_never_selects_text() {
        let layout = layout();
        let mut input = Interaction::default();
        let mut field = TextFieldState::multiline();
        let mut value = "ab\ncd\nef\ngh\nij\nkl".to_owned();
        let mut offset = 0.;
        let scene = build(&layout, &mut input, &field, &value, offset, 200., 60.);
        let area = text(&scene).area.as_ref().unwrap();
        let first_x = area.rows[0].x(1);
        let second_x = area.rows[1].x(4);
        let line_height = area.line_height;
        let message = input.press(&scene, first_x, 1.).unwrap();
        apply(&mut field, &mut value, &mut offset, vec![message]);
        let messages = input.motion(&scene, second_x, line_height + 1., 0.);
        apply(&mut field, &mut value, &mut offset, messages);
        assert_eq!(field.selection().selected(&value), "b\nc");
        let message = input.release(second_x, line_height + 1.).unwrap();
        apply(&mut field, &mut value, &mut offset, vec![message]);
        let scene = build(&layout, &mut input, &field, &value, offset, 200., 60.);
        let selected = field.selection();
        let scroll = scene.hits[0].scroll.as_ref().unwrap();
        let x = scroll.track.x + scroll.track.w / 2.;
        let y = scroll.track.y + scroll.track.h - 1.;
        assert!(matches!(
            input.press(&scene, x, y),
            Some(Message::Scroll(_))
        ));
        assert!(matches!(input.release(x, y), Some(Message::Scroll(_))));
        assert_eq!(field.selection(), selected);
    }
    #[test]
    fn editor_navigation_precedes_nested_scroll_and_wheel_away_does_not_reveal_caret() {
        let layout = layout();
        let mut input = Interaction::default();
        let mut field = TextFieldState::multiline();
        let mut value = "a\nb\nc\nd\ne\nf\ng\nh\ni".to_owned();
        let mut offset = 0.;
        let inner = tree(&value, &field, offset, 200., 60.);
        let outer = Element::scroll(ScrollSpec::new(0., 24.).unwrap(), inner, Message::Outer)
            .size(200., 60.);
        let scene = layout.build(&outer, 200., 60., &input, 0.);
        input.synchronize(&scene);
        assert!(matches!(
            input.key(&scene, &Key::PageDown).as_slice(),
            [Message::Edit(TextEdit::Select { .. })]
        ));
        let previous = build(&layout, &mut input, &field, &value, offset, 200., 60.);
        let messages = input.scroll(&previous, 10., 10., 2.).unwrap();
        apply(&mut field, &mut value, &mut offset, messages);
        assert!(offset > 0.);
        let current = build(&layout, &mut input, &field, &value, offset, 200., 60.);
        assert!(input.text_caret_reveal(&previous, &current).is_empty());
    }
    #[test]
    fn only_visible_rows_are_painted_and_geometry_changes_retire_text_capture() {
        let layout = layout();
        let mut input = Interaction::default();
        let field = TextFieldState::multiline();
        let value = "line\n".repeat(1500);
        let scene = build(&layout, &mut input, &field, &value, 5000., 200., 60.);
        assert_eq!(text(&scene).area.as_ref().unwrap().rows.len(), 1501);
        let painted = scene
            .paint
            .iter()
            .filter(|paint| matches!(paint, Paint::Text { .. }))
            .count();
        assert!(painted <= 6, "offscreen rows have no text masks");
        assert!(scene.paint.iter().all(|paint| match paint {
            Paint::Text { text, rect, .. } => !text.contains('\n') && rect.h < 1024.,
            _ => true,
        }));
        input.press(&scene, 10., 10.);
        assert!(input.press.is_some());
        let _resized = build(&layout, &mut input, &field, &value, 5000., 100., 60.);
        assert!(input.press.is_none());
    }
    #[test]
    fn single_line_submission_and_masked_enter_remain_unchanged() {
        let layout = layout();
        let mut input = Interaction::default();
        let ordinary = Element::text_input("", TextSelection::default(), "", Message::Edit)
            .id("ordinary")
            .on_submit(Message::Submit)
            .autofocus()
            .size(200., 60.);
        let scene = layout.build(&ordinary, 200., 60., &input, 0.);
        input.synchronize(&scene);
        assert_eq!(input.key(&scene, &Key::Enter), [Message::Submit]);
        let control_enter = Key::Modified {
            key: Box::new(Key::Enter),
            modifiers: KeyModifiers {
                control: true,
                ..Default::default()
            },
        };
        assert_eq!(input.key(&scene, &control_enter), [Message::Submit]);
        let masked = Element::masked_input(4, "", |edit| edit)
            .id("secret")
            .on_submit(InputEdit::Clear)
            .autofocus()
            .size(200., 60.);
        let mut masked_input = Interaction::default();
        let masked_scene = layout.build(&masked, 200., 60., &masked_input, 0.);
        masked_input.synchronize(&masked_scene);
        assert_eq!(
            masked_input.key(&masked_scene, &control_enter),
            [InputEdit::Clear]
        );
        assert!(
            masked_scene
                .hits
                .iter()
                .all(|hit| hit.value.is_empty() && hit.text.is_none())
        );
        let mut field = TextFieldState::multiline();
        let mut value = String::new();
        input.focus = None;
        let scene = build(&layout, &mut input, &field, &value, 0., 200., 60.);
        assert!(input.key(&scene, &control_enter).is_empty());
        apply(
            &mut field,
            &mut value,
            &mut 0.,
            input.key(&scene, &Key::Enter),
        );
        assert_eq!(value, "\n");
    }

    /// Opt-in CPU evidence only. Run with an explicit Cargo profile and retain
    /// the invocation with these timings; debug assertions are printed as well.
    #[test]
    #[ignore = "opt-in bounded TextArea CPU measurements; no timing pass thresholds"]
    fn text_area_bounded_cpu_measurement() {
        use std::{hint::black_box, time::Instant};
        let layout = layout();
        let viewport = (96., 80.);
        let control = |key| Key::Modified {
            key: Box::new(key),
            modifiers: KeyModifiers {
                control: true,
                ..Default::default()
            },
        };
        let keys = [
            control(Key::Home),
            Key::Down,
            Key::Down,
            shifted(Key::Up),
            Key::PageDown,
            Key::End,
            control(Key::End),
            Key::PageUp,
        ];
        let cases = [
            ("unbroken-line", "w".repeat(8192)),
            (
                "hard-and-blank-lines",
                format!("{}\n\n", "a\n\n".repeat(2730)),
            ),
            ("combining-cluster", format!("a{}b", "\u{301}".repeat(4095))),
        ];
        let statistics = |samples: &mut [f64]| {
            samples.sort_by(f64::total_cmp);
            (
                samples.iter().sum::<f64>() / samples.len() as f64,
                samples[(samples.len() * 95).div_ceil(100) - 1],
                samples[samples.len() - 1],
            )
        };
        for (name, original) in cases {
            assert_eq!(original.len(), 8192);
            let original_tail = original[8191..].to_owned();
            let mut value = original;
            let mut field = TextFieldState::multiline();
            let mut input = Interaction::default();
            let mut offset = 0.;
            let mut max_rows = 0;
            let mut max_text_rows = 0;
            let mut max_primitives = 0;
            let mut max_row_text_bytes = 0;
            let mut observe = |scene: &Scene<Message>| {
                let text = text(scene);
                let area = text.area.as_ref().unwrap();
                max_rows = max_rows.max(area.rows.len());
                max_row_text_bytes = max_row_text_bytes
                    .max(area.rows.iter().map(|row| row.display.len()).sum::<usize>());
                let text_rows = scene
                    .paint
                    .iter()
                    .filter(|paint| matches!(paint, Paint::Text { .. }))
                    .count();
                max_text_rows = max_text_rows.max(text_rows);
                max_primitives = max_primitives.max(scene.paint.len());
                // Fractional row position plus one row of overscan at each edge.
                assert!(text_rows <= (text.inner.h / area.line_height).ceil() as usize + 3);
                for paint in &scene.paint {
                    let (rect, clip) = match paint {
                        Paint::Outline { rect, clip, .. }
                        | Paint::Shape { rect, clip, .. }
                        | Paint::Shadow { rect, clip, .. }
                        | Paint::Text { rect, clip, .. }
                        | Paint::Image { rect, clip, .. } => (rect, &clip.bounds),
                    };
                    for bounds in [rect, clip] {
                        assert!(
                            [bounds.x, bounds.y, bounds.w, bounds.h]
                                .iter()
                                .all(|value| value.is_finite())
                        );
                        assert!(bounds.w >= 0. && bounds.h >= 0.);
                    }
                }
            };
            // Establish autofocus before timed steady-state layout samples.
            let warm = build(
                &layout, &mut input, &field, &value, offset, viewport.0, viewport.1,
            );
            observe(&warm);
            drop(warm);
            let mut layout_ms = Vec::with_capacity(32);
            for _ in 0..32 {
                let start = Instant::now();
                let scene = black_box(build(
                    &layout, &mut input, &field, &value, offset, viewport.0, viewport.1,
                ));
                layout_ms.push(start.elapsed().as_secs_f64() * 1000.);
                observe(&scene);
            }
            let mut content_ms = Vec::with_capacity(16);
            let mut navigation_ms = Vec::with_capacity(16 * keys.len());
            for cycle in 0..16 {
                for step in 0..=keys.len() {
                    let start = Instant::now();
                    let previous = build(
                        &layout, &mut input, &field, &value, offset, viewport.0, viewport.1,
                    );
                    if step == 0 {
                        // Replace one ASCII tail byte, keeping every workload
                        // exactly at the 8192-byte admission boundary throughout.
                        select(&mut field, &mut value, 8191, 8192);
                        let replacement = if cycle % 2 == 0 { "x" } else { &original_tail };
                        apply(
                            &mut field,
                            &mut value,
                            &mut offset,
                            vec![Message::Edit(TextEdit::Insert(replacement.into()))],
                        );
                    } else {
                        apply(
                            &mut field,
                            &mut value,
                            &mut offset,
                            input.key(&previous, &keys[step - 1]),
                        );
                    }
                    let current = build(
                        &layout, &mut input, &field, &value, offset, viewport.0, viewport.1,
                    );
                    let reveal = input.text_caret_reveal(&previous, &current);
                    let settled = if reveal.is_empty() {
                        None
                    } else {
                        apply(&mut field, &mut value, &mut offset, reveal);
                        Some(build(
                            &layout, &mut input, &field, &value, offset, viewport.0, viewport.1,
                        ))
                    };
                    let elapsed = start.elapsed().as_secs_f64() * 1000.;
                    if step == 0 {
                        content_ms.push(elapsed);
                    } else {
                        navigation_ms.push(elapsed);
                    }
                    // Count/geometry assertions are outside the timed samples.
                    assert_eq!(value.len(), 8192);
                    assert!(
                        TextSelection::boundaries(&value).any(|at| at == field.selection().caret())
                    );
                    observe(&previous);
                    observe(&current);
                    if let Some(scene) = settled {
                        observe(&scene);
                    }
                }
            }
            let (layout_mean, layout_p95, layout_max) = statistics(&mut layout_ms);
            let (content_mean, content_p95, content_max) = statistics(&mut content_ms);
            let (navigation_mean, navigation_p95, navigation_max) = statistics(&mut navigation_ms);
            println!(
                concat!(
                    "TEXT_AREA_CPU {{\"case\":\"{}\",\"debug_assertions\":{},\"bytes\":8192,",
                    "\"viewport\":[96,80],\"font_size\":20,\"layout_samples\":32,",
                    "\"layout_mean_ms\":{:.4},\"layout_p95_ms\":{:.4},\"layout_max_ms\":{:.4},",
                    "\"content_samples\":16,\"content_mean_ms\":{:.4},\"content_p95_ms\":{:.4},\"content_max_ms\":{:.4},",
                    "\"navigation_samples\":128,\"navigation_mean_ms\":{:.4},\"navigation_p95_ms\":{:.4},\"navigation_max_ms\":{:.4},",
                    "\"max_visual_rows\":{},\"max_painted_text_rows\":{},\"max_paint_primitives\":{},\"max_row_text_bytes\":{},",
                    "\"scope\":\"CPU view/layout/synchronize; edits include current-model update and caret reveal; excludes GPU, native delivery, count checks and scene teardown\"}}"
                ),
                name,
                cfg!(debug_assertions),
                layout_mean,
                layout_p95,
                layout_max,
                content_mean,
                content_p95,
                content_max,
                navigation_mean,
                navigation_p95,
                navigation_max,
                max_rows,
                max_text_rows,
                max_primitives,
                max_row_text_bytes
            );
        }
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    fn layout() -> Layout {
        Layout::new(Arc::new(vec![
            fontdue::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
                fontdue::FontSettings::default(),
            )
            .unwrap(),
        ]))
    }
    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Edit(TextEdit),
        Scroll(ScrollEvent),
    }
    fn tree(
        value: &str,
        selection: TextSelection,
        offset: f32,
        width: f32,
        size: f32,
    ) -> Element<Message> {
        Element::text_area(
            value,
            selection,
            "",
            ScrollSpec::new(offset, 24.).unwrap(),
            Message::Edit,
            Message::Scroll,
        )
        .id("area")
        .autofocus()
        .font(size)
        .size(width, 60.)
        .scroll_style(ScrollStyle {
            gutter: 12.,
            ..Default::default()
        })
    }
    #[test]
    fn affinity_only_drag_updates_the_row_even_when_source_byte_is_identical() {
        let layout = layout();
        let width = text::width(&layout.fonts[0], "ab", 20.) + 12.1;
        let value = "abcd";
        let mut input = Interaction::default();
        let scene = layout.build(
            &tree(value, TextSelection::default(), 0., width, 20.),
            width,
            60.,
            &input,
            0.,
        );
        input.synchronize(&scene);
        let text = scene.hits[0].text.as_ref().unwrap();
        let area = text.area.as_ref().unwrap();
        let x = area.rows[0].x(2).min(text.inner.w - 0.01);
        let first = input.press(&scene, x, 1.).unwrap();
        assert!(matches!(
            first,
            Message::Edit(TextEdit::Select { caret: 2, .. })
        ));
        let next = input.motion(&scene, text.inner.x, area.line_height + 1., 0.);
        assert!(matches!(
            next.as_slice(),
            [Message::Edit(TextEdit::Select { caret: 2, .. })]
        ));
        assert_eq!(input.text_navigation.as_ref().unwrap().row, 1);
    }
    #[test]
    fn reveal_scrolling_preserves_selection_and_thumb_captures_until_release() {
        let layout = layout();
        let value = "a\nb\nc\nd\ne\nf\ng\nh\ni";
        let mut input = Interaction::default();
        let mut selection = TextSelection::default();
        let mut owned = value.to_owned();
        let scene = layout.build(
            &tree(value, selection, 0., 200., 20.),
            200.,
            60.,
            &input,
            0.,
        );
        input.synchronize(&scene);
        input.press(&scene, 0., 1.);
        let messages = input.motion(&scene, 0., 90., 0.);
        let [Message::Edit(edit)] = messages.as_slice() else {
            panic!("selection intent required")
        };
        selection.apply(&mut owned, edit.clone(), 8192).unwrap();
        let current = layout.build(
            &tree(value, selection, 0., 200., 20.),
            200.,
            60.,
            &input,
            0.,
        );
        input.synchronize(&current);
        let reveal = input.text_caret_reveal(&scene, &current);
        let [Message::Scroll(event)] = reveal.as_slice() else {
            panic!("caret reveal required")
        };
        let scrolled = layout.build(
            &tree(value, selection, event.offset, 200., 20.),
            200.,
            60.,
            &input,
            0.,
        );
        input.synchronize(&scrolled);
        assert!(
            input.press.is_some(),
            "own reveal must rebase live selection capture"
        );
        assert!(input.release(0., 40.).is_some());
        let scroll = scrolled.hits[0].scroll.as_ref().unwrap();
        let x = scroll.thumb.x + scroll.thumb.w / 2.;
        let y = scroll.thumb.y + 1.;
        assert!(matches!(
            input.press(&scrolled, x, y),
            Some(Message::Scroll(_))
        ));
        let moved = input.motion(&scrolled, x, y + 4., 0.);
        let [Message::Scroll(event)] = moved.as_slice() else {
            panic!("thumb intent required")
        };
        let next = layout.build(
            &tree(value, selection, event.offset, 200., 20.),
            200.,
            60.,
            &input,
            0.,
        );
        input.synchronize(&next);
        assert!(
            input.press.is_some(),
            "scrollbar offset changes preserve its capture"
        );
        assert!(matches!(input.release(x, y + 6.), Some(Message::Scroll(_))));
    }
    #[test]
    fn font_growth_reveals_caret_and_packed_rows_keep_caret_inside_clip() {
        let layout = layout();
        let mut value = "a\nb\nc".to_owned();
        let mut selection = TextSelection::default();
        selection
            .apply(
                &mut value,
                TextEdit::Move {
                    movement: TextMove::End,
                    extend: false,
                },
                8192,
            )
            .unwrap();
        let mut input = Interaction::default();
        let previous = layout.build(
            &tree(&value, selection, 0., 200., 10.),
            200.,
            60.,
            &input,
            0.,
        );
        input.synchronize(&previous);
        let current = layout.build(
            &tree(&value, selection, 0., 200., 30.),
            200.,
            60.,
            &input,
            0.,
        );
        input.synchronize(&current);
        assert!(matches!(
            input.text_caret_reveal(&previous, &current).as_slice(),
            [Message::Scroll(_)]
        ));
        let value = "ab";
        let width = text::width(&layout.fonts[0], value, 20.) + 12.;
        let scene = layout.build(
            &tree(value, TextSelection::at_end(value), 0., width, 20.),
            width,
            60.,
            &input,
            0.,
        );
        let inner = scene.hits[0].text.as_ref().unwrap().inner;
        let caret = scene
            .paint
            .iter()
            .find_map(|paint| match paint {
                Paint::Shape { rect, .. } if rect.w == 1. => Some(rect),
                _ => None,
            })
            .unwrap();
        assert!(caret.x >= inner.x && caret.x + caret.w <= inner.x + inner.w);
    }
}

#[cfg(test)]
mod oversized_grapheme_tests {
    use super::*;
    #[test]
    fn one_oversized_grapheme_paints_only_visible_ink_without_splitting_caret_geometry() {
        let font = fontdue::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .unwrap();
        let value = format!("{}💻", "👩‍".repeat(500));
        assert!(value.len() <= 8192);
        assert_eq!(TextSelection::boundaries(&value).count(), 2);
        let rows = rows(&font, &value, 20., 100.);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].stops.len(),
            2,
            "the edit model still owns one whole grapheme"
        );
        let visible = rows[0].visible_display(&font, 20., 100.);
        assert!(visible.len() < value.len());
        assert!(text::width(&font, &visible, 20.) < 200.);
        let expected = text::glyph_positions(&font, &value, 20.)
            .take(visible.chars().count())
            .collect::<Vec<_>>();
        assert_eq!(
            text::glyph_positions(&font, &visible, 20.).collect::<Vec<_>>(),
            expected
        );
    }
}
