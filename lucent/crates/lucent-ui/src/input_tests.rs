use super::*;

fn layout() -> Layout {
    Layout::new(Arc::new(vec![
        crate::text::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .unwrap(),
    ]))
}

fn text_field(value: &str, selection: TextSelection) -> Element<TextEdit> {
    Element::text_input(value, selection, "Search", |edit| edit)
        .id("text")
        .autofocus()
        .size(180., 40.)
        .padding(8.)
        .clip()
}
#[test]
fn typed_text_edits_in_a_burst_use_current_value_without_a_new_scene() {
    let mut value = "one é".to_string();
    let mut selection = TextSelection::at_end(&value);
    let mut input = Interaction::default();
    let scene = layout().build(&text_field(&value, selection), 180., 40., &input, 0.);
    input.synchronize(&scene);
    for key in [
        Key::Left,
        Key::Backspace,
        Key::Text("!".into()),
        Key::Text("x".into()),
    ] {
        let messages = input.key(&scene, &key);
        assert_eq!(messages.len(), 1);
        for edit in messages {
            selection.apply(&mut value, edit, 8192).unwrap();
        }
    }
    assert_eq!(value, "one!xé");
    assert!(input.input_buffer.is_none());
    let modified = Key::Modified {
        key: Box::new(Key::Home),
        modifiers: KeyModifiers {
            shift: true,
            ..Default::default()
        },
    };
    for edit in input.key(&scene, &modified) {
        selection.apply(&mut value, edit, 8192).unwrap();
    }
    assert_eq!(selection.selected(&value), "one!x");
    for edit in input.key(&scene, &Key::Text("replaced".into())) {
        selection.apply(&mut value, edit, 8192).unwrap();
    }
    assert_eq!(value, "replacedé");
}
#[test]
fn pointer_grapheme_selection_maps_through_children_and_cancels_on_replacement() {
    let mut value = "AV e\u{301} tail".to_string();
    let mut selection = TextSelection::at_end(&value);
    let mut input = Interaction::default();
    let layout = layout();
    let scene = layout.build(&text_field(&value, selection), 180., 40., &input, 0.);
    input.synchronize(&scene);
    let hit = scene.hits.iter().find(|hit| hit.id == "text").unwrap();
    let meta = hit.text.as_ref().unwrap();
    let x = meta.inner.x;
    let y = meta.inner.y + meta.inner.h / 2.;
    let edit = input.press(&scene, x, y).unwrap();
    selection.apply(&mut value, edit, 8192).unwrap();
    assert_eq!(selection.caret(), 0);
    for edit in input.motion(&scene, x + 50., y, 0.) {
        selection.apply(&mut value, edit, 8192).unwrap();
    }
    assert_eq!(selection.anchor(), 0);
    assert!(!selection.selected(&value).is_empty());
    assert!(TextSelection::boundaries(&value).any(|at| at == selection.caret()));
    value = "replacement".into();
    let replaced = layout.build(&text_field(&value, selection), 180., 40., &input, 0.);
    input.synchronize(&replaced);
    assert!(input.release(x + 80., y).is_none());
    let mapped = text_field(&value, TextSelection::default()).map(|edit| ("child", edit));
    let scene = layout.build(&mapped, 180., 40., &Interaction::default(), 0.);
    let mut input = Interaction::default();
    input.synchronize(&scene);
    assert!(matches!(
        input.key(&scene, &Key::Text("!".into())).as_slice(),
        [("child", TextEdit::Insert(_))]
    ));
}
#[test]
fn selection_paint_and_caret_share_geometry_and_keep_the_active_edge_visible() {
    let value = "A long line with a marked final word";
    let mut selection = TextSelection::at_end(value);
    let mut text = value.to_string();
    selection
        .apply(
            &mut text,
            TextEdit::Move {
                movement: TextMove::PreviousWord,
                extend: true,
            },
            8192,
        )
        .unwrap();
    let tree = text_field(value, selection);
    let layout = layout();
    let mut input = Interaction::default();
    let first = layout.build(&tree, 180., 40., &input, 0.);
    input.synchronize(&first);
    let scene = layout.build(&tree, 180., 40., &input, 0.);
    let hit = &scene.hits[0];
    let meta = hit.text.as_ref().unwrap();
    assert!(meta.scroll > 0.);
    let caret = scene
        .paint
        .iter()
        .rev()
        .find_map(|paint| {
            if let Paint::Shape { rect, .. } = paint {
                Some(*rect)
            } else {
                None
            }
        })
        .unwrap();
    assert!(caret.x >= meta.inner.x && caret.x + caret.w <= meta.inner.x + meta.inner.w + 0.001);
    assert_eq!(meta.at(caret.x), Some(selection.caret()));
    assert!(
        scene
            .paint
            .iter()
            .filter(|paint| matches!(paint, Paint::Shape { .. }))
            .count()
            >= 2
    );
}
#[test]
fn normal_text_pointer_shift_extends_and_empty_text_input_submits_without_a_click_action() {
    let value = "abc";
    let mut selection = TextSelection::default();
    let mut current = value.to_string();
    selection
        .apply(
            &mut current,
            TextEdit::Select {
                anchor: 1,
                caret: 1,
            },
            8192,
        )
        .unwrap();
    let tree = text_field(value, selection).on_submit(TextEdit::Clear);
    let layout = layout();
    let mut input = Interaction::default();
    let scene = layout.build(&tree, 180., 40., &input, 0.);
    input.synchronize(&scene);
    let edit = input
        .press_with_modifiers(
            &scene,
            160.,
            20.,
            KeyModifiers {
                shift: true,
                ..Default::default()
            },
        )
        .unwrap();
    selection.apply(&mut current, edit, 8192).unwrap();
    assert_eq!(selection.anchor(), 1);
    assert_eq!(selection.caret(), 3);
    assert_eq!(input.key(&scene, &Key::Enter), [TextEdit::Clear]);
}

#[test]
fn masked_field_burst_edits_focus_replacement_and_paint_hold_no_plaintext() {
    let layout = layout();
    let field = Element::masked_input(4, "Password", |edit| edit)
        .id("secret")
        .autofocus()
        .size(160., 40.);
    let button = Element::text("Done").on_click(InputEdit::Clear).id("done");
    let tree = Element::column(vec![field, button]).fill();
    let mut input = Interaction::default();
    let initial = layout.build(&tree, 200., 100., &input, 0.);
    input.synchronize(&initial);
    let scene = layout.build(&tree, 200., 100., &input, 0.);
    assert_eq!(input.focus.as_deref(), Some("secret"));
    assert!(input.input_buffer.is_none());
    assert!(scene.hits.iter().all(|hit| hit.value.is_empty()));
    assert!(
        scene
            .paint
            .iter()
            .any(|p| matches!(p, Paint::Text { text, .. } if text=="••••"))
    );
    for text in ["fixture", "é", " "] {
        assert_eq!(
            input.key(&scene, &Key::Text(text.into())),
            [InputEdit::Insert(text.into())]
        );
        assert!(input.input_buffer.is_none());
    }
    input.key(&scene, &Key::SelectAll);
    assert_eq!(
        input.key(&scene, &Key::Text("other".into())),
        [InputEdit::Replace("other".into())]
    );
    assert_eq!(input.key(&scene, &Key::Backspace), [InputEdit::Backspace]);
    input.key(&scene, &Key::SelectAll);
    assert_eq!(input.key(&scene, &Key::Backspace), [InputEdit::Clear]);
    assert_eq!(input.key(&scene, &Key::ClearInput), [InputEdit::Clear]);
    assert!(input.key(&scene, &Key::Enter).is_empty());
    input.key(&scene, &Key::Tab);
    assert_eq!(input.focus.as_deref(), Some("done"));
    assert_eq!(input.key(&scene, &Key::Enter), [InputEdit::Clear]);
    input.key(&scene, &Key::BackTab);
    assert_eq!(input.focus.as_deref(), Some("secret"));
    input.press(&scene, 40., 20.);
    assert_eq!(
        input.key(&scene, &Key::Text("last".into())),
        [InputEdit::Insert("last".into())]
    );
    assert!(input.input_buffer.is_none());
}

#[test]
fn enter_submits_normal_and_masked_fields_without_submitting_on_pointer_focus() {
    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Edited,
        Submit,
    }
    for masked in [false, true] {
        let field = if masked {
            Element::masked_input(2, "Password", |_| Message::Edited)
        } else {
            Element::input("value", "Name", |_| Message::Edited)
        };
        let tree = field
            .on_submit(Message::Submit)
            .map(|m| ("child", m))
            .id("field")
            .size(160., 40.);
        let scene = layout().build(&tree, 200., 100., &Interaction::default(), 0.);
        let mut input = Interaction::default();
        assert!(input.press(&scene, 40., 20.).is_none());
        assert_eq!(input.key(&scene, &Key::Enter), [("child", Message::Submit)]);
        assert_eq!(
            input.key(&scene, &Key::Text(" ".into())),
            [("child", Message::Edited)]
        );
    }
}

#[test]
fn wrapped_fill_buttons_are_measured_at_their_allocated_row_width() {
    let layout = layout();
    let button = |text| {
        Element::text(text)
            .font(16.)
            .wrap(3)
            .padding(12.)
            .width(Length::Fill)
            .clip()
            .on_click(())
    };
    let tree = Element::column(vec![
        Element::row(vec![button("Enhanced open"), button("WPA personal")])
            .width(Length::Fill)
            .gap(8.),
        Element::text("Following row").id("following"),
    ])
    .width(Length::Fill);
    let scene = layout.build(&tree, 200., 200., &Interaction::default(), 0.);
    for paint in &scene.paint {
        if let Paint::Text {
            rect,
            text,
            size,
            face,
            ..
        } = paint
            && text.contains('\n')
        {
            let height = text::line_metrics(&layout.fonts[*face], *size).height
                * text.lines().count() as f32;
            assert!(
                rect.h + 0.001 >= height,
                "Wrapped rows must reserve the full text height"
            );
        }
    }
    assert!(
        scene
            .paint
            .iter()
            .any(|p| matches!(p,Paint::Text{text,..} if text.contains('\n')))
    );
    assert!(scene.hits.iter().all(|h| h.rect.h > 48.));
}

#[test]
fn padded_intrinsic_labels_do_not_wrap_from_float_rounding() {
    let layout = layout();
    for label in ["Dismiss", "Retry", "Close", "Cancel"] {
        let tree = Element::<()>::text(label)
            .font(13.)
            .wrap(2)
            .padding_xy(24., 8.)
            .clip();
        let scene = layout.build(&tree, 390., 100., &Interaction::default(), 0.);
        assert!(
            scene
                .paint
                .iter()
                .any(|p| matches!(p,Paint::Text{text,..} if text==label))
        );
    }
}
