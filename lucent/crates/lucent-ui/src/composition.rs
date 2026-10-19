//! Transient presentation for input methods. Canonical values remain client-owned.
use super::*;
#[derive(Clone)]
pub(super) struct Composition {
    pub id: String,
    pub value: String,
    pub begin: i32,
    pub end: i32,
}
impl<M: Clone> Interaction<M> {
    /// Runtime-only preedit presentation. Masked/legacy controls are ineligible.
    /// The string is never included in inspector hits or canonical editing state.
    pub fn set_preedit(&mut self, scene: &Scene<M>, value: Option<(String, i32, i32)>) -> bool {
        let next = value.and_then(|(value, begin, end)| {
            let id = self.focus.as_ref()?;
            if value.len() > 4096
                || !scene
                    .hits
                    .iter()
                    .any(|hit| &hit.id == id && hit.text_edit.is_some() && hit.text.is_some())
            {
                return None;
            }
            Some(Composition {
                id: id.clone(),
                value,
                begin,
                end,
            })
        });
        let changed = self
            .composition
            .as_ref()
            .map(|c| (&c.id, &c.value, c.begin, c.end))
            != next.as_ref().map(|c| (&c.id, &c.value, c.begin, c.end));
        self.composition = next;
        changed
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn paint<M: Clone>(
    scene: &mut Scene<M>,
    element: &Element<M>,
    hit: &TextHit,
    clip: Clip,
    alpha: f32,
    focused: bool,
    font: &text::Font,
    interaction: &Interaction<M>,
    id: &str,
) {
    let Some(composition) = interaction
        .composition
        .as_ref()
        .filter(|c| c.id == id && focused)
    else {
        if hit.is_multiline() {
            text_area::paint(scene, element, hit, clip, alpha, focused, font)
        } else {
            text_input::paint(scene, element, hit, clip, alpha, focused, font)
        }
        return;
    };
    let mut display = hit.value.to_string();
    let range = hit.selection.range(&display);
    display.replace_range(range.clone(), &composition.value);
    let offset = |at: i32| -> usize {
        let at = usize::try_from(at)
            .unwrap_or(composition.value.len())
            .min(composition.value.len());
        TextSelection::boundaries(&composition.value)
            .take_while(|b| *b <= at)
            .last()
            .unwrap_or(0)
            + range.start
    };
    let mut selection = TextSelection::default();
    let _ = selection.apply(
        &mut display,
        TextEdit::Select {
            anchor: offset(composition.begin),
            caret: offset(composition.end),
        },
        usize::MAX,
    );
    let mut projected = element.clone();
    let projected_hit = match &mut projected.kind {
        Kind::TextInput {
            value,
            selection: current,
            ..
        } => {
            *value = display;
            *current = selection;
            text_input::geometry(
                font,
                value,
                *current,
                element.style.font_size,
                hit.inner,
                element.style.caret_width,
                true,
            )
        }
        Kind::TextArea {
            value,
            selection: current,
            scroll,
            ..
        } => {
            *value = display;
            *current = selection;
            text_area::geometry(
                font,
                value,
                *current,
                element.style.font_size,
                hit.inner,
                *scroll,
                id,
                None,
            )
        }
        _ => return,
    };
    if projected_hit.is_multiline() {
        text_area::paint(
            scene,
            &projected,
            &projected_hit,
            clip.clone(),
            alpha,
            focused,
            font,
        )
    } else {
        text_input::paint(
            scene,
            &projected,
            &projected_hit,
            clip.clone(),
            alpha,
            focused,
            font,
        )
    }
    let preedit = range.start..range.start + composition.value.len();
    let mut underline = |a: f32, b: f32, y: f32, height: f32| {
        scene.paint.push(Paint::Shape {
            rect: Rect::new(
                a,
                y + height - element.style.caret_width,
                b - a,
                element.style.caret_width,
            ),
            clip: clip.clone(),
            color: element
                .style
                .foreground
                .alpha(element.style.foreground.3 * alpha),
            radius: 0.,
        })
    };
    if let Some(area) = &projected_hit.area {
        for (index, row) in area.rows.iter().enumerate() {
            for (r, a, b) in row.segments.iter() {
                if r.start < preedit.end && r.end > preedit.start {
                    underline(
                        hit.inner.x + a.min(*b),
                        hit.inner.x + a.max(*b),
                        hit.inner.y + index as f32 * area.line_height - hit.scroll,
                        area.line_height,
                    );
                }
            }
        }
    } else {
        let shape = text::shape(font, &display_of(&projected), element.style.font_size);
        for (a, b) in shape.selection(preedit) {
            underline(
                hit.inner.x + a - projected_hit.scroll,
                hit.inner.x + b - projected_hit.scroll,
                hit.inner.y,
                hit.inner.h,
            );
        }
    }
}
fn display_of<M>(element: &Element<M>) -> String {
    match &element.kind {
        Kind::TextInput { value, .. } | Kind::TextArea { value, .. } => value.clone(),
        _ => String::new(),
    }
}
