//! Native key symbols and modifiers end at this adapter; clients receive SDK keys.
use lucent_api::{Key, KeyModifiers};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym, Modifiers};

pub(super) fn translate(event: KeyEvent, native: Modifiers) -> Option<Key> {
    let modifiers = KeyModifiers {
        control: native.ctrl,
        alt: native.alt,
        shift: native.shift,
        logo: native.logo,
    };
    let control_shortcut = modifiers.control && !modifiers.alt && !modifiers.logo;
    let key = match event.keysym {
        Keysym::Escape => Key::Escape,
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::Menu => Key::ContextMenu,
        Keysym::F10
            if modifiers.shift && !modifiers.control && !modifiers.alt && !modifiers.logo =>
        {
            Key::ContextMenu
        }
        Keysym::BackSpace => Key::Backspace,
        Keysym::Delete | Keysym::KP_Delete => Key::Delete,
        Keysym::Up => Key::Up,
        Keysym::Down => Key::Down,
        Keysym::Left => Key::Left,
        Keysym::Right => Key::Right,
        Keysym::ISO_Left_Tab => Key::BackTab,
        Keysym::Tab if modifiers.shift => Key::BackTab,
        Keysym::Tab => Key::Tab,
        Keysym::Page_Up => Key::PageUp,
        Keysym::Page_Down => Key::PageDown,
        Keysym::Home => Key::Home,
        Keysym::End => Key::End,
        Keysym::a | Keysym::A if control_shortcut => Key::SelectAll,
        Keysym::u | Keysym::U if control_shortcut => Key::ClearInput,
        _ if modifiers.control || modifiers.alt || modifiers.logo => {
            let character = event.keysym.key_char().filter(|c| !c.is_control())?;
            Key::Shortcut {
                character: if modifiers.control {
                    character.to_ascii_lowercase()
                } else {
                    character
                },
                modifiers,
            }
        }
        _ => Key::Text(
            event
                .utf8
                .filter(|s| !s.is_empty() && s.chars().all(|c| !c.is_control()))?,
        ),
    };
    Some(
        if matches!(
            key,
            Key::Left | Key::Right | Key::Home | Key::End | Key::Delete | Key::Backspace
        ) && modifiers != KeyModifiers::default()
        {
            Key::Modified {
                key: Box::new(key),
                modifiers,
            }
        } else {
            key
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_api::{
        ClipboardAction, ClipboardOutcome, ClipboardTarget, Effects, Element, InputEdit, TextEdit,
        TextEditError, TextFieldState,
    };
    use lucent_ui::{Interaction, Layout};
    use std::sync::Arc;

    fn key(keysym: Keysym, utf8: Option<&str>, modifiers: Modifiers) -> Key {
        translate(
            KeyEvent {
                time: 0,
                raw_code: 0,
                keysym,
                utf8: utf8.map(str::to_owned),
            },
            modifiers,
        )
        .unwrap()
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
    fn apply(
        field: &mut TextFieldState,
        value: &mut String,
        messages: Vec<TextEdit>,
        effects: &mut Effects<TextEdit>,
    ) {
        for edit in messages {
            field
                .update(
                    value,
                    edit,
                    5,
                    ClipboardTarget::new("test", "field"),
                    effects,
                    |edit| edit,
                )
                .unwrap();
        }
    }
    #[test]
    fn uppercase_control_select_clear_and_history_work_before_a_new_frame() {
        let layout = layout();
        for (shift, caps_lock) in [(false, true), (true, false), (true, true)] {
            let modifiers = Modifiers {
                ctrl: true,
                shift,
                caps_lock,
                ..Default::default()
            };
            let mut value = "café".to_string();
            let mut field = TextFieldState::default();
            field.reset(&value);
            let tree = Element::text_input(&value, field.selection(), "", |edit| edit)
                .id("field")
                .autofocus()
                .size(180., 40.);
            let mut input = Interaction::default();
            let scene = layout.build(&tree, 180., 40., &input, 0.);
            input.synchronize(&scene);
            let mut effects = Effects::default();
            let selected = input.key(&scene, &key(Keysym::A, Some("\u{1}"), modifiers));
            assert_eq!(selected, [TextEdit::SelectAll]);
            apply(&mut field, &mut value, selected, &mut effects);
            assert_eq!(field.selection().selected(&value), "café");
            apply(
                &mut field,
                &mut value,
                input.key(&scene, &key(Keysym::e, Some("é"), Modifiers::default())),
                &mut effects,
            );
            assert_eq!(value, "é");
            let cleared = input.key(&scene, &key(Keysym::U, Some("\u{15}"), modifiers));
            assert_eq!(cleared, [TextEdit::Clear]);
            apply(&mut field, &mut value, cleared, &mut effects);
            assert!(value.is_empty());
            let control = Modifiers {
                ctrl: true,
                caps_lock: true,
                ..Default::default()
            };
            let undo = input.key(&scene, &key(Keysym::Z, None, control));
            assert_eq!(undo, [TextEdit::Undo]);
            apply(&mut field, &mut value, undo, &mut effects);
            assert_eq!(value, "é");
            let redo = input.key(
                &scene,
                &key(
                    Keysym::Z,
                    None,
                    Modifiers {
                        shift: true,
                        ..control
                    },
                ),
            );
            assert_eq!(redo, [TextEdit::Redo]);
            apply(&mut field, &mut value, redo, &mut effects);
            assert!(value.is_empty());
            assert!(effects.clipboard.is_empty());
        }
    }
    #[test]
    fn alt_and_logo_letters_do_not_edit_ordinary_or_masked_fields() {
        let layout = layout();
        let ordinary = Element::text_input("keep", Default::default(), "", |edit| edit)
            .id("field")
            .autofocus()
            .size(180., 40.);
        let masked = Element::masked_input(4, "", |edit| edit)
            .id("secret")
            .autofocus()
            .size(180., 40.);
        let mut ordinary_input = Interaction::default();
        let ordinary_scene = layout.build(&ordinary, 180., 40., &ordinary_input, 0.);
        ordinary_input.synchronize(&ordinary_scene);
        let mut masked_input = Interaction::default();
        let masked_scene = layout.build(&masked, 180., 40., &masked_input, 0.);
        masked_input.synchronize(&masked_scene);
        for modifiers in [
            Modifiers {
                ctrl: true,
                alt: true,
                ..Default::default()
            },
            Modifiers {
                ctrl: true,
                logo: true,
                ..Default::default()
            },
            Modifiers {
                alt: true,
                ..Default::default()
            },
            Modifiers {
                logo: true,
                ..Default::default()
            },
            Modifiers {
                ctrl: true,
                alt: true,
                logo: true,
                shift: true,
                ..Default::default()
            },
        ] {
            for keysym in [
                Keysym::a,
                Keysym::A,
                Keysym::u,
                Keysym::U,
                Keysym::z,
                Keysym::V,
            ] {
                let translated = key(keysym, Some("a"), modifiers);
                assert!(matches!(&translated, Key::Shortcut { modifiers: kept, .. }
                    if kept.alt == modifiers.alt && kept.logo == modifiers.logo
                    && kept.control == modifiers.ctrl && kept.shift == modifiers.shift));
                assert!(ordinary_input.key(&ordinary_scene, &translated).is_empty());
                assert!(masked_input.key(&masked_scene, &translated).is_empty());
            }
        }
        // Rejected select-all/clear shortcuts must not change masked edit mode.
        let messages = masked_input.key(&masked_scene, &Key::Text("fixture-value".into()));
        assert_eq!(messages, [InputEdit::Insert("fixture-value".into())]);
        assert!(!format!("{messages:?}").contains("fixture-value"));
        assert!(
            masked_scene
                .hits
                .iter()
                .all(|hit| hit.value.is_empty() && hit.text.is_none())
        );
        assert!(!format!("{:?}", masked_scene.paint).contains("fixture-value"));
    }
    #[test]
    fn control_shift_clipboard_keys_preserve_selection_and_byte_admission() {
        let mut value = "café".to_string();
        let mut field = TextFieldState::default();
        field.reset(&value);
        let tree = Element::text_input(&value, field.selection(), "", |edit| edit)
            .id("field")
            .autofocus()
            .size(180., 40.);
        let mut input = Interaction::default();
        let scene = layout().build(&tree, 180., 40., &input, 0.);
        input.synchronize(&scene);
        let mut effects = Effects::default();
        let control = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        apply(
            &mut field,
            &mut value,
            input.key(&scene, &key(Keysym::A, None, control)),
            &mut effects,
        );
        let selected = field.selection();
        let copy = input.key(&scene, &key(Keysym::C, None, control));
        assert_eq!(copy, [TextEdit::Copy]);
        apply(&mut field, &mut value, copy, &mut effects);
        let request = effects.clipboard.pop().unwrap();
        assert!(matches!(&request.action, ClipboardAction::Write(text) if text == "café"));
        apply(
            &mut field,
            &mut value,
            vec![request.finish(ClipboardOutcome::WritePosted)],
            &mut effects,
        );
        assert_eq!(value, "café");
        assert_eq!(field.selection(), selected);
        let paste = input.key(&scene, &key(Keysym::V, None, control));
        assert_eq!(paste, [TextEdit::Paste]);
        apply(&mut field, &mut value, paste, &mut effects);
        let reply = effects
            .clipboard
            .pop()
            .unwrap()
            .finish(ClipboardOutcome::Text("ééé".into()));
        assert_eq!(
            field.update(
                &mut value,
                reply,
                5,
                ClipboardTarget::new("test", "field"),
                &mut effects,
                |edit| edit,
            ),
            Err(TextEditError::ByteLimit)
        );
        assert_eq!(value, "café");
        assert_eq!(field.selection(), selected);
    }
    #[test]
    fn uppercase_control_select_and_clear_keep_masked_edits_count_only() {
        let tree = Element::masked_input(4, "", |edit| edit)
            .id("secret")
            .autofocus()
            .size(180., 40.);
        let mut input = Interaction::default();
        let scene = layout().build(&tree, 180., 40., &input, 0.);
        input.synchronize(&scene);
        let control = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert!(input.key(&scene, &key(Keysym::A, None, control)).is_empty());
        let replacement = input.key(&scene, &Key::Text("fixture-value".into()));
        assert_eq!(replacement, [InputEdit::Replace("fixture-value".into())]);
        assert!(!format!("{replacement:?}").contains("fixture-value"));
        assert_eq!(
            input.key(&scene, &key(Keysym::U, None, control)),
            [InputEdit::Clear]
        );
        assert!(input.key(&scene, &key(Keysym::Z, None, control)).is_empty());
        assert!(
            scene
                .hits
                .iter()
                .all(|hit| hit.value.is_empty() && hit.text.is_none())
        );
    }
    #[test]
    fn printable_text_and_composed_utf8_preserve_case_without_control_alt_or_logo() {
        assert_eq!(
            key(
                Keysym::A,
                Some("A"),
                Modifiers {
                    shift: true,
                    ..Default::default()
                }
            ),
            Key::Text("A".into())
        );
        assert_eq!(
            key(
                Keysym::A,
                Some("A"),
                Modifiers {
                    caps_lock: true,
                    ..Default::default()
                }
            ),
            Key::Text("A".into())
        );
        assert_eq!(
            key(Keysym::e, Some("é"), Modifiers::default()),
            Key::Text("é".into())
        );
        for (keysym, character) in [
            (Keysym::Z, 'z'),
            (Keysym::C, 'c'),
            (Keysym::V, 'v'),
            (Keysym::P, 'p'),
        ] {
            let modifiers = Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            };
            assert_eq!(
                key(keysym, Some("ignored control payload"), modifiers),
                Key::Shortcut {
                    character,
                    modifiers: KeyModifiers {
                        control: true,
                        shift: true,
                        ..Default::default()
                    },
                }
            );
        }
    }
    #[test]
    fn navigation_modifiers_and_context_menu_keep_their_existing_contract() {
        let modifiers = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            key(Keysym::Left, None, modifiers),
            Key::Modified {
                key: Box::new(Key::Left),
                modifiers: KeyModifiers {
                    control: true,
                    shift: true,
                    ..Default::default()
                },
            }
        );
        assert_eq!(
            key(
                Keysym::F10,
                None,
                Modifiers {
                    shift: true,
                    ..Default::default()
                }
            ),
            Key::ContextMenu
        );
        assert_eq!(
            key(Keysym::Menu, None, Modifiers::default()),
            Key::ContextMenu
        );
        assert_eq!(
            key(
                Keysym::Tab,
                None,
                Modifiers {
                    shift: true,
                    ..Default::default()
                }
            ),
            Key::BackTab
        );
        for modifiers in [
            Modifiers::default(),
            Modifiers {
                logo: true,
                ..Default::default()
            },
        ] {
            assert_eq!(
                translate(
                    KeyEvent {
                        time: 0,
                        raw_code: 0,
                        keysym: Keysym::F10,
                        utf8: Some("".into())
                    },
                    modifiers
                ),
                None
            );
        }
    }
}
