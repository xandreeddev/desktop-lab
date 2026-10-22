use super::*;
use lucent_ui::{Interaction, Layout, text::Font};
use std::sync::Arc;

fn lab(story: &str, state: Option<&str>) -> Lab {
    Lab::new(story, "dark", state, Viewport::Regular).unwrap()
}
fn layout() -> Layout {
    Layout::new(Arc::new(
        FONT_BYTES
            .iter()
            .map(|bytes| Font::from_bytes(*bytes, Default::default()).unwrap())
            .collect(),
    ))
}

#[test]
fn story_change_retires_pending_edit_and_menu_messages_and_bounds_event_log() {
    let mut lab = lab("input", None);
    let old = lab.generation;
    let mut effects = Effects::default();
    lab.update(Message::Story(7), &mut effects);
    let value = lab.text.clone();
    lab.update(
        Message::Edit(old, TextEdit::Replace("stale".into())),
        &mut effects,
    );
    lab.update(
        Message::Menu(old, lucent_menu::Message::Cancel),
        &mut effects,
    );
    assert_eq!(lab.text, value);
    assert!(lab.completion.outcome().is_none());
    for _ in 0..10 {
        lab.update(Message::Action("Action"), &mut effects);
    }
    assert_eq!(lab.log.len(), 3);
    assert!(!effects.exit);
}

#[test]
fn actual_menu_completion_is_captured_and_clipboard_target_and_redraw_are_lifted() {
    let mut lab = lab("menu", None);
    let mut effects = Effects::default();
    lab.update(
        Message::Menu(lab.generation, lucent_menu::Message::Edit(TextEdit::Paste)),
        &mut effects,
    );
    assert_eq!(effects.clipboard.len(), 1);
    assert_eq!(effects.clipboard[0].target.surface, SURFACE);
    assert_eq!(effects.clipboard[0].target.element, "menu-search");
    assert!(!effects.redraw.contains("menu"));
    assert!(effects.redraw.contains(SURFACE));
    lab.update(
        Message::Menu(lab.generation, lucent_menu::Message::Accept),
        &mut effects,
    );
    assert!(matches!(
        lab.completion.outcome(),
        Some(MenuOutcome::Accepted(_))
    ));
    assert!(!effects.exit);
    lab.update(Message::Reset, &mut effects);
    assert!(lab.completion.outcome().is_none());
    lab.update(
        Message::Menu(lab.generation, lucent_menu::Message::Cancel),
        &mut effects,
    );
    assert_eq!(lab.completion.outcome(), Some(&MenuOutcome::Cancelled));
    assert!(!effects.exit);
}

#[test]
fn actual_button_focus_traversal_skips_disabled_actions() {
    let lab = lab("buttons", None);
    let layout = layout();
    let cx = lab.preview_context(1., Density::ONE);
    let tree = lab.story_view(&cx);
    let mut input = Interaction::default();
    let scene = layout.build(&tree, cx.width, cx.height, &input, cx.now);
    assert!(scene.hits.iter().all(|hit| hit.id != "sample-disabled"));
    input.key(&scene, &Key::Tab);
    assert_eq!(input.focus.as_deref(), Some("sample-primary"));
    assert!(matches!(
        input.key(&scene, &Key::Enter).as_slice(),
        [Message::Action("Apply")]
    ));
    input.key(&scene, &Key::Tab);
    assert_eq!(input.focus.as_deref(), Some("sample-choice"));
    input.key(&scene, &Key::Tab);
    assert_ne!(input.focus.as_deref(), Some("sample-disabled"));
}

#[test]
fn popover_scope_uses_engine_focus_trap_and_opener_restoration() {
    let mut lab = lab("popover", Some("closed"));
    let layout = layout();
    let cx = lab.preview_context(1., Density::ONE);
    let mut input = Interaction::default();
    input.focus = Some("popover-trigger".into());
    input.focus_visible = true;
    let closed = layout.build(&lab.story_view(&cx), cx.width, cx.height, &input, cx.now);
    input.synchronize(&closed);
    lab.update(Message::TogglePopover, &mut Effects::default());
    let opened = layout.build(&lab.story_view(&cx), cx.width, cx.height, &input, cx.now);
    input.synchronize(&opened);
    assert_eq!(input.focus.as_deref(), Some("popover-first"));
    let dismiss = input.key(&opened, &Key::Escape);
    assert!(matches!(dismiss.as_slice(), [Message::ClosePopover]));
    for message in dismiss {
        lab.update(message, &mut Effects::default());
    }
    let closed = layout.build(&lab.story_view(&cx), cx.width, cx.height, &input, cx.now);
    input.synchronize(&closed);
    assert_eq!(input.focus.as_deref(), Some("popover-trigger"));
}

#[test]
fn all_palette_state_viewport_scenes_have_bounded_hits_and_live_menu_fields() {
    let layout = layout();
    for story in stories::STORIES {
        for state in story.states {
            for palette in palettes() {
                for viewport in Viewport::ALL {
                    let lab = Lab::new(story.id, &palette.id, Some(state), viewport).unwrap();
                    let cx = lab.preview_context(story.time_ms(state) as f64 / 1000., Density::ONE);
                    let scene = layout.build(
                        &lab.story_view(&cx),
                        cx.width,
                        cx.height,
                        &Interaction::default(),
                        cx.now,
                    );
                    assert!(!scene.paint.is_empty(), "{} {state}", story.id);
                    assert!(scene.hits.iter().all(|hit| hit.rect.x.is_finite()
                        && hit.rect.y.is_finite()
                        && hit.rect.w.is_finite()
                        && hit.rect.h.is_finite()));
                    if story.id == "menu" {
                        assert!(
                            scene
                                .hits
                                .iter()
                                .any(|hit| hit.id == "menu-search" && hit.text_edit.is_some())
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn animation_is_active_only_for_actual_menu_reveal_or_explicit_motion_replay() {
    let mut lab = lab("motion", Some("middle"));
    assert!(!lab.animating(SURFACE, 1.));
    let mut effects = Effects {
        now: 2.,
        ..Default::default()
    };
    lab.update(Message::Replay, &mut effects);
    assert!(lab.animating(SURFACE, 2.));
    assert!(!lab.animating(SURFACE, 2. + motion::PANEL + 1.));
    assert_eq!(lab.surfaces()[0].keyboard, Keyboard::OnDemand);
    assert_eq!(lab.surfaces()[0].pointer_input, PointerInput::Content);
    assert_eq!(lab.surfaces()[0].exclusive_zone, -1);
}

#[test]
fn hover_preset_runs_engine_transition_and_leaves_passive_stage_unchanged() {
    let lab = lab("buttons", Some("hover"));
    let layout = layout();
    let cx = lab.preview_context(1., Density::ONE);
    let tree = lab.story_view(&cx);
    let ordinary = layout.build(&tree, cx.width, cx.height, &Interaction::default(), 1.);
    let input = render::preset_interaction(&lab, &ordinary);
    assert_eq!(input.hovered.as_deref(), Some("sample-primary"));
    assert_eq!(input.hover_amount("sample-primary", 1.), 1.);
    let hovered = layout.build(&tree, cx.width, cx.height, &input, 1.);
    assert_ne!(
        format!("{:?}", ordinary.paint),
        format!("{:?}", hovered.paint)
    );
    assert_eq!(
        format!("{:?}", ordinary.paint[0]),
        format!("{:?}", hovered.paint[0])
    );
}

fn native_context() -> ViewContext {
    ViewContext {
        surface: SURFACE,
        instance: None,
        width: token::WIDTH,
        height: token::HEIGHT,
        now: 1.,
        density: Density::ONE,
    }
}

#[test]
fn menu_keyboard_session_preserves_chrome_then_enters_actual_menu_policy() {
    let mut lab = lab("menu", None);
    let layout = layout();
    let cx = native_context();
    for key in [Key::Tab, Key::BackTab, Key::Enter] {
        assert!(
            lab.event(Event::Key {
                surface: SURFACE,
                key
            })
            .is_none()
        );
    }
    let mut input = Interaction::default();
    input.focus = Some("lab-reset".into());
    let scene = layout.build(&lab.view(&cx), cx.width, cx.height, &input, cx.now);
    assert!(matches!(
        input.key(&scene, &Key::Enter).as_slice(),
        [Message::Reset]
    ));
    input.focus = Some("menu-keyboard-start".into());
    let messages = input.key(&scene, &Key::Enter);
    assert!(matches!(messages.as_slice(), [Message::MenuKeys]));
    for message in messages {
        lab.update(message, &mut Effects::default());
    }
    let active = layout.build(&lab.view(&cx), cx.width, cx.height, &input, cx.now);
    input.synchronize(&active);
    assert_eq!(input.focus.as_deref(), Some("menu-search"));
    for key in [Key::Down, Key::Tab] {
        let message = lab
            .event(Event::Key {
                surface: SURFACE,
                key,
            })
            .unwrap();
        assert!(matches!(
            &message,
            Message::Menu(_, lucent_menu::Message::Navigate(1))
        ));
        lab.update(message, &mut Effects::default());
    }
    assert_eq!(lab.menu.selection.selected, 2);
    assert!(
        lab.event(Event::Scroll {
            surface: SURFACE,
            lines: 1.
        })
        .is_none()
    );
    let escape = lab
        .event(Event::Key {
            surface: SURFACE,
            key: Key::Escape,
        })
        .unwrap();
    let mut effects = Effects::default();
    lab.update(escape, &mut effects);
    assert!(!lab.menu_keys);
    assert_eq!(lab.completion.outcome(), Some(&MenuOutcome::Cancelled));
    assert!(!effects.exit);
    assert!(
        lab.event(Event::Key {
            surface: SURFACE,
            key: Key::Enter
        })
        .is_none()
    );
}

#[test]
fn full_native_view_keeps_every_chrome_control_inside_real_surface_bounds() {
    let layout = layout();
    let cx = native_context();
    for story in stories::STORIES {
        for viewport in Viewport::ALL {
            let lab = Lab::new(story.id, "dark", None, viewport).unwrap();
            let scene = layout.build(
                &lab.view(&cx),
                cx.width,
                cx.height,
                &Interaction::default(),
                cx.now,
            );
            for hit in scene.hits.iter().filter(|hit| {
                [
                    "story-",
                    "palette-",
                    "viewport-",
                    "lab-reset",
                    "lab-close",
                    "preset-",
                    "menu-keyboard-start",
                ]
                .iter()
                .any(|prefix| hit.id.starts_with(prefix))
            }) {
                assert!(
                    hit.rect.x >= 0.
                        && hit.rect.y >= 0.
                        && hit.rect.x + hit.rect.w <= cx.width
                        && hit.rect.y + hit.rect.h <= cx.height,
                    "{} {} {:?}",
                    story.id,
                    hit.id,
                    hit.rect
                );
                assert!(
                    hit.contains(hit.rect.x + hit.rect.w / 2., hit.rect.y + hit.rect.h / 2.),
                    "{} {} is clipped",
                    story.id,
                    hit.id
                );
            }
            for required in ["story-motion", "lab-close", "lab-reset"] {
                assert!(
                    scene.hits.iter().any(|hit| hit.id == required),
                    "missing {required}"
                );
            }
        }
    }
}
