//! Shared text/caret/selection geometry. Native protocols and client state stay outside.
use super::*;
#[derive(Clone)]
pub struct TextHit {
    pub value: Arc<str>,
    pub selection: TextSelection,
    pub inner: Rect,
    pub scroll: f32,
    boundaries: Arc<[(usize, f32)]>,
}
impl TextHit {
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
        color: style
            .foreground
            .alpha(style.foreground.3 * alpha * if value.is_empty() { 0.55 } else { 1. }),
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
    #[test]
    fn text_clipboard_shortcuts_accept_control_without_alt_or_logo() {
        for (character, edit) in [
            ('c', TextEdit::Copy),
            ('x', TextEdit::Cut),
            ('v', TextEdit::Paste),
            ('V', TextEdit::Paste),
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
}
