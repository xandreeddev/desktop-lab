//! Native keyboard ownership gates paint without discarding logical selection.
use crate::Interaction;

impl<M> Interaction<M> {
    /// Runners set actual input-context activity independently from the logical
    /// component target and keyboard-versus-pointer focus-decoration modality.
    /// Returns whether paint needs invalidation. This never requests native
    /// focus, changes selection, resets scroll or cancels pointer capture.
    pub fn set_keyboard_active(&mut self, active: bool) -> bool {
        let changed = self.keyboard_active != active;
        self.keyboard_active = active;
        changed
    }
    pub fn keyboard_active(&self) -> bool {
        self.keyboard_active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Layout, Paint, Scene};
    use lucent_api::{Element, ScrollSpec, TextEdit, TextSelection};
    use std::sync::Arc;

    fn layout() -> Layout {
        let font = crate::text::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .unwrap();
        Layout::new(Arc::new(vec![font]))
    }
    fn decorations(scene: &Scene<()>) -> (usize, usize) {
        (
            scene
                .paint
                .iter()
                .filter(|paint| matches!(paint, Paint::Shape { .. }))
                .count(),
            scene
                .paint
                .iter()
                .filter(|paint| matches!(paint, Paint::Outline { .. }))
                .count(),
        )
    }
    #[test]
    fn native_leave_hides_caret_and_outline_but_retains_logical_selection_and_scroll() {
        let layout = layout();
        let mut value = "A deliberately long field with a visible caret at its end".to_string();
        let mut selection = TextSelection::default();
        let caret = value.len();
        selection
            .apply(&mut value, TextEdit::Select { anchor: 1, caret }, 1024)
            .unwrap();
        let field = Element::text_input(value.clone(), selection, "", |_| ())
            .id("field")
            .focus_outline(true)
            .padding(8.);
        let mut interaction = Interaction {
            focus: Some("field".into()),
            focus_visible: true,
            ..Default::default()
        };
        let active = layout.build(&field, 140., 48., &interaction, 0.);
        assert!(decorations(&active).0 >= 2, "selection and caret paint");
        assert_eq!(decorations(&active).1, 1);
        let text = active.hits[0].text.as_ref().unwrap();
        let scroll = text.scroll;
        assert!(scroll > 0.);
        assert!(interaction.set_keyboard_active(false));
        assert!(!interaction.set_keyboard_active(false));
        let inactive = layout.build(&field, 140., 48., &interaction, 0.);
        assert_eq!(decorations(&inactive), (0, 0));
        assert_eq!(interaction.focus.as_deref(), Some("field"));
        assert!(interaction.focus_visible, "decoration modality is retained");
        assert_eq!(inactive.hits[0].text.as_ref().unwrap().selection, selection);
        assert_eq!(inactive.hits[0].text.as_ref().unwrap().scroll, scroll);
        assert!(interaction.set_keyboard_active(true));
        let reacquired = layout.build(&field, 140., 48., &interaction, 0.);
        assert_eq!(decorations(&reacquired), decorations(&active));
        assert_eq!(
            reacquired.hits[0].text.as_ref().unwrap().selection,
            selection
        );
        assert_eq!(reacquired.hits[0].text.as_ref().unwrap().scroll, scroll);
    }
    #[test]
    fn multiline_caret_is_inactive_without_losing_ordinary_editor_hit_geometry() {
        let layout = layout();
        let field = Element::text_area(
            "notes",
            TextSelection::at_end("notes"),
            "",
            ScrollSpec::new(0., 20.).unwrap(),
            |_| (),
            |_| (),
        )
        .id("notes")
        .focus_outline(true)
        .padding(8.);
        let mut interaction = Interaction {
            focus: Some("notes".into()),
            focus_visible: true,
            ..Default::default()
        };
        let active = layout.build(&field, 200., 100., &interaction, 0.);
        assert_eq!(decorations(&active), (1, 1));
        interaction.set_keyboard_active(false);
        let inactive = layout.build(&field, 200., 100., &interaction, 0.);
        assert_eq!(decorations(&inactive), (0, 0));
        assert_eq!(inactive.hits.len(), active.hits.len());
        assert_eq!(inactive.hits[0].rect, active.hits[0].rect);
        assert_eq!(inactive.hits[0].text.as_ref().unwrap().selection.caret(), 5);
        assert!(!interaction.animating(0.));
        assert_eq!(interaction.next_wakeup(&inactive, 0.), None);
    }
    #[test]
    fn inactive_composition_has_no_caret_or_underlines_and_creates_no_idle_deadline() {
        let layout = layout();
        let field =
            Element::text_input("abc", TextSelection::at_end("abc"), "", |_| ()).id("field");
        let mut interaction = Interaction {
            focus: Some("field".into()),
            ..Default::default()
        };
        let prepared = layout.build(&field, 200., 48., &interaction, 0.);
        assert!(interaction.set_preedit(&prepared, Some(("fixture".into(), 0, 7))));
        let active = layout.build(&field, 200., 48., &interaction, 0.);
        assert!(decorations(&active).0 > 0);
        interaction.set_keyboard_active(false);
        let inactive = layout.build(&field, 200., 48., &interaction, 0.);
        assert_eq!(decorations(&inactive), (0, 0));
        assert_eq!(interaction.focus.as_deref(), Some("field"));
        assert!(!interaction.animating(0.));
        assert_eq!(interaction.next_wakeup(&inactive, 0.), None);
    }
    #[test]
    fn offscreen_default_keeps_existing_active_focus_fixtures_explicit() {
        let interaction = Interaction::<()>::default();
        assert!(interaction.keyboard_active());
    }
}
