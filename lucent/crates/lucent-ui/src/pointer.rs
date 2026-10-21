//! Button capture uses the same prepared, clipped hit geometry as other input.
use crate::{Hit, Interaction, Scene};
use lucent_api::{KeyModifiers, MouseButton};

pub(super) struct AlternatePress<M> {
    button: MouseButton,
    hit: Hit<M>,
    origin: (f32, f32),
    moved: bool,
}

fn action<M: Clone>(hit: &Hit<M>, button: MouseButton) -> Option<M> {
    match button {
        MouseButton::Secondary => hit.secondary_click.clone(),
        MouseButton::Middle => hit.middle_click.clone(),
        _ => None,
    }
}

fn plain_primary_action<M>(hit: &Hit<M>) -> bool {
    hit.click.is_some()
        && hit.drag.is_none()
        && hit.slider.is_none()
        && hit.scroll.is_none()
        && hit.input.is_none()
        && hit.edit.is_none()
        && hit.text_edit.is_none()
}

impl<M: Clone> Interaction<M> {
    pub fn press_button(
        &mut self,
        scene: &Scene<M>,
        x: f32,
        y: f32,
        button: MouseButton,
        modifiers: KeyModifiers,
    ) -> Option<M> {
        if let Some(message) = self.popover_outside_press(scene, x, y) {
            return Some(message);
        }
        if button == MouseButton::Primary {
            return self.press_with_modifiers(scene, x, y, modifiers);
        }
        // A covered sibling is never an alternate target if the topmost hit
        // has no handler. A second button does not replace primary drag capture.
        self.alternate_press = scene
            .hits
            .iter()
            .rev()
            .find(|hit| hit.contains(x, y))
            .filter(|hit| action(hit, button).is_some())
            .map(|hit| AlternatePress {
                button,
                hit: hit.clone(),
                origin: (x, y),
                moved: false,
            });
        if let Some(press) = &self.alternate_press {
            self.focus_visible = false;
            if press.hit.tab_stop {
                self.focus = Some(press.hit.id.clone());
            }
        }
        None
    }

    pub fn release_button(&mut self, x: f32, y: f32, button: MouseButton) -> Option<M> {
        if button == MouseButton::Primary {
            return self.release(x, y);
        }
        self.move_buttons(x, y);
        if self.alternate_press.as_ref()?.button != button {
            return None;
        }
        let press = self.alternate_press.take()?;
        (!press.moved && press.hit.contains(x, y))
            .then(|| action(&press.hit, button))
            .flatten()
    }

    pub(super) fn move_buttons(&mut self, x: f32, y: f32) {
        if let Some(press) = &mut self.alternate_press {
            press.moved |= (x - press.origin.0).hypot(y - press.origin.1) >= 5.;
        }
    }

    pub(super) fn synchronize_buttons(&mut self, scene: &Scene<M>) {
        // A plain primary action must not retain a removed control or an old
        // callback across synchronous client updates before the next paint.
        // Text selection, dragging and value controls keep their own capture
        // policies in Interaction::synchronize. An unchanged ID/geometry cannot
        // convert a plain press into a different gesture role; that requires a
        // new press. This adds no native input authority.
        if let Some(press) = self.press.as_mut().filter(|press| {
            plain_primary_action(&press.hit)
                && press.text_anchor.is_none()
                && press.scroll_grab.is_none()
        }) {
            let current = scene.hits.iter().find(|hit| {
                hit.id == press.hit.id
                    && hit.rect == press.hit.rect
                    && hit.clip == press.hit.clip
                    && hit.radius == press.hit.radius
                    && hit.parts == press.hit.parts
                    && plain_primary_action(hit)
            });
            if let Some(hit) = current {
                press.hit = hit.clone();
            } else {
                self.press = None;
            }
        }
        let Some(press) = &mut self.alternate_press else {
            return;
        };
        let current = scene.hits.iter().find(|hit| {
            hit.id == press.hit.id
                && hit.rect == press.hit.rect
                && hit.clip == press.hit.clip
                && hit.radius == press.hit.radius
                && hit.parts == press.hit.parts
                && action(hit, press.button).is_some()
        });
        if let Some(hit) = current {
            press.hit = hit.clone();
        } else {
            self.alternate_press = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Layout;
    use lucent_api::{Element, Key, ScrollSpec, WheelEvent};
    use std::sync::Arc;

    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Primary,
        Secondary,
        Middle,
        Covered,
        Wheel(WheelEvent),
        Scroll(f32),
    }
    fn control() -> Element<Message> {
        Element::empty()
            .size(80., 60.)
            .radius(16.)
            .id("control")
            .on_click(Message::Primary)
            .on_secondary_click(Message::Secondary)
            .on_middle_click(Message::Middle)
            .on_wheel(Message::Wheel)
    }
    fn scene(tree: Element<Message>, input: &Interaction<Message>) -> Scene<Message> {
        // A root always receives the surface extent. Put the fixture beneath it
        // so fixed dimensions and rounded ancestor clipping are actually tested.
        Layout::new(Arc::new(vec![])).build(&Element::stack(vec![tree]), 100., 100., input, 0.)
    }
    fn press(
        input: &mut Interaction<Message>,
        current: &Scene<Message>,
        x: f32,
        y: f32,
        button: MouseButton,
    ) {
        assert!(
            input
                .press_button(current, x, y, button, KeyModifiers::default())
                .is_none()
        );
    }
    #[test]
    fn alternate_buttons_share_rounded_clips_and_do_not_pass_through_topmost_hits() {
        let mut input = Interaction::default();
        let current = scene(
            Element::stack(vec![control()])
                .size(60., 60.)
                .radius(24.)
                .clip(),
            &input,
        );
        for button in [MouseButton::Secondary, MouseButton::Middle] {
            for (x, y) in [(1., 1.), (68., 30.)] {
                press(&mut input, &current, x, y, button);
                assert_eq!(input.release_button(x, y, button), None);
            }
            press(&mut input, &current, 30., 30., button);
            assert_eq!(
                input.release_button(30., 30., button),
                Some(if button == MouseButton::Secondary {
                    Message::Secondary
                } else {
                    Message::Middle
                })
            );
        }
        let covered = scene(
            Element::stack(vec![
                control(),
                Element::empty()
                    .size(80., 60.)
                    .id("overlay")
                    .on_click(Message::Covered),
            ]),
            &input,
        );
        press(&mut input, &covered, 30., 30., MouseButton::Secondary);
        assert_eq!(input.release_button(30., 30., MouseButton::Secondary), None);
        assert_eq!(
            input.wheel(
                &covered,
                30.,
                30.,
                WheelEvent {
                    vertical: 1.,
                    horizontal: 0.
                }
            ),
            None
        );
    }
    #[test]
    fn alternate_capture_matches_button_and_keeps_primary_capture_independent() {
        let mut input = Interaction::default();
        let current = scene(control(), &input);
        press(&mut input, &current, 30., 30., MouseButton::Primary);
        press(&mut input, &current, 30., 30., MouseButton::Secondary);
        assert_eq!(input.release_button(30., 30., MouseButton::Middle), None);
        assert_eq!(
            input.release_button(30., 30., MouseButton::Primary),
            Some(Message::Primary)
        );
        assert_eq!(
            input.release_button(30., 30., MouseButton::Secondary),
            Some(Message::Secondary)
        );
        assert_eq!(input.release_button(30., 30., MouseButton::Secondary), None);
        press(&mut input, &current, 30., 30., MouseButton::Middle);
        assert_eq!(input.release_button(40., 30., MouseButton::Middle), None);
    }
    #[test]
    fn alternate_capture_retires_on_drag_leave_removed_handler_or_geometry_change() {
        for retired in 0..4 {
            let mut input = Interaction::default();
            let current = scene(control(), &input);
            press(&mut input, &current, 30., 30., MouseButton::Secondary);
            match retired {
                0 => {
                    input.motion(&current, 40., 30., 0.);
                    input.motion(&current, 30., 30., 0.);
                }
                1 => input.leave(0.),
                2 => {
                    let mut tree = control();
                    tree.secondary_click = None;
                    input.synchronize(&scene(tree, &input));
                }
                _ => input.synchronize(&scene(control().size(72., 60.), &input)),
            }
            assert_eq!(input.release_button(30., 30., MouseButton::Secondary), None);
        }
    }
    #[test]
    fn held_plain_primary_does_not_become_a_new_drag_capture() {
        let mut input = Interaction::default();
        let previous = scene(control(), &input);
        press(&mut input, &previous, 30., 30., MouseButton::Primary);
        let current = scene(control().on_drag(|_| Message::Covered), &input);
        input.synchronize(&current);
        assert!(input.motion(&current, 50., 30., 0.).is_empty());
        assert_eq!(input.release_button(50., 30., MouseButton::Primary), None);

        // Changing role retires only the old press, not the new control.
        press(&mut input, &current, 30., 30., MouseButton::Primary);
        assert_eq!(input.motion(&current, 50., 30., 0.), vec![Message::Covered]);
        assert_eq!(
            input.release_button(50., 30., MouseButton::Primary),
            Some(Message::Covered)
        );
    }
    #[test]
    fn held_plain_primary_does_not_become_a_new_slider_capture() {
        let mut input = Interaction::default();
        let previous = scene(control(), &input);
        press(&mut input, &previous, 30., 30., MouseButton::Primary);
        let current = scene(
            Element::slider(lucent_api::SliderSpec::new(0.25, 0.01).unwrap(), |_| {
                Message::Covered
            })
            .size(80., 60.)
            .radius(16.)
            .id("control")
            .on_click(Message::Primary),
            &input,
        );
        input.synchronize(&current);
        assert!(input.motion(&current, 60., 30., 0.).is_empty());
        assert_eq!(input.release_button(60., 30., MouseButton::Primary), None);

        assert_eq!(
            input.press_button(
                &current,
                30.,
                30.,
                MouseButton::Primary,
                KeyModifiers::default(),
            ),
            Some(Message::Covered)
        );
        assert_eq!(
            input.release_button(30., 30., MouseButton::Primary),
            Some(Message::Covered)
        );
    }
    #[test]
    fn focused_context_key_uses_secondary_handler_without_changing_primary_enter() {
        let mut input = Interaction::default();
        let current = scene(control(), &input);
        assert!(input.key(&current, &Key::Tab).is_empty());
        assert_eq!(input.focus.as_deref(), Some("control"));
        assert_eq!(
            input.key(&current, &Key::ContextMenu),
            vec![Message::Secondary]
        );
        assert_eq!(input.key(&current, &Key::Enter), vec![Message::Primary]);
    }
    #[test]
    fn leaf_wheel_retains_both_axes_and_consumes_before_ancestor_scroll() {
        let mut input = Interaction::default();
        let tree = Element::scroll(
            ScrollSpec::new(0., 20.).unwrap(),
            Element::stack(vec![control()]).size(80., 200.),
            |event| Message::Scroll(event.offset),
        )
        .size(100., 80.)
        .id("scroll");
        let current = scene(tree, &input);
        let event = WheelEvent {
            horizontal: -0.25,
            vertical: 0.5,
        };
        assert_eq!(
            input.wheel(&current, 30., 30., event),
            Some(vec![Message::Wheel(event)])
        );
        assert_eq!(
            input.wheel(&current, 30., 30., WheelEvent::default()),
            Some(vec![])
        );
        assert_eq!(
            input.wheel(
                &current,
                30.,
                30.,
                WheelEvent {
                    horizontal: f64::NAN,
                    vertical: 1.
                }
            ),
            Some(vec![])
        );
        assert_eq!(
            input.scroll(&current, 30., 70., 1.),
            Some(vec![Message::Scroll(20.)])
        );
    }
}
