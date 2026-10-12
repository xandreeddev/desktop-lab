use super::*;

fn layout() -> Layout {
    Layout::new(Arc::new(vec![
        fontdue::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .unwrap(),
    ]))
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
