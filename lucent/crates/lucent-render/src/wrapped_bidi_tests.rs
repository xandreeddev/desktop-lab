use super::*;
use lucent_api::{Element, ScrollSpec, ScrollStyle, TextSelection};

fn fonts() -> Arc<Vec<lucent_ui::text::Font>> {
    Arc::new(vec![
        lucent_ui::text::Font::from_bytes(
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf").as_slice(),
            Default::default(),
        )
        .unwrap(),
    ])
}

fn row(fonts: Arc<Vec<lucent_ui::text::Font>>, opening: &str, body: &str) -> (Paint, u32) {
    let prefix = "i".repeat(24);
    let width = lucent_ui::text::width(&fonts[0], &format!("{prefix} "), 20.) + 0.25;
    let value = format!("{prefix}{opening} {body}");
    let node = Element::<()>::text_area(
        &value,
        TextSelection::default(),
        "",
        ScrollSpec::new(0., 24.).unwrap(),
        |_| (),
        |_| (),
    )
    .id("wrapped")
    .size(width, 200.)
    .font(20.)
    .scroll_style(ScrollStyle {
        gutter: 0.,
        width: 0.,
        ..Default::default()
    });
    let scene = lucent_ui::Layout::new(fonts).build(
        &node,
        width,
        200.,
        &lucent_ui::Interaction::default(),
        0.,
    );
    let mut paint = scene
        .paint
        .into_iter()
        .find(|paint| matches!(paint, Paint::Text {text,..} if text == body))
        .expect("the complete body must start a soft row");
    if let Paint::Text { rect, clip, .. } = &mut paint {
        rect.y = 0.;
        *clip = Rect::new(0., 0., width.ceil(), 64.).into();
    }
    (paint, width.ceil() as u32)
}

#[test]
#[ignore = "requires Vulkan; run explicitly with --ignored"]
fn wrapped_bidi_pixels_match_explicit_visual_runs_at_both_densities() {
    let fonts = fonts();
    let (actual, width) = row(fonts.clone(), "", "אבג xyz");
    let mut hebrew = actual.clone();
    let mut latin = actual.clone();
    if let Paint::Text { text, layout, .. } = &mut hebrew {
        *text = "אבג".into();
        layout.bidi = None;
        layout.direction = Some(lucent_ui::text::Direction::RightToLeft);
    }
    if let Paint::Text {
        text, layout, rect, ..
    } = &mut latin
    {
        *text = " xyz".into();
        layout.bidi = None;
        layout.direction = Some(lucent_ui::text::Direction::LeftToRight);
        rect.x += lucent_ui::text::shape_directed(
            &fonts[0],
            "אבג",
            20.,
            Some(lucent_ui::text::Direction::RightToLeft),
        )
        .width;
    }
    let mut canvas = Canvas::new(Gpu::headless(fonts).unwrap());
    for density in [1, 2] {
        let a = canvas
            .snapshot(width, 64, density, std::slice::from_ref(&actual))
            .unwrap();
        let b = canvas
            .snapshot(width, 64, density, &[hebrew.clone(), latin.clone()])
            .unwrap();
        assert!(
            a.iter().zip(&b).all(|(a, b)| a.abs_diff(*b) <= 8),
            "wrapped paragraph order must paint Hebrew, then Latin at {density}x"
        );
    }
}

#[test]
#[ignore = "requires Vulkan; run explicitly with --ignored"]
fn wrapped_bidi_cache_rejects_stale_same_length_source_before_lookup() {
    let fonts = fonts();
    let (valid, width) = row(fonts.clone(), "\u{202e}", "uvw XYZ");
    let (stale_source, _) = row(fonts.clone(), "\u{202e}", "abc DEF");
    let stale_context = match stale_source {
        Paint::Text { layout, .. } => layout.bidi,
        _ => unreachable!(),
    };
    let mut ordinary = valid.clone();
    if let Paint::Text { layout, .. } = &mut ordinary {
        layout.bidi = None;
    }
    let mut stale = valid.clone();
    if let Paint::Text { layout, .. } = &mut stale {
        layout.bidi = stale_context;
    }
    let mut canvas = Canvas::new(Gpu::headless(fonts).unwrap());
    for density in [1, 2] {
        let expected = canvas
            .snapshot(width, 64, density, std::slice::from_ref(&ordinary))
            .unwrap();
        let rtl = canvas
            .snapshot(width, 64, density, std::slice::from_ref(&valid))
            .unwrap();
        assert_ne!(
            expected, rtl,
            "the override must actually change the pixels"
        );
        let rejected = canvas
            .snapshot(width, 64, density, std::slice::from_ref(&stale))
            .unwrap();
        assert_eq!(
            rejected, expected,
            "stale order must not hit the valid RTL texture"
        );
    }
}
