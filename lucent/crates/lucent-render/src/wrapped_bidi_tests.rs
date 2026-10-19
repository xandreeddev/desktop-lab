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

// Independent masks rasterize at their own origin before fractional GPU placement,
// whereas one combined mask rounds the full glyph position at coverage density.
// Permit only that one-physical-pixel phase in both directions, preserve total ink,
// and reject an independently rendered wrong run order below.
fn same_ink_with_one_pixel_phase(a: &[u8], b: &[u8], width: u32, height: u32) -> bool {
    let width = width as usize;
    let height = height as usize;
    if a.len() != width * height * 4 || b.len() != a.len() {
        return false;
    }
    // This fixture paints white text on transparent black. Alpha is therefore
    // the complete glyph coverage; require the corresponding RGB as well.
    if a.chunks_exact(4)
        .chain(b.chunks_exact(4))
        .any(|p| p[0] != p[3] || p[1] != p[3] || p[2] != p[3])
    {
        return false;
    }
    let coverage = |image: &[u8], x: usize, y: usize| image[(y * width + x) * 4 + 3];
    let bounds = |image: &[u8]| {
        let mut result: Option<(usize, usize, usize, usize)> = None;
        for y in 0..height {
            for x in 0..width {
                if coverage(image, x, y) > 8 {
                    result = Some(match result {
                        None => (x, y, x, y),
                        Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    });
                }
            }
        }
        result
    };
    let (Some(a_bounds), Some(b_bounds)) = (bounds(a), bounds(b)) else {
        return false;
    };
    if [
        a_bounds.0.abs_diff(b_bounds.0),
        a_bounds.1.abs_diff(b_bounds.1),
        a_bounds.2.abs_diff(b_bounds.2),
        a_bounds.3.abs_diff(b_bounds.3),
    ]
    .into_iter()
    .any(|difference| difference > 1)
    {
        return false;
    }
    let alpha_mass = |image: &[u8]| image.chunks_exact(4).map(|p| u64::from(p[3])).sum::<u64>();
    let nonzero = |image: &[u8]| image.chunks_exact(4).filter(|p| p[3] > 0).count() as u64;
    // At most one alpha quantization unit per covered physical pixel.
    if alpha_mass(a).abs_diff(alpha_mass(b)) > nonzero(a).max(nonzero(b)) {
        return false;
    }
    for (actual, expected) in [(a, b), (b, a)] {
        for y in 0..height {
            for x in 0..width {
                let mut neighboring = 0;
                for v in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                    for u in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                        neighboring = neighboring.max(coverage(expected, u, v));
                    }
                }
                if coverage(actual, x, y).saturating_sub(neighboring) > 8 {
                    return false;
                }
            }
        }
    }
    true
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
    let hebrew_advance = lucent_ui::text::shape_directed(
        &fonts[0],
        "אבג",
        20.,
        Some(lucent_ui::text::Direction::RightToLeft),
    )
    .width;
    let latin_advance = lucent_ui::text::shape_directed(
        &fonts[0],
        " xyz",
        20.,
        Some(lucent_ui::text::Direction::LeftToRight),
    )
    .width;
    if let Paint::Text {
        text, layout, rect, ..
    } = &mut latin
    {
        *text = " xyz".into();
        layout.bidi = None;
        layout.direction = Some(lucent_ui::text::Direction::LeftToRight);
        rect.x += hebrew_advance;
    }
    let mut wrong_latin = latin.clone();
    let mut wrong_hebrew = hebrew.clone();
    if let Paint::Text { rect, .. } = &mut wrong_latin {
        rect.x -= hebrew_advance;
    }
    if let Paint::Text { rect, .. } = &mut wrong_hebrew {
        rect.x += latin_advance;
    }
    let mut canvas = Canvas::new(Gpu::headless(fonts).unwrap());
    for density in [1, 2] {
        eprintln!(
            "independent split-run origin: logical advance={hebrew_advance}, physical fractional phase={} at {density}×",
            (hebrew_advance * density as f32).fract()
        );
        let a = canvas
            .snapshot(width, 64, density, std::slice::from_ref(&actual))
            .unwrap();
        let b = canvas
            .snapshot(width, 64, density, &[hebrew.clone(), latin.clone()])
            .unwrap();
        assert!(
            same_ink_with_one_pixel_phase(&a, &b, width * density, 64 * density),
            "wrapped paragraph run coverage differs at {density}×"
        );
        let wrong = canvas
            .snapshot(
                width,
                64,
                density,
                &[wrong_latin.clone(), wrong_hebrew.clone()],
            )
            .unwrap();
        assert!(
            !same_ink_with_one_pixel_phase(&a, &wrong, width * density, 64 * density),
            "the coverage comparison must reject reversed run order at {density}×"
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
