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
    let projected =
        project(element, hit, font, interaction, id).expect("live composition projection");
    let (projected, projected_hit) = projected;
    let range = hit.selection.range(&hit.value);
    let show_cursor = focused && !(composition.begin < 0 && composition.end < 0);
    if projected_hit.is_multiline() {
        text_area::paint(
            scene,
            &projected,
            &projected_hit,
            clip.clone(),
            alpha,
            show_cursor,
            font,
        )
    } else {
        text_input::paint(
            scene,
            &projected,
            &projected_hit,
            clip.clone(),
            alpha,
            show_cursor,
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
pub(super) fn project<M: Clone>(
    element: &Element<M>,
    hit: &TextHit,
    font: &text::Font,
    interaction: &Interaction<M>,
    id: &str,
) -> Option<(Element<M>, TextHit)> {
    let composition = interaction
        .composition
        .as_ref()
        .filter(|composition| composition.id == id && interaction.focus.as_deref() == Some(id))?;
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
                id,
                None,
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
                element.style.caret_width,
            )
        }
        _ => return None,
    };
    Some((projected, projected_hit))
}

fn display_of<M>(element: &Element<M>) -> String {
    match &element.kind {
        Kind::TextInput { value, .. } | Kind::TextArea { value, .. } => value.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn layout() -> Layout {
        Layout::new(Arc::new(vec![
            text::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf").as_slice(),
                Default::default(),
            )
            .unwrap(),
        ]))
    }
    #[test]
    fn preedit_is_projected_and_canonical_hit_and_client_value_stay_unchanged() {
        let value = "a日b".to_string();
        let mut selection = TextSelection::default();
        let mut owned = value.clone();
        selection
            .apply(
                &mut owned,
                TextEdit::Select {
                    anchor: 1,
                    caret: 4,
                },
                32,
            )
            .unwrap();
        let tree = Element::text_input(&value, selection, "", |edit| edit)
            .id("field")
            .autofocus()
            .size(240., 40.)
            .font(20.);
        let mut input = Interaction::default();
        let layout = layout();
        let scene = layout.build(&tree, 240., 40., &input, 0.);
        input.synchronize(&scene);
        assert!(input.set_preedit(&scene, Some(("語".into(), 3, 3))));
        let projected = layout.build(&tree, 240., 40., &input, 0.);
        assert_eq!(value, "a日b");
        assert!(
            projected.hits[0].value.is_empty(),
            "ordinary controls do not duplicate canonical text in legacy buffers"
        );
        assert_eq!(
            projected.hits[0].text.as_ref().unwrap().value.as_ref(),
            value
        );
        assert!(
            projected
                .paint
                .iter()
                .any(|paint| matches!(paint,Paint::Text {text,..} if text=="a語b"))
        );
        assert!(
            projected.hits[0]
                .text
                .as_ref()
                .unwrap()
                .composition_cursor
                .is_some()
        );
        assert!(input.set_preedit(&projected, None));
    }
    #[test]
    fn masked_and_unfocused_controls_never_accept_composition() {
        let layout = layout();
        let mut input = Interaction::default();
        let tree = Element::masked_input(7, "", |_edit| TextEdit::Clear)
            .id("secret")
            .autofocus()
            .size(240., 40.);
        let scene = layout.build(&tree, 240., 40., &input, 0.);
        input.synchronize(&scene);
        assert!(!input.set_preedit(&scene, Some(("secret".into(), 6, 6))));
        assert!(input.composition.is_none());
        assert!(
            scene
                .hits
                .iter()
                .all(|hit| hit.text.is_none() && hit.value.is_empty())
        );
    }
    #[test]
    fn composition_retires_when_focus_moves_or_control_disappears() {
        let layout = layout();
        let tree = Element::text_input("abc", TextSelection::default(), "", |edit| edit)
            .id("field")
            .autofocus()
            .size(240., 40.);
        let mut input = Interaction::default();
        let scene = layout.build(&tree, 240., 40., &input, 0.);
        input.synchronize(&scene);
        input.set_preedit(&scene, Some(("日".into(), 3, 3)));
        input.focus = Some("other".into());
        input.synchronize(&scene);
        assert!(input.composition.is_none());
        input.focus = Some("field".into());
        input.set_preedit(&scene, Some(("日".into(), 3, 3)));
        input.synchronize(&Scene::default());
        assert!(input.composition.is_none());
    }
    #[test]
    fn multiline_preedit_uses_wrapped_geometry_and_hides_requested_cursor() {
        let layout = layout();
        let tree = Element::text_area(
            "ab",
            TextSelection::at_end("ab"),
            "",
            ScrollSpec::new(0., 20.).unwrap(),
            |edit| edit,
            |_| TextEdit::Clear,
        )
        .id("area")
        .autofocus()
        .size(80., 100.)
        .font(20.);
        let mut input = Interaction::default();
        let scene = layout.build(&tree, 80., 100., &input, 0.);
        input.synchronize(&scene);
        input.set_preedit(&scene, Some(("日本語\n語".into(), -1, -1)));
        let projected = layout.build(&tree, 80., 100., &input, 0.);
        let text = projected.hits[0].text.as_ref().unwrap();
        assert_eq!(text.value.as_ref(), "ab");
        assert!(text.caret_rect().y > text.inner.y);
        assert!(
            projected
                .paint
                .iter()
                .any(|paint| matches!(paint,Paint::Text{text,..} if text.contains('日')))
        );
    }
}
