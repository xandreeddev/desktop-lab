//! Deterministic fixtures use the production layout, fonts, shaders and Vulkan renderer.
//! Run ignored snapshots explicitly; no session, D-Bus service or compositor is needed.
use crate::desktop::{Desktop, Message, Mode};
use lucent_api::*;
use lucent_domain::{AppId, Application as App, ClockSnapshot, Date, LaunchCommand};
use lucent_ui::{Interaction, Layout, Paint, Scene};
use std::{path::PathBuf, sync::Arc};

fn fixture(count: usize, light: bool) -> Desktop {
    let mut app = Desktop::new();
    app.hypr = None;
    app.settings.light = light;
    app.viewport = (640., 580.);
    app.clock = ClockSnapshot {
        date: Date {
            year: 2026,
            month: 1,
            day: 15,
            weekday: 3,
        },
        hour: 10,
        minute: 24,
        unix_seconds: 1_768_472_640,
    };
    let names = [
        "Terminal",
        "Files",
        "Music",
        "Settings",
        "Calendar",
        "Browser",
        "A deliberately long application name",
        "Calculator",
        "Last application",
    ];
    let icons = [
        "command",
        "apps",
        "music",
        "palette",
        "widgets",
        "network",
        "wallpaper",
        "sun",
        "cloud",
    ];
    app.apps = (0..count)
        .map(|n| App {
            id: AppId(format!("fixture-{n}.desktop")),
            name: names[n % names.len()].into(),
            description: String::new(),
            keywords: vec![],
            icon: if n == 8 {
                "missing-fixture-icon".into()
            } else {
                format!("symbol:{}", icons[n % icons.len()])
            },
            startup_class: String::new(),
            command: LaunchCommand {
                program: "unused-fixture".into(),
                args: vec![],
                directory: None,
                terminal: false,
            },
        })
        .collect();
    app.results = (0..count).collect();
    app.update(Message::Mode(Mode::Apps), &mut Effects::default());
    app
}
fn fonts(app: &Desktop) -> Arc<Vec<fontdue::Font>> {
    Arc::new(
        app.fonts()
            .into_iter()
            .map(|f| fontdue::Font::from_bytes(f, Default::default()).unwrap())
            .collect(),
    )
}
fn scene(
    app: &Desktop,
    layout: &Layout,
    interaction: &mut Interaction<Message>,
    now: f64,
) -> Scene<Message> {
    let cx = ViewContext {
        surface: "dock",
        width: app.viewport.0,
        height: app.viewport.1,
        now,
    };
    let root = app.view(&cx);
    let first = layout.build(&root, cx.width, cx.height, interaction, now);
    interaction.synchronize(&first);
    layout.build(&root, cx.width, cx.height, interaction, now)
}
fn key(app: &mut Desktop, key: Key) {
    let msg = app
        .event(Event::Key {
            surface: "dock",
            key,
        })
        .expect("handled key");
    app.update(msg, &mut Effects::default());
}
fn fully_visible(rect: Rect, clip: Rect) -> bool {
    rect.x >= clip.x - 0.01
        && rect.y >= clip.y - 0.01
        && rect.x + rect.w <= clip.x + clip.w + 0.01
        && rect.y + rect.h <= clip.y + clip.h + 0.01
}
#[test]
fn every_visible_launcher_row_fits_even_after_scrolling() {
    for count in [1, 7, 9] {
        let mut app = fixture(count, false);
        let layout = Layout::new(fonts(&app));
        for width in [640., 390.] {
            app.update(
                Message::Resize("dock", width, 580.),
                &mut Effects::default(),
            );
            for index in [0, count - 1] {
                app.update(Message::Select(index), &mut Effects::default());
                let scene = scene(&app, &layout, &mut Interaction::default(), 1.);
                let rows: Vec<_> = scene
                    .hits
                    .iter()
                    .filter(|h| h.id.starts_with("result-"))
                    .collect();
                assert_eq!(rows.len(), count.min(7));
                for row in rows {
                    assert!(
                        fully_visible(row.rect, row.clip),
                        "cropped {} at {width}: {:?} in {:?}",
                        row.id,
                        row.rect,
                        row.clip
                    );
                }
                assert!(
                    scene
                        .paint
                        .iter()
                        .any(|p| matches!(p, Paint::Outline { .. })),
                    "keyboard selection needs a visible outline"
                );
            }
        }
    }
}
#[test]
fn search_text_icons_and_caret_share_a_centered_line_box() {
    let mut app = fixture(9, false);
    app.query =
        "A long input that must keep the caret visible without escaping the field ".repeat(3);
    let layout = Layout::new(fonts(&app));
    let scene = scene(&app, &layout, &mut Interaction::default(), 1.);
    let input = scene
        .hits
        .iter()
        .find(|h| h.id == "launcher-search")
        .unwrap();
    let center = input.rect.y + input.rect.h / 2.;
    let clear = scene.hits.iter().find(|h| h.id == "clear-search").unwrap();
    assert!((center - clear.rect.y - clear.rect.h / 2.).abs() < 0.01);
    let (mut icons, mut carets) = (0, 0);
    for paint in &scene.paint {
        if let Paint::Image { rect, data, .. } = paint
            && data.key.contains("search")
        {
            icons += 1;
            assert!((center - rect.y - rect.h / 2.).abs() < 0.01);
            assert!((rect.w / rect.h - data.width as f32 / data.height as f32).abs() < 0.01);
        }
        if let Paint::Shape { rect, clip, .. } = paint
            && rect.w == lucent_design::component::input::CARET_WIDTH
        {
            carets += 1;
            assert!(
                fully_visible(*rect, *clip),
                "caret should stay inside input"
            );
            assert!(
                (center - rect.y - rect.h / 2.).abs() < 0.01,
                "caret centered on same baseline"
            );
        }
    }
    assert_eq!((icons, carets), (1, 1));
}
#[test]
fn tab_cycles_sections_and_arrows_activate_the_selected_item() {
    let mut app = fixture(9, false);
    for expected in [
        Mode::Commands,
        Mode::Wallpapers,
        Mode::Themes,
        Mode::Widgets,
        Mode::Apps,
    ] {
        key(&mut app, Key::Tab);
        assert_eq!(app.mode, expected);
    }
    key(&mut app, Key::BackTab);
    assert_eq!(app.mode, Mode::Widgets);
    key(&mut app, Key::Down);
    assert_eq!(app.selected, 1);
    let before = app.settings.visible_widgets.contains(&"clock".into());
    key(&mut app, Key::Enter);
    assert_ne!(
        before,
        app.settings.visible_widgets.contains(&"clock".into())
    );
    key(&mut app, Key::BackTab);
    assert_eq!(app.mode, Mode::Themes);
    key(&mut app, Key::Right);
    key(&mut app, Key::Enter);
    assert!(app.settings.light);
}
#[test]
fn framework_buttons_support_keyboard_focus_and_activation_without_hover() {
    let app = fixture(0, false);
    let layout = Layout::new(fonts(&app));
    let root = app.theme().apply(Element::row(vec![
        Element::button("One", 1).id("one").size(100., 40.),
        Element::button("Two", 2).id("two").size(100., 40.),
    ]));
    let mut interaction = Interaction::default();
    let scene = layout.build(&root, 220., 50., &interaction, 0.);
    interaction.key(&scene, &Key::Tab);
    assert_eq!(interaction.focus.as_deref(), Some("one"));
    assert_eq!(interaction.key(&scene, &Key::Enter), Some(1));
    interaction.key(&scene, &Key::Tab);
    assert_eq!(interaction.key(&scene, &Key::Text(" ".into())), Some(2));
    interaction.key(&scene, &Key::BackTab);
    let focused = layout.build(&root, 220., 50., &interaction, 0.);
    assert!(
        focused
            .paint
            .iter()
            .any(|p| matches!(p, Paint::Outline{rect, ..} if rect.x == 0.))
    );
}
fn changed_pixels(expected: &[u8], actual: &[u8]) -> usize {
    expected
        .as_chunks::<4>()
        .0
        .iter()
        .zip(actual.as_chunks::<4>().0.iter())
        .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 8))
        .count()
}
// A tiny missing icon must not disappear in a large, mostly unchanged screenshot.
fn worst_tile_change(expected: &[u8], actual: &[u8], width: u32, height: u32) -> f64 {
    let columns = width.div_ceil(64) as usize;
    let mut tiles = vec![0usize; columns * height.div_ceil(64) as usize];
    for (index, (a, b)) in expected
        .as_chunks::<4>()
        .0
        .iter()
        .zip(actual.as_chunks::<4>().0.iter())
        .enumerate()
    {
        if a.iter().zip(b).any(|(x, y)| x.abs_diff(*y) > 8) {
            let (x, y) = (index % width as usize, index / width as usize);
            tiles[(y / 64) * columns + x / 64] += 1;
        }
    }
    tiles
        .into_iter()
        .enumerate()
        .map(|(index, count)| {
            let w = (width as usize - (index % columns) * 64).min(64);
            let h = (height as usize - (index / columns) * 64).min(64);
            count as f64 / (w * h) as f64
        })
        .fold(0., f64::max)
}
#[test]
fn image_comparison_detects_a_clipped_row() {
    let expected = vec![255; 640 * 580 * 4];
    let mut actual = expected.clone();
    actual[640 * 400 * 4..640 * 409 * 4].fill(0);
    assert!(changed_pixels(&expected, &actual) as f64 / (640. * 580.) > 0.001);
    actual.copy_from_slice(&expected);
    for y in 0..12 {
        actual[y * 640 * 4..(y * 640 + 12) * 4].fill(0);
    }
    assert!(changed_pixels(&expected, &actual) as f64 / (640. * 580.) < 0.001);
    assert!(worst_tile_change(&expected, &actual, 640, 580) > 0.01);
}

#[test]
#[ignore = "requires Vulkan; run explicitly with --ignored"]
fn visual_regressions() {
    let base = fixture(9, false);
    let fonts = fonts(&base);
    let layout = Layout::new(fonts.clone());
    let gpu = lucent_render::Gpu::headless(fonts).expect("Vulkan adapter for native visual tests");
    let mut canvas = lucent_render::Canvas::new(gpu);
    let root = std::env::var_os("LUCENT_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    let baselines = root.join("lucent/tests/visual/baselines");
    let report = root.join("reports/local/visual-tests");
    std::fs::create_dir_all(&report).unwrap();
    std::fs::create_dir_all(&baselines).unwrap();
    let update = std::env::var("LUCENT_UPDATE_GOLDENS").as_deref() == Ok("1");
    let mut failures = vec![];
    let mut html = String::from(
        "<!doctype html><meta charset=utf-8><title>Lucent native visual checks</title><style>body{background:#19191d;color:#eee;font:16px system-ui;margin:40px}section{margin-bottom:48px}img{max-width:45%;vertical-align:top;border:1px solid #555}code{color:#c8b6ff}</style><h1>Lucent native visual checks</h1><p>Production Rust layout → Vulkan shader → fixed PNG. Left: actual; right: amplified difference. Pink marks changed pixels.</p>",
    );
    for light in [false, true] {
        for (name, mode, count, selected, query, now, width) in [
            ("apps-first", Mode::Apps, 9, 0, "", 1., 640.),
            ("apps-last", Mode::Apps, 9, 8, "", 1., 640.),
            ("apps-empty", Mode::Apps, 0, 0, "no results", 1., 640.),
            (
                "apps-long-input",
                Mode::Apps,
                1,
                0,
                "A long query that keeps its caret within the search input even when it overflows",
                1.,
                640.,
            ),
            ("apps-narrow", Mode::Apps, 7, 6, "", 1., 390.),
            ("apps-opening", Mode::Apps, 9, 0, "", 0.12, 640.),
            ("commands", Mode::Commands, 9, 5, "", 1., 640.),
            ("themes", Mode::Themes, 9, 1, "", 1., 640.),
            ("widgets", Mode::Widgets, 9, 6, "", 1., 640.),
            ("wallpapers-empty", Mode::Wallpapers, 9, 0, "", 1., 640.),
        ] {
            let mut app = fixture(count, light);
            app.update(
                Message::Resize("dock", width, 580.),
                &mut Effects::default(),
            );
            app.update(Message::Mode(mode), &mut Effects::default());
            app.update(Message::Select(selected), &mut Effects::default());
            app.query = query.into();
            let scene = scene(&app, &layout, &mut Interaction::default(), now);
            let mut paint = vec![Paint::Shape {
                rect: Rect::new(0., 0., width, 580.),
                clip: Rect::new(0., 0., width, 580.),
                color: Color::hex(if light { 0xc9c3b9 } else { 0x28272d }),
                radius: 0.,
                shadow: false,
            }];
            paint.extend(scene.paint);
            for scale in [1, 2] {
                let id = format!("{name}-{}-{scale}x", if light { "light" } else { "dark" });
                let (w, h) = (width as u32 * scale, 580 * scale);
                let actual = canvas.snapshot(width as u32, 580, scale, &paint).unwrap();
                let path = baselines.join(format!("{id}.png"));
                let save = |path: &std::path::Path, bytes: &[u8]| {
                    image::save_buffer(path, bytes, w, h, image::ColorType::Rgba8).unwrap()
                };
                save(&report.join(format!("{id}.png")), &actual);
                if update {
                    save(&path, &actual);
                }
                let mut difference = vec![0; actual.len()];
                let result = if let Ok(expected) = image::open(&path) {
                    let expected = expected.to_rgba8();
                    if expected.dimensions() != (w, h) {
                        Err("size mismatch".into())
                    } else {
                        for ((e, a), d) in expected
                            .as_raw()
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .zip(actual.as_chunks::<4>().0.iter())
                            .zip(difference.as_chunks_mut::<4>().0.iter_mut())
                        {
                            d.copy_from_slice(
                                if e.iter().zip(a).any(|(x, y)| x.abs_diff(*y) > 8) {
                                    &[255, 0, 180, 255]
                                } else {
                                    &[30, 30, 35, 255]
                                },
                            );
                        }
                        let fraction =
                            changed_pixels(expected.as_raw(), &actual) as f64 / f64::from(w * h);
                        let tile = worst_tile_change(expected.as_raw(), &actual, w, h);
                        if fraction > 0.001 || tile > 0.01 {
                            Err(format!(
                                "{:.3}% changed pixels; worst tile {:.2}%",
                                fraction * 100.,
                                tile * 100.
                            ))
                        } else {
                            Ok(())
                        }
                    }
                } else {
                    Err("missing baseline (review before accepting)".into())
                };
                save(&report.join(format!("{id}-diff.png")), &difference);
                html.push_str(&format!("<section><h2>{id}</h2><p>{}</p><img src='{id}.png'><img src='{id}-diff.png'></section>", result.as_ref().err().map(String::as_str).unwrap_or("Passed")));
                if let Err(reason) = result {
                    failures.push(format!("{id}: {reason}"));
                }
            }
        }
    }
    std::fs::write(report.join("index.html"), html).unwrap();
    assert!(
        failures.is_empty(),
        "Visual regressions: {}. See reports/local/visual-tests/index.html",
        failures.join("; ")
    );
}
