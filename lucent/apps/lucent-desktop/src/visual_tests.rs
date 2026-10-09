//! Deterministic fixtures use the production layout, fonts, shaders and Vulkan renderer.
//! Run ignored snapshots explicitly; no session, D-Bus service or compositor is needed.
use crate::desktop::{Desktop, Message, Mode};
use lucent_api::*;
use lucent_domain::{AppId, Application as App, ClockSnapshot, Date, LaunchCommand};
use lucent_ui::{Interaction, Layout, Paint, Scene};
use std::{path::PathBuf, sync::Arc};

fn fixture(count: usize, light: bool) -> Desktop {
    let mut app = Desktop::new(crate::test_ports::ports());
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
            id: AppId(
                [
                    "foot.desktop",
                    "org.gnome.Nautilus.desktop",
                    "cliamp.desktop",
                    "fcitx5-configtool.desktop",
                    "Google Contacts.desktop",
                    "chromium.desktop",
                    "libreoffice-writer.desktop",
                    "omacalc.desktop",
                    "unmapped-fixture.desktop",
                ][n % 9]
                    .into(),
            ),
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
    for a in &app.apps {
        if let Some(asset) = lucent_design::app_icons::lookup(&a.id.0) {
            app.images.insert(
                format!("app:{}", a.id.0),
                lucent_services::images::svg(asset.svg, 128, format!("lucent-app:{}", asset.id))
                    .unwrap(),
            );
        }
    }
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
#[test]
fn command_rows_center_icons_and_labels_inside_rounded_endcaps() {
    let mut app = fixture(9, false);
    app.update(Message::Mode(Mode::Commands), &mut Effects::default());
    let layout = Layout::new(fonts(&app));
    let command_scene = scene(&app, &layout, &mut Interaction::default(), 1.);
    let rows: Vec<_> = command_scene
        .hits
        .iter()
        .filter(|h| h.id.starts_with("command-"))
        .collect();
    assert_eq!(rows.len(), 6);
    for row in rows {
        let center = row.rect.y + row.rect.h / 2.;
        let paints: Vec<_> = command_scene
            .paint
            .iter()
            .filter_map(|p| match p {
                Paint::Image { rect, .. } | Paint::Text { rect, .. }
                    if rect.y >= row.rect.y && rect.y + rect.h <= row.rect.y + row.rect.h =>
                {
                    Some(rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(paints.len(), 2);
        for rect in paints {
            assert!(
                (rect.y + rect.h / 2. - center).abs() < 0.01,
                "command contents share a vertical center"
            );
            assert!(rect.x - row.rect.x >= 16., "content clears curved endcap");
            assert!(fully_visible(*rect, row.clip));
        }
    }
    app.update(Message::CloseLauncher, &mut Effects::default());
    let dock = scene(&app, &layout, &mut Interaction::default(), 1.);
    let panel = dock.hits.iter().find(|h| h.id == "dock-panel").unwrap();
    let slots: Vec<_> = dock
        .hits
        .iter()
        .filter(|h| h.id.starts_with("dock-") && h.id != "dock-panel")
        .collect();
    let first = slots.first().unwrap();
    let last = slots.last().unwrap();
    assert_eq!(first.rect.x - panel.rect.x, 16.);
    assert_eq!(panel.rect.x + panel.rect.w - last.rect.x - last.rect.w, 16.);
    assert_eq!(first.rect.y - panel.rect.y, 8.);
    assert_eq!(
        panel.rect.y + panel.rect.h - first.rect.y - first.rect.h,
        8.
    );
}

#[test]
fn axis_padding_preserves_intrinsic_size_fill_and_hit_bounds() {
    let app = fixture(0, false);
    let layout = Layout::new(fonts(&app));
    let child = Element::empty().size(20., 10.).on_click(1).id("child");
    let box_ = Element::row(vec![child])
        .padding_xy(18., 6.)
        .on_click(2)
        .id("box");
    let root = Element::stack(vec![box_]).fill();
    let scene = layout.build(&root, 200., 100., &Interaction::default(), 0.);
    let outer = scene.hits.iter().find(|h| h.id == "box").unwrap();
    let inner = scene.hits.iter().find(|h| h.id == "child").unwrap();
    assert_eq!((outer.rect.w, outer.rect.h), (56., 22.));
    assert_eq!((inner.rect.x, inner.rect.y), (18., 6.));
    assert!(outer.contains(1., 1.));
    let root = Element::row(vec![Element::empty().fill().on_click(1)])
        .padding_xy(18., 6.)
        .fill();
    let scene = layout.build(&root, 200., 100., &Interaction::default(), 0.);
    assert_eq!((scene.hits[0].rect.w, scene.hits[0].rect.h), (164., 88.));
}
#[test]
fn widget_drag_is_continuous_until_release_then_snaps_and_clamps() {
    use lucent_domain::Placement;
    let mut app = fixture(0, false);
    app.viewport = (640., 580.);
    app.settings
        .positions
        .insert("calendar".into(), Placement { x: 20., y: 110. });
    let send = |app: &mut Desktop, finished| {
        app.update(
            Message::MoveWidget(
                "calendar".into(),
                DragEvent {
                    dx: 181.,
                    dy: 73.,
                    finished,
                },
            ),
            &mut Effects::default(),
        )
    };
    send(&mut app, false);
    assert_eq!(
        app.settings.positions["calendar"],
        Placement { x: 201., y: 183. }
    );
    send(&mut app, true);
    assert_eq!(
        app.settings.positions["calendar"],
        Placement { x: 208., y: 176. }
    );
    assert!(app.drag_origins.is_empty());
    app.update(
        Message::MoveWidget(
            "calendar".into(),
            DragEvent {
                dx: 9999.,
                dy: 9999.,
                finished: true,
            },
        ),
        &mut Effects::default(),
    );
    assert_eq!(
        app.settings.positions["calendar"],
        Placement { x: 370., y: 304. }
    );
    assert!(lucent_usecases::snap_placement(Placement { x: 0., y: 0. }, 0.).is_err());
}
#[test]
fn catalog_uses_desktop_identity_and_every_icon_rasterizes() {
    use lucent_design::app_icons::{ALL, lookup};
    assert_eq!(lookup("foot.desktop").unwrap().id, "terminal");
    assert_eq!(lookup("foot-server.desktop").unwrap().id, "terminal-server");
    assert!(lookup("my-foot-wrapper.desktop").is_none());
    assert!(lookup("Foot").is_none());
    let mut pixels = std::collections::BTreeSet::new();
    for asset in ALL {
        let image = lucent_services::images::svg(asset.svg, 64, asset.id.into()).unwrap();
        assert!(image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
        assert!(
            image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[0] == p[1] && p[1] == p[2]),
            "artwork remains monochrome before theme tinting"
        );
        assert!(
            pixels.insert(image.rgba.clone()),
            "each app pictogram is distinct"
        );
    }
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
            ("icon-catalog", Mode::Apps, 9, 0, "", 1., 640.),
            ("bar", Mode::Apps, 9, 0, "", 1., 1280.),
            ("notification", Mode::Apps, 0, 0, "", 1., 640.),
            ("notification-history", Mode::Apps, 0, 0, "", 1., 390.),
            ("lock", Mode::Apps, 0, 0, "", 1., 640.),
            ("greeter", Mode::Apps, 0, 0, "", 1., 640.),
        ] {
            if light && matches!(name, "lock" | "greeter") {
                continue;
            }
            let mut app = fixture(count, light);
            app.update(
                Message::Resize("dock", width, 580.),
                &mut Effects::default(),
            );
            app.update(Message::Mode(mode), &mut Effects::default());
            app.update(Message::Select(selected), &mut Effects::default());
            app.query = query.into();
            let scene = if matches!(name, "lock" | "greeter") {
                let mode = if name == "lock" {
                    lucent_session::Mode::Lock
                } else {
                    lucent_session::Mode::Login
                };
                let mut session = lucent_session::SessionScreen::new(
                    mode,
                    "fixture".into(),
                    Arc::new(crate::test_ports::Fake::default()),
                );
                if name == "lock" {
                    session.update(
                        lucent_session::Message::Prompt(lucent_domain::AuthPrompt {
                            kind: lucent_domain::PromptKind::Secret,
                            text: "Password:".into(),
                        }),
                        &mut Effects::default(),
                    );
                    session.update(
                        lucent_session::Message::Key(Key::Text("fixture".into())),
                        &mut Effects::default(),
                    );
                }
                let cx = ViewContext {
                    surface: "lock",
                    width,
                    height: 580.,
                    now,
                };
                let tree = session.view(&cx).map(|_| Message::Quit);
                layout.build(&tree, width, 580., &Interaction::default(), now)
            } else if name.starts_with("notification") {
                app.notifications.snapshot=lucent_domain::NotificationSnapshot {active:vec![lucent_domain::Notification {id:1,app:"Lucent fixture".into(),summary:"A notification rendered by our framework".into(),body:"Shared design tokens, measured text wrapping, native actions and keyboard focus.".into(),actions:vec![lucent_domain::NotificationAction {id:"default".into(),label:"Open fixture".into()}],critical:false,resident:false,transient:false,timeout_ms:0}],..Default::default()};
                app.notifications.ready = true;
                app.notifications.history_open = name.ends_with("history");
                let cx = ViewContext {
                    surface: "notifications",
                    width,
                    height: 580.,
                    now,
                };
                layout.build(&app.view(&cx), width, 580., &Interaction::default(), now)
            } else if name == "icon-catalog" {
                let icons = lucent_design::app_icons::ALL
                    .iter()
                    .map(|asset| {
                        let image = lucent_services::images::svg(
                            asset.svg,
                            128,
                            format!("catalog:{}", asset.id),
                        )
                        .unwrap();
                        Element::<Message>::column(vec![
                            Element::image(image).tint(app.ink()).size(40., 40.),
                            app.label(asset.id, 9.).size(76., 16.).align(Align::Center),
                        ])
                        .gap(4.)
                        .align(Align::Center)
                    })
                    .collect();
                let root = Element::grid(8, icons).gap(8.).padding(12.).fill();
                layout.build(&root, width, 580., &Interaction::default(), now)
            } else if name == "bar" {
                app.compositor.workspaces = (1..=6)
                    .map(|id| lucent_domain::Workspace {
                        id,
                        name: id.to_string(),
                        windows: 0,
                        active: id == 1,
                    })
                    .collect();
                let cx = ViewContext {
                    surface: "bar",
                    width,
                    height: 580.,
                    now,
                };
                layout.build(&app.view(&cx), width, 580., &Interaction::default(), now)
            } else {
                scene(&app, &layout, &mut Interaction::default(), now)
            };
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

#[test]
fn selection_fill_and_outline_move_together_on_the_first_frame() {
    let mut app = fixture(9, false);
    let layout = Layout::new(fonts(&app));
    for (step, index) in [1, 5, 2, 8, 0].into_iter().enumerate() {
        let now = 1. + step as f64 * 0.01;
        app.update(
            Message::Select(index),
            &mut Effects {
                now,
                ..Default::default()
            },
        );
        let scene = scene(&app, &layout, &mut Interaction::default(), now);
        let row = scene
            .hits
            .iter()
            .find(|h| h.id == format!("result-{index}"))
            .unwrap();
        assert!(scene.paint.iter().any(|p|matches!(p,Paint::Shape {rect,color,..} if *rect==row.rect && *color==app.widget_color())),"selected fill must be immediate");
        assert!(
            scene
                .paint
                .iter()
                .any(|p| matches!(p,Paint::Outline {rect,..} if *rect==row.rect)),
            "fill and outline must share geometry"
        );
    }
}
#[test]
fn desktop_actions_use_injected_ports_without_executing_host_commands() {
    let fake = Arc::new(crate::test_ports::Fake::default());
    let mut app = Desktop::new(crate::test_ports::with(fake.clone()));
    for action in [
        crate::desktop::Action::VolumeUp,
        crate::desktop::Action::PlayPause,
        crate::desktop::Action::Lock,
    ] {
        let mut effects = Effects::default();
        app.update(Message::Action(action), &mut effects);
        for task in effects.tasks {
            app.update(task(), &mut Effects::default());
        }
    }
    assert_eq!(*fake.calls.lock().unwrap(), ["audio", "media", "lock"]);
}

#[test]
fn notification_controls_fit_at_normal_and_narrow_widths() {
    let mut app = fixture(0, false);
    app.notifications.update(crate::notifications::Message::Snapshot(Ok(lucent_domain::NotificationSnapshot {active:vec![lucent_domain::Notification {id:1,app:"Fixture".into(),summary:"A long notification title that wraps onto two lines".into(),body:"A body with enough text to exercise wrapping and leave room for actions below the text.".repeat(3),actions:(0..8).map(|n|lucent_domain::NotificationAction {id:n.to_string(),label:format!("Action {n}")}).collect(),critical:false,resident:false,transient:false,timeout_ms:0}],..Default::default()})),&mut Effects::default());
    let layout = Layout::new(fonts(&app));
    for width in [390., 1920.] {
        for history in [false, true] {
            app.notifications.history_open = history;
            let cx = ViewContext {
                surface: "notifications",
                width,
                height: 720.,
                now: 0.,
            };
            let tree = app.view(&cx);
            let scene = layout.build(&tree, width, 720., &Interaction::default(), 0.);
            for hit in scene.hits {
                assert!(
                    fully_visible(hit.rect, hit.clip),
                    "cropped notification control {}: {:?} in {:?}",
                    hit.id,
                    hit.rect,
                    hit.clip
                );
                assert!(hit.rect.y + hit.rect.h <= 720.);
            }
        }
    }
}
