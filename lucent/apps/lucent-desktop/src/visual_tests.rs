//! Deterministic fixtures use the production layout, fonts, shaders and Vulkan renderer.
//! Run ignored snapshots explicitly; no session, D-Bus service or compositor is needed.
use crate::desktop::{Desktop, Message, Mode};
use lucent_api::*;
use lucent_domain::{AppId, Application as App, ClockSnapshot, Date, LaunchCommand};
use lucent_ui::{Interaction, Layout, Paint, Scene};
use std::{path::PathBuf, sync::Arc};

fn fixture(count: usize, light: bool) -> Desktop {
    let mut app = Desktop::new(crate::test_adapters::adapters());
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
fn launcher_size_preserves_selection_and_limits_at_every_preset() {
    use lucent_domain::LauncherSize;
    let mut app = fixture(50, false);
    let layout = Layout::new(fonts(&app));
    for viewport in [(1920., 1080.), (1366., 768.), (390., 580.), (320., 360.)] {
        app.update(
            Message::Resize("dock", viewport.0, viewport.1),
            &mut Effects::default(),
        );
        for size in crate::shell_layout::LAUNCHER_PRESETS
            .map(|(_, size)| size)
            .into_iter()
            .chain([
                LauncherSize {
                    width: Some(777),
                    max_height: Some(699),
                },
                LauncherSize {
                    width: Some(u32::MAX),
                    max_height: Some(u32::MAX),
                },
                LauncherSize {
                    width: Some(320),
                    max_height: Some(224),
                },
            ])
        {
            app.update(Message::Mode(Mode::Apps), &mut Effects::default());
            app.update(Message::Select(49), &mut Effects::default());
            app.update(Message::LauncherSize(size), &mut Effects::default());
            let rendered = scene(&app, &layout, &mut Interaction::default(), 1.);
            let panel = rendered
                .hits
                .iter()
                .find(|h| h.id == "dock-panel")
                .unwrap()
                .rect;
            assert!(panel.x >= 16. && panel.x + panel.w <= viewport.0 - 16.);
            let top =
                lucent_design::component::panel::BAR_HEIGHT + lucent_design::layout::SECTION_GAP;
            assert!(panel.y >= top && panel.y + panel.h <= viewport.1 - 16.);
            assert_eq!(panel.w % 32., 0.);
            assert_eq!(panel.h % 16., 0.);
            let last = rendered
                .hits
                .iter()
                .find(|h| h.id == "result-49")
                .expect("selected last result stays visible when resized");
            assert!(fully_visible(last.rect, last.clip));
            for hit in rendered
                .hits
                .iter()
                .filter(|h| h.id.starts_with("result-") || h.id == "launcher-search")
            {
                assert!(
                    fully_visible(hit.rect, hit.clip),
                    "cropped {} for {size:?} at {viewport:?}",
                    hit.id
                );
            }
            app.update(Message::Mode(Mode::Themes), &mut Effects::default());
            let rendered = scene(&app, &layout, &mut Interaction::default(), 1.);
            for id in [
                "theme-dark",
                "theme-light",
                "launcher-size-small",
                "launcher-size-default",
                "launcher-size-large",
            ] {
                let hit = rendered.hits.iter().find(|h| h.id == id).unwrap();
                assert!(
                    fully_visible(hit.rect, hit.clip),
                    "cropped {id} for {size:?} at {viewport:?}"
                );
                assert!(hit.rect.h >= 32.);
            }
        }
    }
    app.update(
        Message::Resize("dock", 1920., 1080.),
        &mut Effects::default(),
    );
    app.update(
        Message::LauncherSize(LauncherSize::default()),
        &mut Effects::default(),
    );
    app.update(Message::Mode(Mode::Apps), &mut Effects::default());
    let rendered = scene(&app, &layout, &mut Interaction::default(), 1.);
    let panel = rendered
        .hits
        .iter()
        .find(|h| h.id == "dock-panel")
        .unwrap()
        .rect;
    assert_eq!((panel.w, panel.h), (640., 640.));
    assert_eq!(
        rendered
            .hits
            .iter()
            .filter(|h| h.id.starts_with("result-"))
            .count(),
        10
    );
}

#[test]
fn launcher_size_commands_keyboard_presets_and_settings_roundtrip() {
    use lucent_domain::{DesktopSettings, LauncherSize};
    let mut app = fixture(50, false);
    assert!(
        app.command("launcher size 800 720").is_err(),
        "do not promise a saved change before settings load"
    );
    app.settings_writable = true;
    for command in [
        "launcher size",
        "launcher size 0 720",
        "launcher size 800 -1",
        "launcher size 800 NaN",
        "launcher size 800 720 extra",
        "launcher size 319 224",
        "launcher size 320 223",
    ] {
        assert!(app.command(command).is_err(), "accepted {command}");
    }
    let message = app.command("launcher size 777 699").unwrap().unwrap();
    let mut effects = Effects::default();
    app.update(message, &mut effects);
    assert_eq!(
        app.settings.launcher,
        LauncherSize {
            width: Some(777),
            max_height: Some(699)
        }
    );
    assert!(effects.redraw.contains("dock"));
    assert_eq!(
        effects.tasks.len(),
        1,
        "save through the injected settings adapter"
    );
    let data = serde_json::to_string(&app.settings).unwrap();
    let restored: DesktopSettings = serde_json::from_str(&data).unwrap();
    assert_eq!(restored, app.settings);
    let legacy: DesktopSettings =
        serde_json::from_str(r#"{"version":1,"light":true,"notes":"keep my notes"}"#).unwrap();
    assert_eq!(legacy.launcher, LauncherSize::default());
    assert!(legacy.light);
    assert_eq!(legacy.notes, "keep my notes");
    app.update(Message::Mode(Mode::Themes), &mut Effects::default());
    for _ in 0..4 {
        key(&mut app, Key::Down);
    }
    key(&mut app, Key::Enter);
    assert_eq!(
        app.settings.launcher,
        crate::shell_layout::LAUNCHER_PRESETS[2].1
    );
    key(&mut app, Key::Up);
    key(&mut app, Key::Enter);
    assert_eq!(app.settings.launcher, LauncherSize::default());
    app.update(
        Message::LauncherSize(restored.launcher),
        &mut Effects::default(),
    );
    let reset = app.command("launcher size reset").unwrap().unwrap();
    app.update(reset, &mut Effects::default());
    assert_eq!(app.settings.launcher, LauncherSize::default());
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
    let left = first.rect.x - panel.rect.x;
    let right = panel.rect.x + panel.rect.w - last.rect.x - last.rect.w;
    assert_eq!(
        left, right,
        "dock padding must stay symmetric after grid sizing"
    );
    assert!(left >= lucent_design::component::dock::CONTENT_INSET);
    assert_eq!(left % lucent_design::layout::UNIT, 0.);
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
fn drag_grid_matches_snap_spacing_and_never_captures_input() {
    let mut app = fixture(0, false);
    let layout = Layout::new(fonts(&app));
    let cx = ViewContext {
        surface: "widgets",
        width: 640.,
        height: 580.,
        now: 1.,
    };
    let idle = layout.build(
        &app.view(&cx),
        cx.width,
        cx.height,
        &Interaction::default(),
        cx.now,
    );
    app.update(
        Message::MoveWidget(
            "calendar".into(),
            DragEvent {
                dx: 25.,
                dy: 35.,
                finished: false,
            },
        ),
        &mut Effects::default(),
    );
    let dragging = layout.build(
        &app.view(&cx),
        cx.width,
        cx.height,
        &Interaction::default(),
        cx.now,
    );
    let step = lucent_design::component::widget_layout::GRID_STEP;
    let width = lucent_design::component::widget_grid::LINE_WIDTH;
    let lines: Vec<_> = dragging
        .paint
        .iter()
        .filter_map(|p| match p {
            Paint::Shape {
                rect,
                shadow: false,
                ..
            } if rect.w == width && rect.h == cx.height
                || rect.h == width && rect.w == cx.width =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        lines.len(),
        (cx.width / step).ceil() as usize + (cx.height / step).ceil() as usize
    );
    for line in &lines {
        assert_eq!(line.x % step, 0.);
        assert_eq!(line.y % step, 0.);
    }
    assert_eq!(dragging.hits.len(), idle.hits.len());
    assert_eq!(dragging.regions.len(), idle.regions.len());
    app.update(
        Message::MoveWidget(
            "calendar".into(),
            DragEvent {
                dx: 25.,
                dy: 35.,
                finished: true,
            },
        ),
        &mut Effects::default(),
    );
    let released = layout.build(
        &app.view(&cx),
        cx.width,
        cx.height,
        &Interaction::default(),
        cx.now,
    );
    assert_eq!(released.paint.len(), idle.paint.len());
    assert_eq!(dragging.paint.len(), released.paint.len() + lines.len());
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
            ("launcher-size-default", Mode::Apps, 30, 29, "", 1., 1024.),
            ("launcher-size-small", Mode::Apps, 30, 29, "", 1., 1024.),
            ("launcher-size-large", Mode::Apps, 30, 29, "", 1., 1024.),
            ("launcher-size-custom", Mode::Apps, 30, 29, "", 1., 1024.),
            ("themes-small-screen", Mode::Themes, 9, 4, "", 1., 320.),
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
            ("wallpapers", Mode::Wallpapers, 9, 0, "", 1., 1280.),
            ("icon-catalog", Mode::Apps, 9, 0, "", 1., 640.),
            ("bar", Mode::Apps, 9, 0, "", 1., 1280.),
            ("bar-dense", Mode::Apps, 9, 0, "", 1., 1920.),
            ("bar-narrow", Mode::Apps, 9, 0, "", 1., 390.),
            ("dock", Mode::Apps, 9, 0, "", 1., 640.),
            ("notification", Mode::Apps, 0, 0, "", 1., 640.),
            ("notification-history", Mode::Apps, 0, 0, "", 1., 390.),
            ("lock", Mode::Apps, 0, 0, "", 1., 640.),
            ("greeter", Mode::Apps, 0, 0, "", 1., 640.),
            ("authorization", Mode::Apps, 0, 0, "", 1., 640.),
            ("osd", Mode::Apps, 0, 0, "", 1., 640.),
            ("menu", Mode::Apps, 0, 0, "", 1., 960.),
            ("menu-last-narrow", Mode::Apps, 0, 22, "", 1., 390.),
            ("menu-empty", Mode::Apps, 0, 0, "no such action", 1., 640.),
            ("menu-input", Mode::Apps, 0, 0, "A reminder", 1., 640.),
            ("widget-drag-grid", Mode::Apps, 0, 0, "", 1., 960.),
            ("menu-submenu", Mode::Apps, 0, 0, "", 1., 640.),
        ] {
            let height = if name.starts_with("launcher-size-") {
                1080.
            } else if name == "themes-small-screen" {
                360.
            } else {
                580.
            };
            let mut app = fixture(count, light);
            let preference = match name {
                "launcher-size-small" => crate::shell_layout::LAUNCHER_PRESETS[0].1,
                "launcher-size-large" => crate::shell_layout::LAUNCHER_PRESETS[2].1,
                "launcher-size-custom" => lucent_domain::LauncherSize {
                    width: Some(777),
                    max_height: Some(699),
                },
                _ => lucent_domain::LauncherSize::default(),
            };
            app.update(Message::LauncherSize(preference), &mut Effects::default());
            app.update(
                Message::Resize("dock", width, height),
                &mut Effects::default(),
            );
            app.update(Message::Mode(mode), &mut Effects::default());
            app.update(Message::Select(selected), &mut Effects::default());
            app.query = query.into();
            if name == "wallpapers" {
                app.wallpapers = wallpaper_fixtures();
            }
            let scene = if name == "widget-drag-grid" {
                app.settings.visible_widgets = vec!["calendar".into(), "weather".into()];
                app.update(
                    Message::MoveWidget(
                        "calendar".into(),
                        DragEvent {
                            dx: 151.,
                            dy: 73.,
                            finished: false,
                        },
                    ),
                    &mut Effects::default(),
                );
                layout.build(
                    &app.view(&ViewContext {
                        surface: "widgets",
                        width,
                        height,
                        now,
                    }),
                    width,
                    height,
                    &Interaction::default(),
                    now,
                )
            } else if name.starts_with("menu") {
                let names = if name == "menu-submenu" {
                    [
                        "Lock",
                        "Suspend",
                        "Hibernate",
                        "Logout",
                        "Reboot",
                        "Shutdown",
                        "Unavailable action",
                        "Another action",
                    ]
                } else {
                    [
                        "Keybindings",
                        "Terminal",
                        "Browser",
                        "File manager",
                        "System menu",
                        "Theme menu",
                        "Full screen",
                        "Toggle window floating/tiling",
                    ]
                };
                let (mut menu, _) = lucent_menu::Menu::new(lucent_menu::Request {
                    prompt: if name == "menu-submenu" {
                        "System"
                    } else if name == "menu-input" {
                        "Reminder"
                    } else {
                        "Keybindings"
                    }
                    .into(),
                    mode: if name == "menu-input" {
                        lucent_menu::Mode::Input
                    } else {
                        lucent_menu::Mode::Select
                    },
                    entries: (0..if name == "menu-submenu" { 6 } else { 23 })
                        .map(|i| lucent_domain::MenuEntry {
                            label: names[i % names.len()].into(),
                            detail: if name == "menu-submenu" {
                                if i == 2 {
                                    "Unavailable on this system"
                                } else {
                                    ""
                                }
                                .into()
                            } else {
                                format!("SUPER CTRL + {}", i + 1)
                            },
                            value: i.to_string(),
                            disabled: name == "menu-submenu" && i == 2,
                        })
                        .collect(),
                    width: Some(800.),
                    max_height: Some(500.),
                    light,
                    back: name == "menu-submenu",
                });
                menu.init(&mut Effects::default());
                for msg in [
                    lucent_menu::Message::Resize(width, height),
                    lucent_menu::Message::Query(query.into()),
                    lucent_menu::Message::Select(selected),
                ] {
                    menu.update(msg, &mut Effects::default());
                }
                let tree = menu
                    .view(&ViewContext {
                        surface: "menu",
                        width,
                        height,
                        now,
                    })
                    .map(|_| Message::Quit);
                let mut input = Interaction::default();
                input.synchronize(&layout.build(&tree, width, height, &input, now));
                layout.build(&tree, width, height, &input, now)
            } else if name == "osd" {
                app.update(
                    Message::Osd(crate::osd::Feedback {
                        message: "Output volume".into(),
                        value: "70".into(),
                        progress_text: "70%".into(),
                        max: "100".into(),
                    }),
                    &mut Effects::default(),
                );
                layout.build(
                    &app.view(&ViewContext {
                        surface: "osd",
                        width,
                        height,
                        now,
                    }),
                    width,
                    height,
                    &Interaction::default(),
                    now,
                )
            } else if matches!(name, "lock" | "greeter" | "authorization") {
                let mode = if name == "lock" {
                    lucent_session::Mode::Lock
                } else if name == "authorization" {
                    lucent_session::Mode::Authorization
                } else {
                    lucent_session::Mode::Login
                };
                let mut session = lucent_session::SessionScreen::new(
                    mode,
                    "fixture".into(),
                    Arc::new(crate::test_adapters::Fake::default()),
                )
                .with_theme(light)
                .with_authorization(
                    "Authentication is required to change this system setting".into(),
                );
                // Original deterministic landscape gradient, never a private VM image.
                let mut pixels = Vec::new();
                for y in 0..180u32 {
                    for x in 0..320u32 {
                        pixels.extend_from_slice(&[
                            (25 + x / 5) as u8,
                            (35 + y / 3) as u8,
                            (80 + (x + y) / 5) as u8,
                            255,
                        ]);
                    }
                }
                session.update(
                    lucent_session::Message::Wallpaper(Some(Arc::new(ImageData {
                        key: "session-wallpaper-fixture".into(),
                        width: 320,
                        height: 180,
                        rgba: pixels,
                    }))),
                    &mut Effects::default(),
                );
                if matches!(name, "lock" | "authorization") {
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
                    height,
                    now,
                };
                let tree = session.view(&cx).map(|_| Message::Quit);
                layout.build(&tree, width, height, &Interaction::default(), now)
            } else if name.starts_with("notification") {
                app.notifications.snapshot=lucent_domain::NotificationSnapshot {active:vec![lucent_domain::Notification {id:1,app:"Lucent fixture".into(),summary:"A notification rendered by our framework".into(),body:"Shared design tokens, measured text wrapping, native actions and keyboard focus.".into(),actions:vec![lucent_domain::NotificationAction {id:"default".into(),label:"Open fixture".into()}],critical:false,resident:false,transient:false,timeout_ms:0}],..Default::default()};
                app.notifications.ready = true;
                app.notifications.history_open = name.ends_with("history");
                let cx = ViewContext {
                    surface: "notifications",
                    width,
                    height,
                    now,
                };
                layout.build(&app.view(&cx), width, height, &Interaction::default(), now)
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
                layout.build(&root, width, height, &Interaction::default(), now)
            } else if name.starts_with("bar") {
                app.media.title = "A long media title that must stay inside its own capsule".into();
                app.system.volume = Some(100);
                app.compositor.workspaces = (1..=8)
                    .map(|id| lucent_domain::Workspace {
                        id,
                        name: id.to_string(),
                        windows: 0,
                        active: id == 8,
                    })
                    .collect();
                let cx = ViewContext {
                    surface: "bar",
                    width,
                    height,
                    now,
                };
                layout.build(&app.view(&cx), width, height, &Interaction::default(), now)
            } else {
                if name == "dock" {
                    app.update(Message::CloseLauncher, &mut Effects::default());
                }
                scene(&app, &layout, &mut Interaction::default(), now)
            };
            let mut paint = vec![Paint::Shape {
                rect: Rect::new(0., 0., width, height),
                clip: Rect::new(0., 0., width, height),
                color: Color::hex(if light { 0xc9c3b9 } else { 0x28272d }),
                radius: 0.,
                shadow: false,
            }];
            paint.extend(scene.paint);
            for scale in [1, 2] {
                let id = format!("{name}-{}-{scale}x", if light { "light" } else { "dark" });
                let (w, h) = (width as u32 * scale, height as u32 * scale);
                let actual = canvas
                    .snapshot(width as u32, height as u32, scale, &paint)
                    .unwrap();
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
fn desktop_actions_use_injected_adapters_without_executing_host_commands() {
    let fake = Arc::new(crate::test_adapters::Fake::default());
    let mut app = Desktop::new(crate::test_adapters::with(fake.clone()));
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

#[test]
fn settled_shell_geometry_obeys_grid_and_contains_its_controls() {
    use lucent_design::{component, layout as grid, space};
    let mut app = fixture(20, false);
    app.wallpapers = wallpaper_fixtures();
    let layout = Layout::new(fonts(&app));
    let aligned = |value: f32, step: f32| {
        assert!(
            (value / step - (value / step).round()).abs() < 0.001,
            "{value} is off the {step}px grid"
        );
    };
    for (width, height) in [
        (320., 360.),
        (390., 580.),
        (640., 580.),
        (1366., 768.),
        (1920., 1080.),
    ] {
        app.update(
            Message::Resize("dock", width, height),
            &mut Effects::default(),
        );
        for mode in Mode::ALL {
            app.update(Message::Mode(mode), &mut Effects::default());
            app.update(Message::Select(19), &mut Effects::default());
            let rendered = scene(&app, &layout, &mut Interaction::default(), 1.);
            let panel = rendered
                .hits
                .iter()
                .find(|h| h.id == "dock-panel")
                .unwrap()
                .rect;
            for value in [panel.x, panel.y, panel.w, panel.h] {
                aligned(value, grid::SHELL_STEP);
            }
            assert!(panel.x >= grid::SHELL_INSET && panel.x + panel.w <= width - grid::SHELL_INSET);
            assert!(panel.y >= component::panel::BAR_HEIGHT + grid::SECTION_GAP);
            assert!(panel.y + panel.h <= height - grid::SHELL_INSET);
            for hit in &rendered.hits {
                if hit.id.starts_with("mode-") {
                    assert!(
                        fully_visible(hit.rect, panel),
                        "{} outside {mode:?} header at {width}",
                        hit.id
                    );
                    for value in [hit.rect.x, hit.rect.y, hit.rect.w, hit.rect.h] {
                        aligned(value, grid::CONTROL_STEP);
                    }
                }
                if hit.id.starts_with("result-")
                    || hit.id.starts_with("command-")
                    || hit.id.starts_with("toggle-")
                    || hit.id == "reset-layout"
                    || hit.id == "apply-wallpaper"
                    || hit.id == "wallpaper-previous"
                    || hit.id == "wallpaper-next"
                {
                    assert!(
                        fully_visible(hit.rect, hit.clip),
                        "clipped {} in {mode:?} at {width}x{height}",
                        hit.id
                    );
                    for value in [hit.rect.x, hit.rect.y, hit.rect.w, hit.rect.h] {
                        aligned(value, grid::SHELL_STEP);
                    }
                }
            }
            if mode == Mode::Apps {
                let input = rendered
                    .hits
                    .iter()
                    .find(|h| h.id == "launcher-search")
                    .unwrap()
                    .rect;
                let field = rendered
                    .paint
                    .iter()
                    .find_map(|p| match p {
                        Paint::Shape {
                            rect,
                            color,
                            shadow: false,
                            ..
                        } if *color == app.widget_color()
                            && rect.h == component::input::HEIGHT
                            && fully_visible(input, *rect) =>
                        {
                            Some(*rect)
                        }
                        _ => None,
                    })
                    .expect("painted search field");
                for value in [field.x, field.y, field.w, field.h] {
                    aligned(value, grid::SHELL_STEP);
                }
                assert_eq!(field.x - panel.x, component::dock::CONTENT_INSET);
                assert_eq!(
                    panel.y + panel.h - field.y - field.h,
                    component::dock::CONTENT_INSET
                );
                assert!(
                    rendered.hits.iter().any(|h| h.id == "result-19"),
                    "last result must remain reachable at {height}px tall"
                );
            }
            if mode == Mode::Widgets {
                assert!(rendered.hits.iter().any(|h| h.id == "toggle-timer"));
                assert!(rendered.hits.iter().any(|h| h.id == "reset-layout"));
            }
            if mode == Mode::Wallpapers {
                let controls: Vec<_> = ["wallpaper-previous", "apply-wallpaper", "wallpaper-next"]
                    .into_iter()
                    .map(|id| rendered.hits.iter().find(|h| h.id == id).unwrap().rect)
                    .collect();
                for pair in controls.windows(2) {
                    assert!(pair[1].x - pair[0].x - pair[0].w >= grid::SECTION_GAP);
                }
            }
            if mode == Mode::Themes {
                let a = rendered
                    .hits
                    .iter()
                    .find(|h| h.id == "theme-dark")
                    .unwrap()
                    .rect;
                let b = rendered
                    .hits
                    .iter()
                    .find(|h| h.id == "theme-light")
                    .unwrap()
                    .rect;
                assert_eq!(a.w, b.w);
                assert_eq!(b.x - a.x - a.w, grid::SECTION_GAP);
                assert_eq!(a.x - panel.x, panel.x + panel.w - b.x - b.w);
            }
        }
        app.update(Message::CloseLauncher, &mut Effects::default());
        let rendered = scene(&app, &layout, &mut Interaction::default(), 1.);
        let panel = rendered
            .hits
            .iter()
            .find(|h| h.id == "dock-panel")
            .unwrap()
            .rect;
        for value in [panel.x, panel.y, panel.w, panel.h] {
            aligned(value, grid::SHELL_STEP);
        }
        assert!(panel.x >= grid::SHELL_INSET && panel.x + panel.w <= width - grid::SHELL_INSET);
        let slots: Vec<_> = rendered
            .hits
            .iter()
            .filter(|h| h.id.starts_with("dock-") && h.id != "dock-panel")
            .collect();
        for pair in slots.windows(2) {
            assert_eq!(pair[1].rect.x - pair[0].rect.x - pair[0].rect.w, space::SM);
        }
    }
}

#[test]
fn bar_capsules_share_grid_do_not_overlap_and_keep_active_workspace_visible() {
    use lucent_design::{component, layout as grid};
    let mut app = fixture(0, false);
    let layout = Layout::new(fonts(&app));
    let surface = app.surfaces().into_iter().find(|s| s.id == "bar").unwrap();
    // The compositor owns the gap below the visible capsules. Reserving that
    // gap again here pushes tiled windows away from the shell spacing grid.
    let visible_bottom = component::bar::TOP + component::pill::HEIGHT;
    assert_eq!(surface.exclusive_zone as f32, visible_bottom);
    assert_eq!(surface.height as f32, visible_bottom);
    assert_eq!(component::window::GAP_OUT, grid::SHELL_INSET);
    assert_eq!(component::window::GAP_IN * 2., grid::SECTION_GAP);
    for width in [320., 390., 640., 1024., 1280., 1366., 1920.] {
        for count in 1..=8 {
            app.compositor.workspaces = (1..=count)
                .map(|id| lucent_domain::Workspace {
                    id,
                    name: id.to_string(),
                    windows: 0,
                    active: id == count,
                })
                .collect();
            let geometry = crate::shell_layout::BarLayout::new(width, count as usize);
            let mut groups = vec![geometry.workspaces, geometry.system];
            groups.extend(geometry.media);
            groups.extend(geometry.clock);
            let cx = ViewContext {
                surface: "bar",
                width,
                height: component::panel::BAR_HEIGHT,
                now: 1.,
            };
            let rendered = layout.build(
                &app.view(&cx),
                width,
                cx.height,
                &Interaction::default(),
                1.,
            );
            for rect in &groups {
                for value in [rect.x, rect.y, rect.w, rect.h] {
                    assert_eq!(value % grid::SHELL_STEP, 0.);
                }
                assert!(
                    rect.x >= grid::SHELL_INSET && rect.x + rect.w <= width - grid::SHELL_INSET
                );
                assert!(rendered.paint.iter().any(|p| matches!(p, Paint::Shape { rect: painted, color, .. } if painted == rect && *color == app.surface_color())), "view drifted from calculated capsule {rect:?}");
            }
            groups.sort_by(|a, b| a.x.total_cmp(&b.x));
            for pair in groups.windows(2) {
                assert!(pair[1].x - pair[0].x - pair[0].w >= grid::SECTION_GAP);
            }
            assert!(
                rendered
                    .hits
                    .iter()
                    .any(|h| h.id == format!("workspace-{count}")),
                "active workspace disappeared at {width}"
            );
            for hit in &rendered.hits {
                assert!(
                    groups.iter().any(|g| fully_visible(hit.rect, *g)),
                    "bar control {} escaped its capsule",
                    hit.id
                );
            }
        }
    }
}

fn wallpaper_fixtures() -> Vec<lucent_domain::Wallpaper> {
    ["Dawn", "Day", "Dusk"]
        .into_iter()
        .map(|name| lucent_domain::Wallpaper {
            path: format!("fixture:{name}"),
            name: name.into(),
        })
        .collect()
}

#[test]
fn theme_intent_saves_preferences_then_propagates_through_the_port() {
    let fake = Arc::new(crate::test_adapters::Fake::default());
    let mut app = Desktop::new(crate::test_adapters::with(fake.clone()));
    assert!(app.command("theme light").is_err());
    app.settings_writable = true;
    app.settings.notes = "Keep these notes".into();
    let mut effects = Effects::default();
    app.update(app.command("theme light").unwrap().unwrap(), &mut effects);
    assert!(app.settings.light);
    assert_eq!(app.settings.notes, "Keep these notes");
    for task in effects.tasks {
        app.update(task(), &mut Effects::default());
    }
    assert_eq!(*fake.calls.lock().unwrap(), ["save", "light"]);
    app.update(
        Message::ThemeApplied(Err(lucent_domain::DomainError::Unavailable(
            "Export failed".into(),
        ))),
        &mut Effects::default(),
    );
    app.update(Message::Completed(Ok(())), &mut Effects::default());
    assert!(
        !app.theme_error.is_empty(),
        "unrelated successful effects cannot erase the export error"
    );
}

#[test]
fn feedback_stops_scheduling_after_expiry_and_has_no_keyboard_grab() {
    let mut app = fixture(0, false);
    app.update(
        Message::Osd(crate::osd::Feedback {
            message: "Volume".into(),
            ..Default::default()
        }),
        &mut Effects::default(),
    );
    let spec = app.surfaces().into_iter().find(|s| s.id == "osd").unwrap();
    assert_eq!(spec.keyboard, Keyboard::None);
    assert!(!spec.capture_all);
    app.update(
        Message::OsdTick,
        &mut Effects {
            now: 2.,
            ..Default::default()
        },
    );
    app.update(
        Message::OsdTick,
        &mut Effects {
            now: 3.,
            ..Default::default()
        },
    );
    assert!(app.osd.feedback.is_none());
    assert!(!app.animating("osd", 3.));
}
