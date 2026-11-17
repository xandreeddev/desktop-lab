//! Physical intent follows the prepared hit, never a retained focus or re-hit.
use crate::{Interaction, Scene};
use lucent_api::{KeyModifiers, MouseButton, PointerPress};

impl<M: Clone> Interaction<M> {
    /// Inspect the actual captured primary target immediately after a physical
    /// primary press. Runners pair this with their current native input origin;
    /// this getter alone grants no user-gesture authority.
    pub fn pressed_keyboard_focusable(&self) -> bool {
        self.press.as_ref().is_some_and(|press| press.hit.tab_stop)
    }

    /// Return the nearest subtree observer followed by the pressed control's
    /// ordinary message. Runners dispatch in this order, retaining their native
    /// lifetime guard between messages. Existing press APIs remain unchanged.
    pub fn press_button_messages(
        &mut self,
        scene: &Scene<M>,
        x: f32,
        y: f32,
        button: MouseButton,
        modifiers: KeyModifiers,
    ) -> [Option<M>; 2] {
        // Dismissal consumes this press; an underlying subtree did not receive
        // it and must not acquire an interaction merely because it has a hit.
        if let Some(dismiss) = self.popover_outside_press(scene, x, y) {
            return [None, Some(dismiss)];
        }
        let observation = scene
            .hits
            .iter()
            .rev()
            .find(|hit| hit.contains(x, y))
            .and_then(|hit| {
                hit.pointer_press.as_ref().map(|observe| {
                    observe(PointerPress {
                        button,
                        keyboard_focusable: hit.tab_stop,
                    })
                })
            });
        let action = self.press_button(scene, x, y, button, modifiers);
        [observation, action]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Layout;
    use lucent_api::{AnchoredSpec, Element, Key, OverlayAlign, OverlaySide, Popover};
    use std::sync::Arc;

    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Observed(&'static str, PointerPress),
        Action,
        Dismiss,
    }
    fn observer(owner: &'static str) -> impl Fn(PointerPress) -> Message {
        move |press| Message::Observed(owner, press)
    }
    fn scene(tree: Element<Message>, input: &Interaction<Message>) -> Scene<Message> {
        Layout::new(Arc::new(vec![])).build(&tree, 200., 100., input, 0.)
    }
    fn press(
        input: &mut Interaction<Message>,
        scene: &Scene<Message>,
        x: f32,
        y: f32,
    ) -> Vec<Message> {
        input
            .press_button_messages(scene, x, y, MouseButton::Primary, KeyModifiers::default())
            .into_iter()
            .flatten()
            .collect()
    }
    #[test]
    fn nearest_press_observer_uses_pressed_hit_instead_of_previous_keyboard_focus() {
        let mut input = Interaction::default();
        let tree = Element::stack(vec![
            Element::empty()
                .size(80., 80.)
                .on_click(Message::Action)
                .id("media-control")
                .on_pointer_press(observer("media")),
            Element::empty()
                .size(80., 80.)
                .at(100., 0.)
                .on_drag(|_| Message::Action)
                .id("clock-drag")
                .on_pointer_press(observer("clock")),
        ]);
        let current = scene(tree, &input);
        press(&mut input, &current, 30., 30.);
        assert_eq!(input.focus.as_deref(), Some("media-control"));
        assert!(input.pressed_keyboard_focusable());
        let messages = press(&mut input, &current, 130., 30.);
        assert_eq!(
            messages.first(),
            Some(&Message::Observed(
                "clock",
                PointerPress {
                    button: MouseButton::Primary,
                    keyboard_focusable: false
                }
            ))
        );
        assert!(!input.pressed_keyboard_focusable());
        // A passive press deliberately retains component focus, but that is
        // not evidence that Media was pressed or requested keyboard ownership.
        assert_eq!(input.focus.as_deref(), Some("media-control"));
    }
    #[test]
    fn nested_press_observer_runs_once_and_never_expands_input_regions() {
        let mut input = Interaction::default();
        let tree = Element::stack(vec![
            Element::empty()
                .size(40., 40.)
                .on_click(Message::Action)
                .on_pointer_press(observer("inner")),
        ])
        .on_pointer_press(observer("outer"));
        let current = scene(tree, &input);
        assert_eq!(current.hits.len(), 1);
        assert_eq!(press(&mut input, &current, 90., 70.), vec![]);
        assert_eq!(press(&mut input, &current, 20., 20.).len(), 1);
        assert!(matches!(
            press(&mut input, &current, 20., 20.).as_slice(),
            [Message::Observed(
                "inner",
                PointerPress {
                    keyboard_focusable: true,
                    ..
                }
            )]
        ));
    }
    #[test]
    fn covered_target_and_rounded_clips_share_ordinary_press_selection() {
        let mut input = Interaction::default();
        let tree = Element::stack(vec![
            Element::empty()
                .size(80., 80.)
                .on_click(Message::Action)
                .on_pointer_press(observer("under")),
            Element::empty()
                .size(80., 80.)
                .on_drag(|_| Message::Action)
                .radius(30.)
                .on_pointer_press(observer("top")),
        ])
        .size(80., 80.)
        .radius(30.)
        .clip();
        let current = scene(Element::stack(vec![tree]), &input);
        assert_eq!(press(&mut input, &current, 1., 1.), vec![]);
        assert!(matches!(
            press(&mut input, &current, 40., 40.).first(),
            Some(Message::Observed("top", _))
        ));
    }
    #[test]
    fn press_observer_precedes_text_selection_action() {
        let font = crate::text::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .unwrap();
        let mut input = Interaction::default();
        let field = Element::text_input("", Default::default(), "", |_| Message::Action)
            .on_pointer_press(observer("field"));
        let current = Layout::new(Arc::new(vec![font])).build(&field, 200., 100., &input, 0.);
        assert!(matches!(
            press(&mut input, &current, 30., 30.).as_slice(),
            [Message::Observed("field", _), Message::Action]
        ));
    }
    #[test]
    fn keyboard_activation_does_not_invent_a_physical_press() {
        let mut input = Interaction::default();
        let current = scene(
            Element::empty()
                .on_click(Message::Action)
                .id("button")
                .on_pointer_press(observer("button")),
            &input,
        );
        input.focus = Some("button".into());
        assert_eq!(input.key(&current, &Key::Enter), vec![Message::Action]);
    }
    #[test]
    fn mapped_press_observer_preserves_typed_owner_and_button() {
        let mut input = Interaction::default();
        let tree = Element::empty()
            .on_click(Message::Action)
            .on_pointer_press(observer("mapped"))
            .map(|message| (7, message));
        let current = Layout::new(Arc::new(vec![])).build(&tree, 200., 100., &input, 0.);
        assert_eq!(
            input.press_button_messages(
                &current,
                30.,
                30.,
                MouseButton::Middle,
                KeyModifiers::default(),
            ),
            [
                Some((
                    7,
                    Message::Observed(
                        "mapped",
                        PointerPress {
                            button: MouseButton::Middle,
                            keyboard_focusable: true,
                        }
                    )
                )),
                None
            ]
        );
    }
    #[test]
    fn popover_outside_dismissal_does_not_arm_underlying_observer() {
        let mut input = Interaction::default();
        let tree = Element::stack(vec![
            Element::empty().size(40., 30.).id("anchor"),
            Element::empty()
                .size(40., 40.)
                .at(140., 50.)
                .on_click(Message::Action)
                .on_pointer_press(observer("under")),
        ])
        .popover(Popover {
            id: "test".into(),
            anchor: "anchor".into(),
            content: Box::new(Element::empty().size(40., 30.).on_click(Message::Action)),
            placement: AnchoredSpec {
                side: OverlaySide::Below,
                align: OverlayAlign::Start,
                gap: 4.,
                inset: 4.,
            },
            dismiss: Message::Dismiss,
        });
        let current = scene(tree, &input);
        assert_eq!(
            press(&mut input, &current, 160., 70.),
            vec![Message::Dismiss]
        );
    }
}
