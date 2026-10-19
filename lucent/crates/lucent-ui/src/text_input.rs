//! Shared text/caret/selection geometry. Native protocols and client state stay outside.
use super::*;
#[derive(Clone)]
pub struct TextHit {
    pub value: Arc<str>,
    pub selection: TextSelection,
    pub inner: Rect,
    pub scroll: f32,
    pub(super) boundaries: Arc<[(usize, f32)]>,
    pub(super) area: Option<Arc<super::text_area::AreaGeometry>>,
}
impl TextHit {
    pub fn is_multiline(&self) -> bool {
        self.area.is_some()
    }
    pub fn at_point(&self, x: f32, y: f32) -> Option<usize> {
        if self.area.is_some() {
            super::text_area::at(self, x, y).map(|(byte, _)| byte)
        } else {
            self.at(x)
        }
    }
    pub(super) fn same_geometry(&self, other: &Self) -> bool {
        self.value == other.value
            && self.inner == other.inner
            && match (&self.area, &other.area) {
                (Some(a), Some(b)) => {
                    a.line_height == b.line_height
                        && a.rows.len() == b.rows.len()
                        && a.rows
                            .iter()
                            .zip(b.rows.iter())
                            .all(|(a, b)| a.range == b.range && a.stops == b.stops)
                }
                (None, None) => self.boundaries == other.boundaries,
                _ => false,
            }
    }

    pub fn at(&self, x: f32) -> Option<usize> {
        if !x.is_finite() {
            return None;
        }
        let target = x - self.inner.x + self.scroll;
        self.boundaries
            .iter()
            .min_by(|(_, a), (_, b)| (a - target).abs().total_cmp(&(b - target).abs()))
            .map(|(at, _)| *at)
    }
    fn advance(&self, at: usize) -> f32 {
        self.boundaries
            .iter()
            .find(|(byte, _)| *byte == at)
            .map_or(0., |(_, x)| *x)
    }
}
pub(super) fn geometry(
    font: &fontdue::Font,
    value: &str,
    selection: TextSelection,
    size: f32,
    inner: Rect,
    caret_width: f32,
    focused: bool,
) -> TextHit {
    let display = if value.contains(['\n', '\r']) {
        std::borrow::Cow::Owned(value.replace(['\n', '\r'], " "))
    } else {
        std::borrow::Cow::Borrowed(value)
    };
    let selection = selection.reconciled(value);
    let mut stops = TextSelection::boundaries(value).peekable();
    let mut boundaries = vec![];
    let mut end = 0.;
    for ((byte, c), (_, x)) in value
        .char_indices()
        .zip(text::glyph_positions(font, &display, size))
    {
        end = x + font
            .metrics(if matches!(c, '\r' | '\n') { ' ' } else { c }, size)
            .advance_width;
        if stops.peek() == Some(&byte) {
            boundaries.push((byte, x));
            stops.next();
        }
    }
    boundaries.push((value.len(), end));
    let advance = boundaries
        .iter()
        .find(|(byte, _)| *byte == selection.caret())
        .map_or(0., |(_, x)| *x);
    let anchor = boundaries
        .iter()
        .find(|(byte, _)| *byte == selection.anchor())
        .map_or(advance, |(_, x)| *x);
    let visible_edge = if (anchor - advance).abs() + caret_width <= inner.w {
        anchor.max(advance)
    } else {
        advance
    };
    let scroll = if focused {
        (visible_edge + caret_width - inner.w).max(0.)
    } else {
        0.
    };
    TextHit {
        value: Arc::from(value),
        selection,
        inner,
        scroll,
        boundaries: boundaries.into(),
        area: None,
    }
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
    let Kind::TextInput {
        value,
        placeholder,
        selection,
    } = &element.kind
    else {
        return;
    };
    let style = &element.style;
    let inner = hit.inner;
    let metrics = text::line_metrics(font, style.font_size);
    let top = inner.y + (inner.h - metrics.height).max(0.) / 2.;
    let range = selection.range(value);
    if focused && !range.is_empty() {
        let a = hit.advance(range.start);
        let b = hit.advance(range.end);
        scene.paint.push(Paint::Shape {
            rect: Rect::new(
                inner.x + a.min(b) - hit.scroll,
                top + metrics.baseline - metrics.ascent,
                (b - a).abs(),
                metrics.ascent - metrics.descent,
            ),
            clip: clip.clone(),
            color: style
                .text_selection_color
                .alpha(style.text_selection_color.3 * alpha),
            radius: 0.,
        });
    }
    scene.paint.push(Paint::Text {
        rect: Rect::new(inner.x - hit.scroll, inner.y, inner.w, inner.h),
        clip: clip.clone(),
        text: if value.is_empty() {
            placeholder.clone()
        } else {
            value.replace(['\n', '\r'], " ")
        },
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
    if focused {
        let x = (inner.x + hit.advance(hit.selection.caret()) - hit.scroll)
            .min(inner.x + inner.w - style.caret_width);
        scene.paint.push(Paint::Shape {
            rect: Rect::new(
                x,
                top + metrics.baseline - metrics.ascent,
                style.caret_width,
                metrics.ascent - metrics.descent,
            ),
            clip,
            color: style.foreground.alpha(style.foreground.3 * alpha),
            radius: 0.,
        });
    }
}
pub(super) fn key(key: &Key) -> Option<TextEdit> {
    let (key, modifiers) = match key {
        Key::Modified { key, modifiers } => (key.as_ref(), *modifiers),
        _ => (key, KeyModifiers::default()),
    };
    if modifiers.alt || modifiers.logo {
        return None;
    }
    Some(match key {
        Key::Shortcut {
            character,
            modifiers,
        } if modifiers.control && !modifiers.alt && !modifiers.logo => {
            match character.to_ascii_lowercase() {
                'c' => TextEdit::Copy,
                'x' => TextEdit::Cut,
                'v' => TextEdit::Paste,
                'z' if modifiers.shift => TextEdit::Redo,
                'z' => TextEdit::Undo,
                'y' => TextEdit::Redo,
                _ => return None,
            }
        }
        Key::Text(value) => TextEdit::Insert(value.clone()),
        Key::Backspace => {
            if modifiers.control {
                TextEdit::BackspaceWord
            } else {
                TextEdit::Backspace
            }
        }
        Key::Delete => {
            if modifiers.control {
                TextEdit::DeleteWord
            } else {
                TextEdit::Delete
            }
        }
        Key::SelectAll => TextEdit::SelectAll,
        Key::ClearInput => TextEdit::Clear,
        Key::Left => TextEdit::Move {
            movement: if modifiers.control {
                TextMove::PreviousWord
            } else {
                TextMove::PreviousGrapheme
            },
            extend: modifiers.shift,
        },
        Key::Right => TextEdit::Move {
            movement: if modifiers.control {
                TextMove::NextWord
            } else {
                TextMove::NextGrapheme
            },
            extend: modifiers.shift,
        },
        Key::Home => TextEdit::Move {
            movement: TextMove::Start,
            extend: modifiers.shift,
        },
        Key::End => TextEdit::Move {
            movement: TextMove::End,
            extend: modifiers.shift,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod clipboard_keys {
    use super::*;
    fn test_layout() -> Layout {
        Layout::new(Arc::new(vec![
            fontdue::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
                fontdue::FontSettings::default(),
            )
            .unwrap(),
        ]))
    }
    #[test]
    fn text_clipboard_shortcuts_accept_control_without_alt_or_logo() {
        for (character, edit) in [
            ('c', TextEdit::Copy),
            ('x', TextEdit::Cut),
            ('v', TextEdit::Paste),
            ('V', TextEdit::Paste),
            ('z', TextEdit::Undo),
            ('Z', TextEdit::Undo),
            ('y', TextEdit::Redo),
        ] {
            assert_eq!(
                key(&Key::Shortcut {
                    character,
                    modifiers: KeyModifiers {
                        control: true,
                        ..Default::default()
                    }
                }),
                Some(edit)
            );
            assert_eq!(
                key(&Key::Shortcut {
                    character,
                    modifiers: KeyModifiers {
                        control: true,
                        alt: true,
                        ..Default::default()
                    }
                }),
                None
            );
            assert_eq!(
                key(&Key::Shortcut {
                    character,
                    modifiers: KeyModifiers {
                        control: true,
                        logo: true,
                        ..Default::default()
                    }
                }),
                None
            );
        }
    }
    #[test]
    fn control_shift_z_redoes_and_plain_letters_still_type() {
        assert_eq!(
            key(&Key::Shortcut {
                character: 'Z',
                modifiers: KeyModifiers {
                    control: true,
                    shift: true,
                    ..Default::default()
                }
            }),
            Some(TextEdit::Redo)
        );
        assert_eq!(
            key(&Key::Text("z".into())),
            Some(TextEdit::Insert("z".into()))
        );
    }
    #[test]
    fn history_shortcuts_apply_against_current_client_state_before_another_frame() {
        let mut value = "base".to_string();
        let mut field = TextFieldState::default();
        field.reset(&value);
        let tree = Element::text_input(&value, field.selection(), "", |edit| edit)
            .id("input")
            .autofocus()
            .size(180., 40.);
        let mut input = Interaction::default();
        let scene = test_layout().build(&tree, 180., 40., &input, 0.);
        input.synchronize(&scene);
        let mut effects = Effects::default();
        let undo = Key::Shortcut {
            character: 'z',
            modifiers: KeyModifiers {
                control: true,
                ..Default::default()
            },
        };
        let redo = Key::Shortcut {
            character: 'z',
            modifiers: KeyModifiers {
                control: true,
                shift: true,
                ..Default::default()
            },
        };
        for (key, expected) in [
            (Key::Text("é".into()), "baseé"),
            (Key::Text("!".into()), "baseé!"),
            (undo.clone(), "base"),
            (redo.clone(), "baseé!"),
            (Key::Left, "baseé!"),
            (Key::Delete, "baseé"),
            (undo, "baseé!"),
            (redo, "baseé"),
        ] {
            let messages = input.key(&scene, &key);
            assert_eq!(messages.len(), 1);
            for edit in messages {
                field
                    .update(
                        &mut value,
                        edit,
                        8192,
                        ClipboardTarget::new("test", "input"),
                        &mut effects,
                        |edit| edit,
                    )
                    .unwrap();
            }
            assert_eq!(value, expected);
        }
        assert!(input.input_buffer.is_none());
        assert!(effects.clipboard.is_empty());
    }
    #[test]
    fn masked_controls_do_not_route_ordinary_history_shortcuts() {
        let tree = Element::masked_input(5, "", |edit| edit)
            .id("secret")
            .autofocus()
            .size(180., 40.);
        let mut input = Interaction::default();
        let scene = test_layout().build(&tree, 180., 40., &input, 0.);
        input.synchronize(&scene);
        for shift in [false, true] {
            assert!(
                input
                    .key(
                        &scene,
                        &Key::Shortcut {
                            character: 'z',
                            modifiers: KeyModifiers {
                                control: true,
                                shift,
                                ..Default::default()
                            },
                        }
                    )
                    .is_empty()
            );
        }
        assert!(input.input_buffer.is_none());
        assert!(
            scene
                .hits
                .iter()
                .all(|hit| hit.text_edit.is_none() && hit.value.is_empty())
        );
    }
}

#[cfg(test)]
mod placeholder_styling {
    use super::*;
    #[test]
    fn placeholder_opacity_is_explicit_and_entered_text_keeps_its_own_alpha() {
        let layout = Layout::new(Arc::new(vec![
            fontdue::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
                fontdue::FontSettings::default(),
            )
            .unwrap(),
        ]));
        assert_eq!(Style::default().placeholder_opacity, 1.);
        for populated in [false, true] {
            for opacity in [None, Some(0.25)] {
                let value = if populated { "entered" } else { "" };
                let fields = [
                    Element::input(value, "Hint", |_| ()),
                    Element::masked_input(usize::from(populated), "Hint", |_| ()),
                    Element::text_input(value, TextSelection::default(), "Hint", |_| ()),
                    Element::text_area(
                        value,
                        TextSelection::default(),
                        "Hint",
                        ScrollSpec::new(0., 24.).unwrap(),
                        |_| (),
                        |_| (),
                    ),
                ];
                for field in fields {
                    let field = field
                        .size(200., 80.)
                        .font(20.)
                        .color(Color::hex(0xffffff).alpha(0.8))
                        .opacity(0.5);
                    let field = if let Some(opacity) = opacity {
                        field.placeholder_opacity(opacity)
                    } else {
                        field
                    };
                    let scene = layout.build(&field, 200., 80., &Interaction::default(), 0.);
                    let painted = scene
                        .paint
                        .iter()
                        .find_map(|paint| match paint {
                            Paint::Text { color, .. } => Some(color.3),
                            _ => None,
                        })
                        .expect("the control paints ordinary or placeholder text");
                    let expected = 0.8 * 0.5 * if populated { 1. } else { opacity.unwrap_or(1.) };
                    assert!(
                        (painted - expected).abs() < 1e-6,
                        "placeholder style must compose with foreground and ancestor opacity only for empty fields"
                    );
                }
            }
        }
    }
}
