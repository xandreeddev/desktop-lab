//! Deterministic fixtures use the production layout, fonts, shaders and Vulkan renderer.
//! Run ignored snapshots explicitly; no session, D-Bus service or compositor is needed.
use crate::desktop::{Desktop, Message};
use crate::launcher::{self, Mode};
use lucent_api::*;
use lucent_domain::{AppId, Application as App, ClockSnapshot, Date, LaunchCommand};
use lucent_ui::{Interaction, Layout, Paint, Scene};
use std::{path::PathBuf, sync::Arc};

fn fixture(count: usize, light: bool) -> Desktop {
    let mut app = Desktop::new(crate::test_adapters::adapters());
    app.settings.light = light;
    app.viewport = (640., 580.);
    app.clock = ClockSnapshot {
        date: Date::new(2026, 1, 15).expect("valid calendar date"),
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
            id: AppId::new(
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
                ][n % 9],
            )
            .unwrap(),
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
    app.update(
        Message::Catalog(crate::catalog::Message::Observed(Ok(app.apps.clone()))),
        &mut Effects::default(),
    );
    for a in &app.apps {
        if let Some(asset) = lucent_design::app_icons::lookup(a.id.as_str()) {
            app.images.insert(
                format!("app:{}", a.id.as_str()),
                lucent_services::images::svg_mask(
                    asset.svg,
                    128,
                    format!("lucent-app:{}", asset.id),
                )
                .unwrap(),
            );
        }
    }
    app.update(
        Message::Launcher(launcher::Message::Submitted {
            height: 580.,
            now: 0.,
        }),
        &mut Effects::default(),
    );
    app.update(
        Message::Launcher(launcher::Message::Mode(Mode::Apps)),
        &mut Effects::default(),
    );
    app
}

struct UnavailablePreviews(Arc<dyn crate::ports::AssetPort>);
impl crate::ports::AssetPort for UnavailablePreviews {
    fn initial(&self) -> std::collections::BTreeMap<String, Arc<ImageData>> {
        self.0.initial()
    }
    fn application_icons(&self, apps: &[lucent_domain::Application]) -> crate::ports::Images {
        self.0.application_icons(apps)
    }
    fn thumbnail(&self, _: &str) -> lucent_domain::Result<Arc<ImageData>> {
        Err(lucent_domain::DomainError::Failed(
            "Unavailable fixture".into(),
        ))
    }
    fn preview(&self, image: &ImageData) -> Arc<ImageData> {
        self.0.preview(image)
    }
    fn background(
        &self,
        path: &str,
        target: Option<crate::ports::BackgroundTarget>,
    ) -> lucent_domain::Result<Arc<ImageData>> {
        self.0.background(path, target)
    }
}
/// The offscreen canvas is immediately available at the requested full size.
/// Native tests separately exercise the asynchronous configure/submission path.
fn resize_fixture(app: &mut Desktop, width: f32, height: f32) {
    app.update(
        Message::Resize("background", width, height, 1),
        &mut Effects::default(),
    );
    app.update(
        Message::Launcher(launcher::Message::Submitted { height, now: 0. }),
        &mut Effects::default(),
    );
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
            resize_fixture(&mut app, width, 580.);
            for index in [0, count - 1] {
                app.update(
                    Message::Launcher(launcher::Message::Select(index)),
                    &mut Effects::default(),
                );
                let scene = scene(&app, &layout, &mut Interaction::default(), 1.);
                let rows: Vec<_> = scene
                    .hits
                    .iter()
                    .filter(|h| h.id.starts_with("result-"))
                    .collect();
                assert_eq!(rows.len(), count.min(7));
                for row in rows {
                    assert!(
                        fully_visible(row.rect, row.clip.bounds),
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
        resize_fixture(&mut app, viewport.0, viewport.1);
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
            app.update(
                Message::Launcher(launcher::Message::Mode(Mode::Apps)),
                &mut Effects::default(),
            );
            app.update(
                Message::Launcher(launcher::Message::Select(49)),
                &mut Effects::default(),
            );
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
            assert!(fully_visible(last.rect, last.clip.bounds));
            for hit in rendered
                .hits
                .iter()
                .filter(|h| h.id.starts_with("result-") || h.id == "launcher-search")
            {
                assert!(
                    fully_visible(hit.rect, hit.clip.bounds),
                    "cropped {} for {size:?} at {viewport:?}",
                    hit.id
                );
            }
            app.update(
                Message::Launcher(launcher::Message::Mode(Mode::Themes)),
                &mut Effects::default(),
            );
            for index in 0..app.palette_choices().len() {
                app.update(
                    Message::Launcher(launcher::Message::Select(index)),
                    &mut Effects::default(),
                );
                let rendered = scene(&app, &layout, &mut Interaction::default(), 1.);
                for id in [
                    format!("theme-{}", app.palette_choices()[index].id),
                    "launcher-size-small".into(),
                    "launcher-size-default".into(),
                    "launcher-size-large".into(),
                ] {
                    let hit = rendered
                        .hits
                        .iter()
                        .find(|h| h.id == id)
                        .expect("selected palette and size controls remain reachable");
                    assert!(
                        fully_visible(hit.rect, hit.clip.bounds),
                        "cropped {id} for {size:?} at {viewport:?}"
                    );
                    assert!(hit.rect.h >= 32.);
                }
            }
        }
    }
    resize_fixture(&mut app, 1920., 1080.);
    app.update(
        Message::LauncherSize(LauncherSize::default()),
        &mut Effects::default(),
    );
    app.update(
        Message::Launcher(launcher::Message::Mode(Mode::Apps)),
        &mut Effects::default(),
    );
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
    app.update(
        Message::Launcher(launcher::Message::Mode(Mode::Themes)),
        &mut Effects::default(),
    );
    for _ in 0..(app.palette_choices().len() + 3) {
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
    app.launcher.fixture_query(
        "A long input that must keep the caret visible without escaping the field ".repeat(3),
    );
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
            assert!((rect.w / rect.h - data.width() as f32 / data.height() as f32).abs() < 0.01);
        }
        if let Paint::Shape { rect, clip, .. } = paint
            && rect.w == lucent_design::component::input::CARET_WIDTH
        {
            carets += 1;
            assert!(
                fully_visible(*rect, clip.bounds),
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
        assert_eq!(app.launcher.mode(), expected);
    }
    key(&mut app, Key::BackTab);
    assert_eq!(app.launcher.mode(), Mode::Widgets);
    key(&mut app, Key::Down);
    assert_eq!(app.launcher.selected(), 1);
    let before = app.settings.visible_widgets.contains(&"clock".into());
    key(&mut app, Key::Enter);
    assert_ne!(
        before,
        app.settings.visible_widgets.contains(&"clock".into())
    );
    key(&mut app, Key::BackTab);
    assert_eq!(app.launcher.mode(), Mode::Themes);
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
    assert_eq!(interaction.key(&scene, &Key::Enter), vec![1]);
    interaction.key(&scene, &Key::Tab);
    assert_eq!(interaction.key(&scene, &Key::Text(" ".into())), vec![2]);
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
    app.update(
        Message::Launcher(launcher::Message::Mode(Mode::Commands)),
        &mut Effects::default(),
    );
    let layout = Layout::new(fonts(&app));
    let command_scene = scene(&app, &layout, &mut Interaction::default(), 1.);
    let rows: Vec<_> = command_scene
        .hits
        .iter()
        .filter(|h| h.id.starts_with("command-"))
        .collect();
    assert_eq!(rows.len(), crate::actions::COMMANDS.len());
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
            assert!(fully_visible(*rect, row.clip.bounds));
        }
    }
    app.update(
        Message::Launcher(launcher::Message::Close),
        &mut Effects::default(),
    );
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
        .insert("calendar".into(), Placement::new(20., 110.).unwrap());
    let send = |app: &mut Desktop, finished| {
        app.update(
            Message::Widgets(crate::widgets::Message::Move(
                "calendar".into(),
                DragEvent {
                    dx: 181.,
                    dy: 73.,
                    finished,
                },
            )),
            &mut Effects::default(),
        )
    };
    send(&mut app, false);
    assert_eq!(
        app.settings.positions["calendar"],
        Placement::new(20., 110.).unwrap()
    );
    assert_eq!(
        app.widgets.preview_position("calendar"),
        Some(Placement::new(201., 183.).unwrap())
    );
    send(&mut app, true);
    assert_eq!(
        app.settings.positions["calendar"],
        Placement::new(208., 176.).unwrap()
    );
    assert!(!app.widgets.grid_visible());
    send(&mut app, false);
    app.update(
        Message::Widgets(crate::widgets::Message::Move(
            "calendar".into(),
            DragEvent {
                dx: 9999.,
                dy: 9999.,
                finished: true,
            },
        )),
        &mut Effects::default(),
    );
    assert_eq!(
        app.settings.positions["calendar"],
        Placement::new(370., 304.).unwrap()
    );
    assert!(lucent_usecases::snap_placement(Placement::new(0., 0.).unwrap(), 0.).is_err());
}

#[test]
fn unrelated_preference_save_during_drag_cannot_persist_the_preview() {
    use lucent_domain::{DesktopSettings, Placement, Result, SettingsPort};
    #[derive(Default)]
    struct Store(std::sync::Mutex<Vec<DesktopSettings>>);
    impl SettingsPort for Store {
        fn load(&self) -> Result<DesktopSettings> {
            Ok(DesktopSettings::default())
        }
        fn save(&self, settings: &DesktopSettings) -> Result<()> {
            self.0.lock().unwrap().push(settings.clone());
            Ok(())
        }
    }
    let store = Arc::new(Store::default());
    let mut adapters = crate::test_adapters::adapters();
    adapters.settings = store.clone();
    let mut app = Desktop::new(adapters);
    app.settings_writable = true;
    let origin = Placement::new(32., 96.).unwrap();
    app.settings.positions.insert("calendar".into(), origin);
    let movement = |finished| {
        Message::Widgets(crate::widgets::Message::Move(
            "calendar".into(),
            DragEvent {
                dx: 93.,
                dy: 71.,
                finished,
            },
        ))
    };
    let mut effects = Effects::default();
    app.update(movement(false), &mut effects);
    assert!(effects.tasks.is_empty());
    let preview = app.widgets.preview_position("calendar").unwrap();
    assert_ne!(preview, origin);
    app.update(
        Message::Widgets(crate::widgets::Message::Notes("Keep this edit".into())),
        &mut effects,
    );
    while let Some(task) = effects.tasks.pop() {
        app.update(task.run(), &mut effects);
    }
    assert_eq!(store.0.lock().unwrap()[0].positions["calendar"], origin);
    assert_eq!(store.0.lock().unwrap()[0].notes, "Keep this edit");
    assert_eq!(app.widgets.preview_position("calendar"), Some(preview));
    app.update(movement(true), &mut effects);
    while let Some(task) = effects.tasks.pop() {
        app.update(task.run(), &mut effects);
    }
    let stored = store.0.lock().unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(
        stored[1].positions["calendar"],
        lucent_usecases::snap_placement(
            preview,
            lucent_design::component::widget_layout::GRID_STEP
        )
        .unwrap()
    );
    assert_eq!(stored[1].notes, "Keep this edit");
    assert!(!app.widgets.grid_visible());
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
        Message::Widgets(crate::widgets::Message::Move(
            "calendar".into(),
            DragEvent {
                dx: 25.,
                dy: 35.,
                finished: false,
            },
        )),
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
            Paint::Shape { rect, .. }
                if rect.w == width && rect.h == cx.height
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
        Message::Widgets(crate::widgets::Message::Move(
            "calendar".into(),
            DragEvent {
                dx: 25.,
                dy: 35.,
                finished: true,
            },
        )),
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
        let image = lucent_services::images::svg_mask(asset.svg, 64, asset.id.into()).unwrap();
        assert_eq!(image.format(), ImageFormat::Alpha8);
        assert!(image.pixels().iter().any(|a| *a > 0));
        assert!(
            pixels.insert(image.pixels().to_vec()),
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
fn compact_dock_resize_cannot_replace_the_display_viewport() {
    let mut app = Desktop::new(crate::test_adapters::adapters());
    app.update(
        Message::Resize("background", 1920., 1080., 1),
        &mut Effects::default(),
    );
    app.update(
        Message::Resize("dock", 1920., 120., 1),
        &mut Effects::default(),
    );
    assert_eq!(app.viewport, (1920., 1080.));
    app.update(
        Message::Resize("background", f32::NAN, 20., 1),
        &mut Effects::default(),
    );
    assert_eq!(app.viewport, (1920., 1080.));
    let dock = app.surfaces().into_iter().find(|s| s.id == "dock").unwrap();
    assert_eq!(dock.anchor, Anchor::Bottom);
    assert_eq!(dock.height, 120);
    assert_eq!(dock.pointer_input, PointerInput::Content);
}

#[test]
fn bounded_widget_sources_preserve_capture_and_pass_through_preview() {
    let mut app = fixture(9, false);
    resize_fixture(&mut app, 1920., 1080.);
    let initial = app.surfaces();
    assert!(!initial.iter().find(|s| s.id == "widgets").unwrap().visible);
    let group = app
        .widgets
        .surfaces(&app.settings, app.viewport)
        .into_iter()
        .find(|s| s.members.contains(&"calendar"))
        .unwrap();
    let layout = Layout::new(fonts(&app));
    let cx = ViewContext {
        surface: group.id,
        width: group.rect.w,
        height: group.rect.h,
        now: 1.,
    };
    let mut input = Interaction::default();
    let before = layout.build(&app.view(&cx), cx.width, cx.height, &input, cx.now);
    let hit = before
        .hits
        .iter()
        .find(|h| h.id == "widget-calendar")
        .unwrap();
    let (x, y) = (hit.rect.x + 100., hit.rect.y + 20.);
    input.press(&before, x, y);
    for message in input.motion(&before, x + 400., y + 200., cx.now) {
        app.update(message, &mut Effects::default());
    }
    assert!(app.settings.positions.is_empty());
    assert!(app.widgets.grid_visible());
    let during = app.surfaces();
    let preview = during.iter().find(|s| s.id == "widgets").unwrap();
    assert!(preview.visible);
    assert_eq!(preview.pointer_input, PointerInput::None);
    assert_eq!(preview.keyboard, Keyboard::None);
    assert_eq!(
        during.iter().find(|s| s.id == group.id),
        initial.iter().find(|s| s.id == group.id)
    );
    let input_only = layout.build(&app.view(&cx), cx.width, cx.height, &input, cx.now);
    assert!(input_only.paint.is_empty());
    assert_eq!(before.regions, input_only.regions);
    input.synchronize(&input_only);
    let message = input.release(x + 500., y + 220.).unwrap();
    app.update(message, &mut Effects::default());
    assert!(!app.widgets.grid_visible());
    assert!(app.settings.positions.contains_key("calendar"));
    assert!(
        !app.surfaces()
            .iter()
            .find(|s| s.id == "widgets")
            .unwrap()
            .visible
    );
}

#[test]
#[ignore = "requires Vulkan; run explicitly with --ignored"]
fn bounded_widget_pixels_match_the_full_surface() {
    let base = fixture(9, false);
    let fonts = fonts(&base);
    let layout = Layout::new(fonts.clone());
    let gpu = lucent_render::Gpu::headless(fonts).unwrap();
    let mut canvas = lucent_render::Canvas::new(gpu);
    for light in [false, true] {
        for (width, height) in [(1920, 1080), (960, 540), (390, 700)] {
            for overlap in [false, true] {
                let mut app = fixture(9, light);
                resize_fixture(&mut app, width as f32, height as f32);
                app.settings.visible_widgets = crate::widgets::catalog::WIDGETS
                    .iter()
                    .map(|w| w.id.into())
                    .collect();
                if overlap {
                    for (index, widget) in crate::widgets::catalog::WIDGETS.iter().enumerate() {
                        app.settings.positions.insert(
                            widget.id.into(),
                            lucent_domain::Placement::new(
                                32. + index as f32 * 48.,
                                24. + index as f32 * 40.,
                            )
                            .unwrap(),
                        );
                    }
                }
                let full_cx = ViewContext {
                    surface: "widgets",
                    width: width as f32,
                    height: height as f32,
                    now: 1.,
                };
                let full = layout.build(
                    &app.view(&full_cx),
                    full_cx.width,
                    full_cx.height,
                    &Interaction::default(),
                    full_cx.now,
                );
                for scale in [1, 2] {
                    let expected = canvas.snapshot(width, height, scale, &full.paint).unwrap();
                    let mut actual = vec![0; expected.len()];
                    let mut native_hits = std::collections::BTreeMap::new();
                    for source in app.widgets.surfaces(&app.settings, app.viewport) {
                        let cx = ViewContext {
                            surface: source.id,
                            width: source.rect.w,
                            height: source.rect.h,
                            now: 1.,
                        };
                        let native = layout.build(
                            &app.view(&cx),
                            cx.width,
                            cx.height,
                            &Interaction::default(),
                            cx.now,
                        );
                        for hit in &native.hits {
                            native_hits.insert(
                                hit.id.clone(),
                                Rect::new(
                                    hit.rect.x + source.rect.x,
                                    hit.rect.y + source.rect.y,
                                    hit.rect.w,
                                    hit.rect.h,
                                ),
                            );
                        }
                        let pixels = canvas
                            .snapshot(
                                source.rect.w as u32,
                                source.rect.h as u32,
                                scale,
                                &native.paint,
                            )
                            .unwrap();
                        let row_bytes = source.rect.w as usize * scale as usize * 4;
                        for row in 0..source.rect.h as usize * scale as usize {
                            let start = ((source.rect.y as usize * scale as usize + row)
                                * width as usize
                                * scale as usize
                                + source.rect.x as usize * scale as usize)
                                * 4;
                            actual[start..start + row_bytes]
                                .copy_from_slice(&pixels[row * row_bytes..(row + 1) * row_bytes]);
                        }
                    }
                    assert_eq!(native_hits.len(), full.hits.len());
                    for hit in &full.hits {
                        assert_eq!(native_hits[&hit.id], hit.rect);
                    }
                    let max_error = expected
                        .iter()
                        .zip(&actual)
                        .map(|(a, b)| a.abs_diff(*b))
                        .max()
                        .unwrap();
                    assert!(
                        max_error <= 1,
                        "widget crop {width}x{height}/{scale}x/light={light}/overlap={overlap}: {max_error}"
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "requires Vulkan; run explicitly with --ignored"]
fn compact_dock_pixels_match_the_full_surface() {
    let base = fixture(9, false);
    let fonts = fonts(&base);
    let layout = Layout::new(fonts.clone());
    let gpu = lucent_render::Gpu::headless(fonts).unwrap();
    let mut canvas = lucent_render::Canvas::new(gpu);
    for light in [false, true] {
        for (width, height) in [(1920, 1080), (1280, 720), (960, 540), (390, 700)] {
            let mut app = fixture(9, light);
            resize_fixture(&mut app, width as f32, height as f32);
            app.update(
                Message::Launcher(launcher::Message::Close),
                &mut Effects::default(),
            );
            let compact_height = crate::shell_layout::dock_surface_height(height as f32);
            let build = |native_height: u32| {
                let cx = ViewContext {
                    surface: "dock",
                    width: width as f32,
                    height: native_height as f32,
                    now: 1.,
                };
                layout.build(
                    &app.view(&cx),
                    cx.width,
                    cx.height,
                    &Interaction::default(),
                    cx.now,
                )
            };
            let full = build(height);
            let compact = build(compact_height);
            let offset = (height - compact_height) as f32;
            assert_eq!(full.hits.len(), compact.hits.len());
            for (a, b) in full.hits.iter().zip(&compact.hits) {
                assert_eq!(a.id, b.id);
                assert_eq!(
                    a.rect,
                    Rect::new(b.rect.x, b.rect.y + offset, b.rect.w, b.rect.h)
                );
                assert!(fully_visible(b.rect, b.clip.bounds));
            }
            for scale in [1, 2] {
                let expected = canvas.snapshot(width, height, scale, &full.paint).unwrap();
                let pixels = canvas
                    .snapshot(width, compact_height, scale, &compact.paint)
                    .unwrap();
                let mut actual = vec![0; expected.len()];
                let start = ((height - compact_height) * scale * width * scale * 4) as usize;
                assert!(
                    expected[..start].iter().all(|byte| *byte == 0),
                    "crop must retain the entire shadow"
                );
                actual[start..].copy_from_slice(&pixels);
                // Changing the native extent changes floating-point vertex
                // interpolation. Permit only one 8-bit quantization step;
                // existing whole-desktop snapshot tolerances stay unchanged.
                let max_error = expected
                    .iter()
                    .zip(&actual)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                assert!(
                    max_error <= 1,
                    "cropping changed {width}x{height}/{scale}x/light={light} pixels by {max_error}"
                );
            }
        }
    }
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
    let filter = std::env::var("LUCENT_VISUAL_FILTER").ok();
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
            ("catalog-empty", Mode::Apps, 0, 0, "", 1., 640.),
            ("catalog-unavailable", Mode::Apps, 0, 0, "", 1., 390.),
            ("catalog-pinned", Mode::Apps, 9, 0, "", 1., 640.),
            ("dock-hidden", Mode::Apps, 6, 0, "", 1., 640.),
            ("dock-dragging", Mode::Apps, 6, 0, "", 1., 640.),
            ("apps-first", Mode::Apps, 9, 0, "", 1., 640.),
            ("apps-last", Mode::Apps, 9, 8, "", 1., 640.),
            ("apps-empty", Mode::Apps, 9, 0, "no results", 1., 640.),
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
            (
                "wallpapers-unavailable",
                Mode::Wallpapers,
                9,
                0,
                "",
                1.,
                1280.,
            ),
            ("wallpapers-online", Mode::Wallpapers, 9, 0, "", 1., 960.),
            (
                "wallpapers-online-narrow",
                Mode::Wallpapers,
                9,
                0,
                "",
                1.,
                390.,
            ),
            (
                "wallpapers-online-error",
                Mode::Wallpapers,
                9,
                0,
                "",
                1.,
                640.,
            ),
            ("palette-ocean", Mode::Themes, 9, 2, "", 1., 640.),
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
            ("widget-resize-preview", Mode::Apps, 0, 0, "", 1., 960.),
            ("widget-resize-saved", Mode::Apps, 0, 0, "", 1., 960.),
            ("widget-resize-small", Mode::Apps, 0, 0, "", 1., 390.),
            ("widget-edit-keyboard", Mode::Apps, 0, 0, "", 1., 640.),
            ("widget-editor", Mode::Apps, 0, 0, "", 1., 960.),
            ("notes-saved", Mode::Apps, 0, 0, "", 1., 390.),
            ("notes-saving", Mode::Apps, 0, 0, "", 1., 390.),
            ("notes-retrying", Mode::Apps, 0, 0, "", 1., 390.),
            ("notes-failed", Mode::Apps, 0, 0, "", 1., 390.),
            ("notes-unavailable", Mode::Apps, 0, 0, "", 1., 390.),
            ("menu-submenu", Mode::Apps, 0, 0, "", 1., 640.),
            ("audio-loading", Mode::Apps, 0, 0, "", 1., 640.),
            ("tray-loading", Mode::Apps, 0, 0, "", 1., 640.),
            ("tray-unavailable", Mode::Apps, 0, 0, "", 1., 390.),
            ("tray-empty", Mode::Apps, 0, 0, "", 1., 390.),
            ("tray-items", Mode::Apps, 0, 0, "", 1., 390.),
            ("tray-menu", Mode::Apps, 0, 0, "", 1., 390.),
            ("tray-last", Mode::Apps, 0, 0, "", 1., 390.),
            ("tray-opening", Mode::Apps, 0, 0, "", 0.12, 640.),
            ("power-loading", Mode::Apps, 0, 0, "", 1., 640.),
            ("power-unavailable", Mode::Apps, 0, 0, "", 1., 390.),
            ("power-controls", Mode::Apps, 0, 0, "", 1., 390.),
            ("power-confirmation", Mode::Apps, 0, 0, "", 1., 390.),
            ("power-pending", Mode::Apps, 0, 0, "", 1., 390.),
            ("power-failed", Mode::Apps, 0, 0, "", 1., 390.),
            ("power-last", Mode::Apps, 0, 0, "", 1., 390.),
            ("audio-unavailable", Mode::Apps, 0, 0, "", 1., 390.),
            ("audio-outputs", Mode::Apps, 0, 0, "", 1., 640.),
            ("audio-inputs", Mode::Apps, 0, 0, "", 1., 390.),
            ("audio-apps", Mode::Apps, 0, 0, "", 1., 640.),
            ("audio-route", Mode::Apps, 0, 0, "", 1., 390.),
            ("audio-error", Mode::Apps, 0, 0, "", 1., 390.),
            ("audio-stale", Mode::Apps, 0, 0, "", 1., 390.),
            ("audio-opening", Mode::Apps, 0, 0, "", 0.12, 640.),
            ("audio-last", Mode::Apps, 0, 0, "", 1., 390.),
            ("network-loading", Mode::Apps, 0, 0, "", 1., 640.),
            ("network-unavailable", Mode::Apps, 0, 0, "", 1., 390.),
            ("network-available", Mode::Apps, 0, 0, "", 1., 640.),
            ("network-password", Mode::Apps, 0, 0, "", 1., 390.),
            ("network-hidden", Mode::Apps, 0, 0, "", 1., 390.),
            ("network-forget", Mode::Apps, 0, 0, "", 1., 390.),
            ("network-error", Mode::Apps, 0, 0, "", 1., 390.),
            ("network-opening", Mode::Apps, 0, 0, "", 0.12, 640.),
            ("network-last", Mode::Apps, 0, 0, "", 1., 390.),
            ("bluetooth-loading", Mode::Apps, 0, 0, "", 1., 640.),
            ("bluetooth-unavailable", Mode::Apps, 0, 0, "", 1., 390.),
            ("bluetooth-empty", Mode::Apps, 0, 0, "", 1., 640.),
            ("bluetooth-devices", Mode::Apps, 0, 0, "", 1., 390.),
            (
                "bluetooth-connection-admission",
                Mode::Apps,
                0,
                0,
                "",
                1.,
                390.,
            ),
            (
                "bluetooth-connection-pending",
                Mode::Apps,
                0,
                0,
                "",
                1.,
                390.,
            ),
            (
                "bluetooth-connection-cancelled",
                Mode::Apps,
                0,
                0,
                "",
                1.,
                390.,
            ),
            (
                "bluetooth-connection-failed",
                Mode::Apps,
                0,
                0,
                "",
                1.,
                390.,
            ),
            (
                "bluetooth-unpaired-connected",
                Mode::Apps,
                0,
                0,
                "",
                1.,
                390.,
            ),
            ("bluetooth-radio", Mode::Apps, 0, 0, "", 1., 390.),
            ("bluetooth-radio-last", Mode::Apps, 0, 0, "", 1., 390.),
            ("bluetooth-confirmation", Mode::Apps, 0, 0, "", 1., 390.),
            ("bluetooth-pin", Mode::Apps, 0, 0, "", 1., 390.),
            ("bluetooth-display", Mode::Apps, 0, 0, "", 1., 640.),
            ("bluetooth-service", Mode::Apps, 0, 0, "", 1., 390.),
            ("bluetooth-forget", Mode::Apps, 0, 0, "", 1., 390.),
            ("bluetooth-last", Mode::Apps, 0, 0, "", 1., 390.),
            ("bluetooth-opening", Mode::Apps, 0, 0, "", 0.12, 640.),
        ] {
            if filter
                .as_ref()
                .is_some_and(|filter| !name.starts_with(filter))
            {
                continue;
            }
            let height = if name.starts_with("launcher-size-") {
                1080.
            } else if name.starts_with("notes-") {
                320.
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
            resize_fixture(&mut app, width, height);
            app.update(
                Message::Launcher(launcher::Message::Mode(mode)),
                &mut Effects::default(),
            );
            app.update(
                Message::Launcher(launcher::Message::Select(selected)),
                &mut Effects::default(),
            );
            if name == "apps-empty" {
                app.update(
                    Message::Launcher(launcher::Message::Query(query.into())),
                    &mut Effects::default(),
                );
            } else {
                app.launcher.fixture_query(query.into());
            }
            if matches!(name, "wallpapers" | "wallpapers-unavailable") {
                app.wallpapers = wallpaper_fixtures();
            }
            if name == "wallpapers-unavailable" {
                app.previews = crate::previews::Previews::new(Arc::new(UnavailablePreviews(
                    app.adapters.assets.clone(),
                )));
                let mut effects = Effects::default();
                app.update(Message::Clock(app.clock.clone()), &mut effects);
                while let Some(task) = effects.tasks.pop() {
                    app.update(task.run(), &mut effects);
                }
            }
            if name.starts_with("wallpapers-online") {
                online_fixture(&mut app);
                if name.ends_with("error") {
                    app.browser.items.clear();
                    app.browser.status = "Provider unavailable. Try again later.".into();
                }
            }
            if name == "palette-ocean" {
                app.settings.palette = Some(lucent_design::palettes()[2].clone());
            }
            let scene = if let Some(state) = name.strip_prefix("notes-") {
                let app = notes_fixture(state, light);
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
            } else if name.starts_with("widget-resize-") || name.starts_with("widget-edit") {
                app.settings_writable = true;
                app.settings.visible_widgets = vec!["notes".into()];
                app.settings.positions.insert(
                    "notes".into(),
                    lucent_domain::Placement::new(64., 96.).unwrap(),
                );
                app.settings.notes = "A thought to keep nearby".into();
                if name == "widget-editor" {
                    app.settings.visible_widgets.push("calendar".into());
                    app.settings.positions.insert(
                        "calendar".into(),
                        lucent_domain::Placement::new(416., 96.).unwrap(),
                    );
                }
                let mut effects = Effects::default();
                app.update(
                    Message::Widgets(crate::widgets::Message::Edit(true)),
                    &mut effects,
                );
                app.update(
                    Message::Widgets(crate::widgets::Message::Press),
                    &mut effects,
                );
                if name == "widget-resize-small" {
                    app.settings.widget_sizes.insert(
                        "notes".into(),
                        lucent_domain::WidgetSize::new(640., 320.).unwrap(),
                    );
                }
                if name == "widget-resize-preview" || name == "widget-resize-saved" {
                    app.update(
                        Message::Widgets(crate::widgets::Message::Resize(
                            "notes".into(),
                            DragEvent {
                                dx: 89.,
                                dy: 65.,
                                finished: false,
                            },
                        )),
                        &mut effects,
                    );
                    if name == "widget-resize-saved" {
                        app.update(
                            Message::Widgets(crate::widgets::Message::Resize(
                                "notes".into(),
                                DragEvent {
                                    dx: 89.,
                                    dy: 65.,
                                    finished: true,
                                },
                            )),
                            &mut effects,
                        );
                    }
                }
                let mut interaction = Interaction::default();
                interaction.focus =
                    (name == "widget-edit-keyboard").then(|| "widget-size-notes-1".into());
                interaction.focus_visible = name == "widget-edit-keyboard";
                layout.build(
                    &app.view(&ViewContext {
                        surface: "widgets",
                        width,
                        height,
                        now,
                    }),
                    width,
                    height,
                    &interaction,
                    now,
                )
            } else if name == "widget-drag-grid" {
                app.settings.visible_widgets = vec!["calendar".into(), "weather".into()];
                app.update(
                    Message::Widgets(crate::widgets::Message::Move(
                        "calendar".into(),
                        DragEvent {
                            dx: 151.,
                            dy: 73.,
                            finished: false,
                        },
                    )),
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
                    palette: None,
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
                    lucent_session::Message::Wallpaper(Some(Arc::new(
                        ImageData::from_rgba("session-wallpaper-fixture", 320, 180, pixels)
                            .unwrap(),
                    ))),
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
            } else if name.starts_with("catalog-") || name.starts_with("dock-") {
                let mut effects = Effects::default();
                if name.starts_with("catalog-") {
                    app.update(
                        Message::Launcher(launcher::Message::Mode(Mode::Apps)),
                        &mut effects,
                    );
                    if name == "catalog-empty" {
                        app.update(
                            Message::Catalog(crate::catalog::Message::Observed(Ok(vec![]))),
                            &mut effects,
                        );
                    }
                    if name == "catalog-unavailable" {
                        app.update(
                            Message::Catalog(crate::catalog::Message::Observed(Err(
                                lucent_domain::DomainError::Unavailable(
                                    "Application catalog observation is unavailable".into(),
                                ),
                            ))),
                            &mut effects,
                        );
                    }
                    if name == "catalog-pinned" {
                        let id = app.apps[4].id.clone();
                        app.update(
                            Message::Dock(crate::dock::Message::SetPin(id, true)),
                            &mut effects,
                        );
                    }
                } else {
                    if app.launcher.open() {
                        app.update(Message::Launcher(launcher::Message::Close), &mut effects);
                        if let Some(message) = app.event(Event::FrameSubmitted {
                            surface: "dock",
                            width,
                            height,
                            now: 10.,
                        }) {
                            let mut settled = Effects {
                                now: 10.,
                                ..Default::default()
                            };
                            app.update(message, &mut settled);
                        }
                    }
                    if name == "dock-hidden" {
                        app.settings.dock.auto_hide = true;
                        app.dock.fixture_hidden();
                    } else {
                        let id = app.dock_apps()[0].id.clone();
                        app.update(Message::Dock(crate::dock::Message::Press), &mut effects);
                        app.update(
                            Message::Dock(crate::dock::Message::Drag(
                                id,
                                DragEvent {
                                    dx: lucent_design::component::dock_row::ITEM_SIZE
                                        + lucent_design::space::SM,
                                    dy: 0.,
                                    finished: false,
                                },
                            )),
                            &mut effects,
                        );
                    }
                }
                let tree = app.view(&ViewContext {
                    surface: "dock",
                    width,
                    height,
                    now,
                });
                layout.build(&tree, width, height, &Interaction::default(), now)
            } else if name.starts_with("tray-") {
                use crate::tray::{
                    Message as T,
                    tests::{fixture, menu_fixture},
                };
                let mut effects = Effects::default();
                app.update(Message::Tray(T::Open), &mut effects);
                if name != "tray-loading" {
                    let result = if name == "tray-unavailable" {
                        Err(lucent_domain::DomainError::Unavailable(
                            "Tray application service is unavailable".into(),
                        ))
                    } else if name == "tray-empty" {
                        Ok(lucent_domain::TraySnapshot::default())
                    } else {
                        Ok(fixture())
                    };
                    app.update(Message::Tray(T::Observed(result)), &mut effects);
                    for task in std::mem::take(&mut effects.tasks) {
                        app.update(task.run(), &mut effects);
                    }
                }
                if name == "tray-menu" {
                    app.tray.load_fixture_menu(menu_fixture());
                }
                if name == "tray-last" {
                    app.update(
                        Message::Tray(T::Scroll(ScrollEvent { offset: 10000. })),
                        &mut effects,
                    );
                }
                let tree = app.view(&ViewContext {
                    surface: "tray",
                    width,
                    height,
                    now,
                });
                let mut input = Interaction::default();
                input.synchronize(&layout.build(&tree, width, height, &input, now));
                layout.build(&tree, width, height, &input, now)
            } else if name.starts_with("power-") {
                use crate::power::{Message as P, tests::fixture};
                use lucent_domain::{
                    DomainError, PowerAction, PowerCommand, PowerObjectId, PowerOperation,
                    PowerOperationId, PowerOperationPhase,
                };
                let mut effects = Effects::default();
                app.update(Message::Power(P::Open), &mut effects);
                if name != "power-loading" {
                    let snapshot = if name == "power-unavailable" {
                        Err(DomainError::Unavailable(
                            "Power services disconnected".into(),
                        ))
                    } else {
                        let mut state = fixture();
                        if matches!(name, "power-pending" | "power-failed") {
                            state.operation = Some(PowerOperation {
                                id: PowerOperationId::new(10).unwrap(),
                                command: PowerCommand::SetProfile(PowerObjectId::new(2).unwrap()),
                                phase: if name == "power-pending" {
                                    PowerOperationPhase::InProgress
                                } else {
                                    PowerOperationPhase::Failed("Power permission denied".into())
                                },
                            });
                        }
                        Ok(state)
                    };
                    app.update(Message::Power(P::Observed(snapshot)), &mut effects);
                }
                if name == "power-confirmation" {
                    app.update(Message::Power(P::Ask(PowerAction::PowerOff)), &mut effects);
                }
                if name == "power-last" {
                    app.update(
                        Message::Power(P::Scroll(ScrollEvent { offset: 10000. })),
                        &mut effects,
                    );
                }
                let tree = app.view(&ViewContext {
                    surface: "power",
                    width,
                    height,
                    now,
                });
                let mut input = Interaction::default();
                input.synchronize(&layout.build(&tree, width, height, &input, now));
                layout.build(&tree, width, height, &input, now)
            } else if name.starts_with("bluetooth-") {
                use crate::bluetooth::{
                    Message as B,
                    tests::{connecting_fixture, fixture, id, pairing_fixture, pairing_id},
                };
                use lucent_domain::{
                    BluetoothPairingCode as Code, BluetoothPairingPrompt as P, BluetoothServiceId,
                };
                if name.starts_with("bluetooth-radio") {
                    app.update(
                        Message::Bluetooth(B::Tab(crate::bluetooth::Tab::Radio)),
                        &mut Effects::default(),
                    );
                }
                let mut effects = Effects::default();
                app.update(Message::Bluetooth(B::Open), &mut effects);
                if name != "bluetooth-loading" {
                    let snapshot = match name {
                        "bluetooth-unavailable" => Err(lucent_domain::DomainError::Unavailable(
                            "Bluetooth service disconnected".into(),
                        )),
                        "bluetooth-empty" => Ok(lucent_domain::BluetoothSnapshot::default()),
                        "bluetooth-connection-pending"
                        | "bluetooth-connection-cancelled"
                        | "bluetooth-connection-failed" => {
                            let mut snapshot = connecting_fixture();
                            snapshot.connection.as_mut().unwrap().phase = match name {
                                "bluetooth-connection-cancelled" => {
                                    lucent_domain::BluetoothConnectionPhase::Cancelled
                                }
                                "bluetooth-connection-failed" => {
                                    lucent_domain::BluetoothConnectionPhase::Failed(
                                        "Bluetooth permission denied".into(),
                                    )
                                }
                                _ => lucent_domain::BluetoothConnectionPhase::Connecting,
                            };
                            Ok(snapshot)
                        }
                        "bluetooth-unpaired-connected" => {
                            let mut snapshot = fixture();
                            snapshot.devices[0].connected = true;
                            Ok(snapshot)
                        }
                        "bluetooth-confirmation" => {
                            Ok(pairing_fixture(P::ConfirmPasskey(Code::new(17).unwrap())))
                        }
                        "bluetooth-pin" => Ok(pairing_fixture(P::PinCode)),
                        "bluetooth-display" => Ok(pairing_fixture(P::DisplayPasskey {
                            code: Code::new(17).unwrap(),
                            entered: 3,
                        })),
                        "bluetooth-service" => Ok(pairing_fixture(P::AuthorizeService(
                            BluetoothServiceId::new("0000110b-0000-1000-8000-00805f9b34fb")
                                .unwrap(),
                        ))),
                        _ => Ok(fixture()),
                    };
                    app.update(Message::Bluetooth(B::Observed(snapshot)), &mut effects);
                }
                if name == "bluetooth-connection-admission" {
                    app.update(Message::Bluetooth(B::Connect(id(3))), &mut effects);
                }
                if name == "bluetooth-pin" {
                    app.update(
                        Message::Bluetooth(B::Edit(
                            pairing_id(41),
                            InputEdit::Insert("fixturePin1".into()),
                        )),
                        &mut effects,
                    );
                }
                if name == "bluetooth-forget" {
                    app.update(Message::Bluetooth(B::AskForget(id(3))), &mut effects);
                }
                if matches!(name, "bluetooth-last" | "bluetooth-radio-last") {
                    app.update(
                        Message::Bluetooth(B::Scroll(ScrollEvent { offset: 10000. })),
                        &mut effects,
                    );
                }
                let tree = app.view(&ViewContext {
                    surface: "bluetooth",
                    width,
                    height,
                    now,
                });
                let mut input = Interaction::default();
                input.synchronize(&layout.build(&tree, width, height, &input, now));
                layout.build(&tree, width, height, &input, now)
            } else if name.starts_with("network-") {
                use crate::network::{
                    Message as N,
                    tests::{fixture, id},
                };
                let mut effects = Effects::default();
                app.update(Message::Network(N::Open), &mut effects);
                if name != "network-loading" {
                    let state = if name == "network-unavailable" {
                        Err(lucent_domain::DomainError::Unavailable(
                            "The network service disconnected".into(),
                        ))
                    } else {
                        let mut snapshot = fixture();
                        for index in 3..10 {
                            let mut ap = snapshot.access_points[0].clone();
                            ap.id = id(10 + index);
                            ap.ssid = Some(
                                lucent_domain::NetworkSsid::new(
                                    format!("Example network {index}").into_bytes(),
                                )
                                .unwrap(),
                            );
                            snapshot.access_points.push(ap);
                        }
                        Ok(snapshot)
                    };
                    app.update(Message::Network(N::Observed(state)), &mut effects);
                }
                if name == "network-password" {
                    app.update(Message::Network(N::Join(id(2))), &mut effects);
                    app.update(
                        Message::Network(N::Password(
                            1,
                            InputEdit::Insert("fixture-secret".into()),
                        )),
                        &mut effects,
                    );
                }
                if name == "network-hidden" {
                    app.update(Message::Network(N::Hidden), &mut effects);
                }
                if name == "network-forget" {
                    app.update(Message::Network(N::AskForget(id(3))), &mut effects);
                }
                if name == "network-error" {
                    app.update(
                        Message::Network(N::Request(crate::network::Control::Scan(id(999)))),
                        &mut effects,
                    );
                }
                if name == "network-last" {
                    app.update(
                        Message::Network(N::Scroll(ScrollEvent { offset: 10000. })),
                        &mut effects,
                    );
                }
                let tree = app.view(&ViewContext {
                    surface: "network",
                    width,
                    height,
                    now,
                });
                let mut input = Interaction::default();
                input.synchronize(&layout.build(&tree, width, height, &input, now));
                layout.build(&tree, width, height, &input, now)
            } else if name.starts_with("audio-") {
                use crate::audio::{Message as AudioMessage, panel::Tab};
                app.update(Message::Audio(AudioMessage::Open), &mut Effects::default());
                if name != "audio-loading" {
                    let state = if name == "audio-unavailable" {
                        Err(lucent_domain::DomainError::Unavailable(
                            "The audio service disconnected".into(),
                        ))
                    } else {
                        Ok(audio_fixture_snapshot())
                    };
                    app.update(
                        Message::Audio(AudioMessage::Observed(state)),
                        &mut Effects::default(),
                    );
                }
                let tab = if name == "audio-inputs" {
                    Tab::Inputs
                } else if matches!(name, "audio-apps" | "audio-route") {
                    Tab::Applications
                } else {
                    Tab::Outputs
                };
                app.update(
                    Message::Audio(AudioMessage::Tab(tab)),
                    &mut Effects::default(),
                );
                if name == "audio-route" {
                    app.update(
                        Message::Audio(AudioMessage::ChooseRoute(
                            lucent_domain::AudioObjectId::new(100).unwrap(),
                        )),
                        &mut Effects::default(),
                    );
                }
                if name == "audio-last" {
                    app.update(
                        Message::Audio(AudioMessage::Scroll(ScrollEvent { offset: 10000. })),
                        &mut Effects::default(),
                    );
                }
                if name == "audio-error" {
                    app.update(
                        Message::Audio(AudioMessage::Request(lucent_domain::AudioCommand::Raise)),
                        &mut Effects::default(),
                    );
                    app.update(Message::Audio(AudioMessage::Control(crate::audio::commands::Message::WorkerFailed("The requested audio change failed; the observed device state is preserved".into()))), &mut Effects::default());
                }
                if name == "audio-stale" {
                    audio_stale_failure(&mut app);
                }
                let cx = ViewContext {
                    surface: "audio",
                    width,
                    height,
                    now,
                };
                layout.build(&app.view(&cx), width, height, &Interaction::default(), now)
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
                        let image = lucent_services::images::svg_mask(
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
                app.audio.observe(Ok(crate::audio::fixture()));
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
                    app.update(
                        Message::Launcher(launcher::Message::Close),
                        &mut Effects::default(),
                    );
                }
                scene(&app, &layout, &mut Interaction::default(), now)
            };
            let mut paint = vec![Paint::Shape {
                rect: Rect::new(0., 0., width, height),
                clip: Rect::new(0., 0., width, height).into(),
                color: Color::hex(if light { 0xc9c3b9 } else { 0x28272d }),
                radius: 0.,
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
            Message::Launcher(launcher::Message::Select(index)),
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
        assert!(scene.paint.iter().any(|p|matches!(p,Paint::Shape {rect,color,..} if *rect==row.rect && *color==app.theme().surface_container)),"selected fill must be immediate");
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
        crate::actions::Action::VolumeUp,
        crate::actions::Action::PlayPause,
        crate::actions::Action::Lock,
    ] {
        let mut effects = Effects::default();
        app.update(Message::Action(action), &mut effects);
        for task in effects.tasks {
            app.update(task.run(), &mut Effects::default());
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
                    fully_visible(hit.rect, hit.clip.bounds),
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
        resize_fixture(&mut app, width, height);
        for mode in Mode::ALL {
            app.update(
                Message::Launcher(launcher::Message::Mode(mode)),
                &mut Effects::default(),
            );
            app.update(
                Message::Launcher(launcher::Message::Select(19)),
                &mut Effects::default(),
            );
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
                    || hit.id == "edit-layout"
                    || hit.id == "apply-wallpaper"
                    || hit.id == "wallpaper-previous"
                    || hit.id == "wallpaper-next"
                {
                    assert!(
                        fully_visible(hit.rect, hit.clip.bounds),
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
                        Paint::Shape { rect, color, .. }
                            if *color == app.theme().surface_container
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
                for hit in rendered
                    .hits
                    .iter()
                    .filter(|h| h.id.starts_with("theme-") || h.id == "palette-from-wallpaper")
                {
                    assert!(fully_visible(hit.rect, hit.clip.bounds));
                    assert_eq!(
                        hit.rect.x - panel.x,
                        panel.x + panel.w - hit.rect.x - hit.rect.w
                    );
                }
            }
        }
        app.update(
            Message::Launcher(launcher::Message::Close),
            &mut Effects::default(),
        );
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
            let geometry = crate::shell_layout::BarLayout::new(width, count.max(6) as usize);
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
        app.update(task.run(), &mut Effects::default());
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
    assert_eq!(spec.pointer_input, PointerInput::Content);
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

fn online_fixture(app: &mut Desktop) {
    use lucent_domain::{RemoteWallpaper, WallpaperProvider};
    app.browser.source = Some(WallpaperProvider::Wallhaven);
    app.browser.query = "landscape".into();
    app.browser.page = 1;
    app.browser.has_more = true;
    app.browser.status = "6 results · page 1".into();
    app.browser.items = (0..6)
        .map(|i| RemoteWallpaper {
            provider: WallpaperProvider::Wallhaven,
            id: format!("wall0{i}"),
            title: format!("Synthetic landscape {i}"),
            page_url: String::new(),
            thumbnail_url: String::new(),
            image_url: String::new(),
            resolution: "1920×1080".into(),
        })
        .collect();
    for (i, item) in app.browser.items.iter().enumerate() {
        let mut rgba = Vec::new();
        for y in 0..96 {
            for x in 0..160 {
                rgba.extend_from_slice(&[
                    (x + i * 19) as u8,
                    (y * 2 + i * 11) as u8,
                    (80 + i * 23) as u8,
                    255,
                ]);
            }
        }
        app.browser.previews.insert(
            item.id.clone(),
            Arc::new(
                ImageData::from_rgba(format!("synthetic-landscape-{i}"), 160, 96, rgba).unwrap(),
            ),
        );
    }
}
#[test]
fn online_browser_controls_and_selected_result_fit_at_small_and_large_sizes() {
    let mut app = fixture(0, false);
    let layout = Layout::new(fonts(&app));
    for (width, height) in [(960., 768.), (390., 580.), (320., 360.)] {
        resize_fixture(&mut app, width, height);
        app.update(
            Message::Launcher(launcher::Message::Mode(Mode::Wallpapers)),
            &mut Effects::default(),
        );
        online_fixture(&mut app);
        app.browser.selected = 5;
        let rendered = scene(&app, &layout, &mut Interaction::default(), 1.);
        for id in [
            "source-local",
            "source-wallhaven",
            "source-alpha-coders",
            "wallpaper-search",
            "wallpaper-search-submit",
            "download-theme",
            "download-wallpaper",
            "catalog-previous",
            "catalog-next",
        ] {
            let hit = rendered.hits.iter().find(|h| h.id == id).unwrap();
            assert!(
                fully_visible(hit.rect, hit.clip.bounds),
                "{id} clipped at {width}×{height}: {:?} {:?}",
                hit.rect,
                hit.clip
            );
        }
        if height > 360. {
            let selected = rendered
                .hits
                .iter()
                .find(|h| h.id == "online-wallpaper-5")
                .expect("last result reachable");
            assert!(fully_visible(selected.rect, selected.clip.bounds));
        }
    }
}
#[test]
fn palette_library_and_failed_download_preserve_user_intent() {
    let mut app = fixture(0, false);
    app.settings_writable = true;
    let mut effects = Effects::default();
    let palette = lucent_design::wallpaper_palette(lucent_domain::Rgb::new(33, 66, 150));
    app.update(Message::Palette(palette.clone()), &mut effects);
    app.update(Message::Theme(true), &mut effects);
    assert!(app.palette_choices().iter().any(|p| p == &palette));
    app.wallpaper.current = Some(lucent_domain::WallpaperAsset {
        source: lucent_domain::Wallpaper {
            path: "existing".into(),
            name: "Existing".into(),
        },
        path: "existing".into(),
    });
    app.update(
        Message::Browser(crate::wallpaper_browser::Message::Ready(
            0,
            Err(lucent_domain::DomainError::Unavailable(
                "network unavailable".into(),
            )),
        )),
        &mut effects,
    );
    assert_eq!(app.wallpaper.source_path(), "existing");
    assert_eq!(app.settings.palette.as_ref().unwrap().name, "Pearl");
}

#[test]
fn invalid_drag_does_not_persist_positions_or_leave_grid_visible() {
    let mut app = fixture(0, false);
    app.settings_writable = true;
    app.update(
        Message::Widgets(crate::widgets::Message::Move(
            "calendar".into(),
            DragEvent {
                dx: 10.,
                dy: 20.,
                finished: false,
            },
        )),
        &mut Effects::default(),
    );
    let position = app.settings.positions.get("calendar").copied();
    let mut effects = Effects::default();
    app.update(
        Message::Widgets(crate::widgets::Message::Move(
            "calendar".into(),
            DragEvent {
                dx: f32::NAN,
                dy: 0.,
                finished: true,
            },
        )),
        &mut effects,
    );
    assert_eq!(app.settings.positions.get("calendar").copied(), position);
    assert!(!app.widgets.grid_visible());
    assert!(effects.tasks.is_empty());
    assert!(app.error.contains("finite"));
    assert!(effects.redraw.contains("widgets"));
}
#[test]
fn calendar_navigation_handles_extreme_messages_without_overflow() {
    let mut app = fixture(0, false);
    for delta in [i32::MAX, i32::MAX, i32::MIN, i32::MIN] {
        app.update(
            Message::Widgets(crate::widgets::Message::Month(delta)),
            &mut Effects::default(),
        );
        assert!((-120..=120).contains(&app.widgets.month_offset()));
    }
}

#[test]
fn widget_catalog_covers_selector_views_geometry_and_ipc() {
    let mut app = fixture(0, false);
    let catalog = &crate::widgets::catalog::WIDGETS;
    app.settings.visible_widgets = catalog.iter().map(|entry| entry.id.into()).collect();
    let layout = Layout::new(fonts(&app));
    let cx = ViewContext {
        surface: "widgets",
        width: 1920.,
        height: 1080.,
        now: 100.,
    };
    let widgets = layout.build(
        &app.view(&cx),
        cx.width,
        cx.height,
        &Interaction::default(),
        cx.now,
    );
    app.update(
        Message::Launcher(launcher::Message::Mode(Mode::Widgets)),
        &mut Effects::default(),
    );
    let mut ids = std::collections::BTreeSet::new();
    for (index, entry) in catalog.iter().enumerate() {
        assert!(ids.insert(entry.id), "duplicate catalog entry");
        let hit = widgets
            .hits
            .iter()
            .find(|hit| hit.id == format!("widget-{}", entry.id))
            .expect("catalog view exists");
        assert_eq!((hit.rect.w, hit.rect.h), entry.size);
        assert!(app.command(&format!("widget {} show", entry.id)).is_ok());
        app.update(
            Message::Launcher(launcher::Message::Select(index)),
            &mut Effects::default(),
        );
        let selector = scene(&app, &layout, &mut Interaction::default(), 100.);
        assert!(
            selector
                .hits
                .iter()
                .any(|hit| hit.id == format!("toggle-{}", entry.id)),
            "selector scrolls to {}",
            entry.id
        );
    }
    assert!(app.command("widget missing show").is_err());
}

fn notes_fixture(state: &str, light: bool) -> Desktop {
    let mut app = fixture(0, light);
    app.settings_writable = state != "unavailable";
    app.settings.visible_widgets = vec!["notes".into()];
    app.settings.notes = "An idea to keep".into();
    app.settings.positions.insert(
        "notes".into(),
        lucent_domain::Placement::new(48., 80.).unwrap(),
    );
    if matches!(state, "saving" | "failed" | "retrying") {
        app.update(
            Message::Widgets(crate::widgets::Message::Notes(app.settings.notes.clone())),
            &mut Effects::default(),
        );
    }
    if matches!(state, "failed" | "retrying") {
        let failure = app
            .runtime_failure(RuntimeFailure {
                source: "effect queue settings".into(),
                reason: "fixture save failure".into(),
            })
            .unwrap();
        app.update(failure, &mut Effects::default());
    }
    if state == "retrying" {
        app.update(
            Message::Persistence(crate::persistence::Message::Retry),
            &mut Effects::default(),
        );
    }
    app
}

#[test]
fn settings_save_failure_survives_unrelated_success_and_widget_retry_acknowledges_it() {
    let mut app = notes_fixture("failed", false);
    app.update(Message::Completed(Ok(())), &mut Effects::default());
    assert!(app.error_message().contains("fixture save failure"));
    let layout = Layout::new(fonts(&app));
    let cx = ViewContext {
        surface: "widgets",
        width: 390.,
        height: 320.,
        now: 1.,
    };
    let scene = layout.build(
        &app.view(&cx),
        cx.width,
        cx.height,
        &Interaction::default(),
        cx.now,
    );
    let retry = scene
        .hits
        .iter()
        .find(|hit| hit.id == "notes-save-retry")
        .unwrap();
    assert!(fully_visible(retry.rect, retry.clip.bounds));
    let mut effects = Effects::default();
    app.update(retry.click.clone().unwrap(), &mut effects);
    assert_eq!(effects.tasks.len(), 1);
    assert!(app.error_message().contains("fixture save failure"));
    app.update(effects.tasks.remove(0).run(), &mut effects);
    assert!(app.error_message().is_empty());
    let inspect: serde_json::Value = serde_json::from_str(&app.inspect()).unwrap();
    assert_eq!(inspect["settings_save_status"], "saved");
    assert_eq!(inspect["settings_save_error"], "");
    assert!(effects.tasks.is_empty());
}

#[test]
fn pending_saves_and_load_failure_never_render_a_saved_caption() {
    use crate::persistence::Status;
    for (state, status, caption) in [
        ("saving", Status::Saving, "Saving…"),
        ("unavailable", Status::Unavailable, "Settings unavailable"),
    ] {
        let app = notes_fixture(state, false);
        assert_eq!(app.persistence.status(app.settings_writable), status);
        let layout = Layout::new(fonts(&app));
        let cx = ViewContext {
            surface: "widgets",
            width: 390.,
            height: 320.,
            now: 1.,
        };
        let scene = layout.build(
            &app.view(&cx),
            cx.width,
            cx.height,
            &Interaction::default(),
            cx.now,
        );
        assert!(
            scene
                .paint
                .iter()
                .any(|paint| matches!(paint, Paint::Text { text, .. } if text == caption))
        );
        assert!(
            !scene
                .paint
                .iter()
                .any(|paint| matches!(paint, Paint::Text { text, .. } if text == "Saved"))
        );
        assert!(!scene.hits.iter().any(|hit| hit.id == "notes-save-retry"));
        if state == "unavailable" {
            assert!(app.command("settings retry").is_err());
        }
    }
}

#[test]
fn invalid_settings_remain_visible_after_unrelated_successful_effects() {
    let mut app = fixture(0, false);
    let mut effects = Effects::default();
    app.update(
        Message::Loaded(
            Err(lucent_domain::DomainError::Invalid(
                "invalid saved position".into(),
            )),
            vec![],
            Ok(None),
        ),
        &mut effects,
    );
    while !effects.tasks.is_empty() {
        let result = effects.tasks.remove(0).run();
        app.update(result, &mut effects);
    }
    app.update(Message::Completed(Ok(())), &mut effects);
    assert!(!app.settings_writable);
    assert!(app.error_message().contains("invalid saved position"));
    assert!(app.command("launcher size 800 600").is_err());
    let inspected: serde_json::Value = serde_json::from_str(&app.inspect()).unwrap();
    assert_eq!(inspected["settings_writable"], false);
    assert!(
        inspected["error"]
            .as_str()
            .unwrap()
            .contains("invalid saved position")
    );
}

#[test]
fn wallpaper_preview_demand_is_lazy_and_reaches_past_the_old_preload_limit() {
    let mut app = fixture(0, false);
    app.wallpapers = (0..200)
        .map(|i| lucent_domain::Wallpaper {
            name: format!("Wallpaper {i}"),
            path: format!("/fixture/{i}"),
        })
        .collect();
    let mut effects = Effects::default();
    app.update(Message::Clock(app.clock.clone()), &mut effects);
    assert!(
        effects.tasks.is_empty(),
        "closed browser must not decode the library"
    );
    app.update(Message::Wallpaper(150, false), &mut effects);
    assert!(effects.tasks.is_empty());
    effects.now = 5.; // settle carousel before opening the local browser
    app.update(
        Message::Launcher(launcher::Message::Mode(Mode::Wallpapers)),
        &mut effects,
    );
    assert_eq!(effects.tasks.len(), 1);
    let info = app.previews.inspect();
    assert_eq!(info["wanted"], 7);
    assert_eq!(info["active"], true);
    // Inject the accepted image as a transaction would. Only UI-owned pixels
    // change; the pure settings and wallpaper selection remain untouched.
    let image = Arc::new(ImageData::from_rgba("late-library-item", 1, 1, vec![255; 4]).unwrap());
    app.previews
        .insert("/fixture/150".into(), image.clone())
        .unwrap();
    assert_eq!(
        app.previews.view().get("/fixture/150").unwrap().cache_id(),
        image.cache_id()
    );
    app.update(Message::Launcher(launcher::Message::Close), &mut effects);
    effects.now = 10.;
    app.update(Message::Clock(app.clock.clone()), &mut effects);
    assert_eq!(app.previews.inspect()["wanted"], 0);
}

#[test]
fn wallpaper_ranges_match_visible_cards_and_bound_large_libraries() {
    let distance = lucent_design::component::wallpaper::VISIBLE_DISTANCE;
    for center in [0., 3.2, 6.9, 64., 10000.4, -1., 1_000_000.] {
        let range = launcher::visible_wallpapers(100_000, center);
        assert!(range.len() <= 7);
        for index in range.clone() {
            assert!((index as f32 - center).abs() < distance);
        }
        if range.start > 0 {
            assert!(((range.start - 1) as f32 - center).abs() >= distance);
        }
        if range.end < 100_000 {
            assert!((range.end as f32 - center).abs() >= distance);
        }
    }
    assert!(launcher::visible_wallpapers(100, f32::NAN).is_empty());
}

fn audio_fixture_snapshot() -> lucent_domain::AudioSnapshot {
    use lucent_domain::*;
    let mut snapshot = crate::audio::fixture();
    for index in 2..=6 {
        snapshot.devices.push(AudioDevice {
            id: AudioObjectId::new(index).unwrap(),
            key: AudioDeviceKey::new(format!("fixture.{index}")).unwrap(),
            name: format!(
                "Device {index}: a deliberately long label to check wrapping and rounded insets"
            ),
            direction: if index % 2 == 0 {
                AudioDirection::Capture
            } else {
                AudioDirection::Playback
            },
            volume: AudioVolume::new(0.42).unwrap(),
            muted: false,
            monitor: index == 6,
        });
    }
    snapshot.default_input = Some(AudioObjectId::new(2).unwrap());
    for index in 0..4 {
        snapshot.streams.push(AudioStream {
            id: AudioObjectId::new(100 + index).unwrap(),
            name: format!("Example stream {index}"),
            application: format!("Example application {index}"),
            direction: if index == 3 {
                AudioDirection::Capture
            } else {
                AudioDirection::Playback
            },
            device: Some(AudioObjectId::new(if index == 3 { 2 } else { 1 }).unwrap()),
            volume: Some(AudioVolume::new(0.65).unwrap()),
            volume_writable: index != 2,
            muted: false,
        });
    }
    snapshot
}
fn audio_stale_failure(app: &mut Desktop) {
    use crate::audio::{Message as AudioMessage, commands};
    use lucent_domain::{AudioCommand, AudioObjectId, AudioVolume};
    app.update(
        Message::Audio(AudioMessage::Request(AudioCommand::SetVolume(
            AudioObjectId::new(1).unwrap(),
            AudioVolume::new(0.5).unwrap(),
        ))),
        &mut Effects::default(),
    );
    app.update(
        Message::Audio(AudioMessage::Control(commands::Message::WorkerFailed(
            "The audio connection changed before the command completed".into(),
        ))),
        &mut Effects::default(),
    );
    let mut reconnected = audio_fixture_snapshot();
    let id = AudioObjectId::new(999).unwrap();
    reconnected.devices[0].id = id;
    reconnected.default_output = Some(id);
    app.update(
        Message::Audio(AudioMessage::Observed(Ok(reconnected))),
        &mut Effects::default(),
    );
}
#[test]
fn audio_stale_failure_exposes_dismiss_and_keyboard_reaches_it_at_narrow_sizes() {
    use crate::audio::Message as AudioMessage;
    for (width, height) in [(390., 700.), (640., 580.), (1920., 1080.)] {
        let mut app = fixture(0, false);
        app.update(
            Message::Audio(AudioMessage::Observed(Ok(audio_fixture_snapshot()))),
            &mut Effects::default(),
        );
        app.update(Message::Audio(AudioMessage::Open), &mut Effects::default());
        audio_stale_failure(&mut app);
        let layout = Layout::new(fonts(&app));
        let cx = ViewContext {
            surface: "audio",
            width,
            height,
            now: 1.,
        };
        let mut input = Interaction::default();
        let scene = layout.build(&app.view(&cx), width, height, &input, 1.);
        assert!(!scene.hits.iter().any(|h| h.id == "audio-retry"));
        let dismiss = scene
            .hits
            .iter()
            .find(|h| h.id == "audio-dismiss-error")
            .unwrap();
        assert!(fully_visible(dismiss.rect, dismiss.clip.bounds));
        for _ in 0..scene.hits.len() {
            let scene = layout.build(&app.view(&cx), width, height, &input, 1.);
            input.synchronize(&scene);
            for message in input.key(&scene, &Key::Tab) {
                app.update(message, &mut Effects::default());
            }
            if input.focus.as_deref() == Some("audio-dismiss-error") {
                break;
            }
        }
        assert_eq!(input.focus.as_deref(), Some("audio-dismiss-error"));
        let scene = layout.build(&app.view(&cx), width, height, &input, 1.);
        for message in input.key(&scene, &Key::Enter) {
            app.update(message, &mut Effects::default());
        }
        assert!(app.audio.inspect()["control_error"].is_null());
    }
}
#[test]
fn audio_panel_closes_competing_launcher_and_keyboard_reveals_controls_at_narrow_sizes() {
    use crate::audio::{Message as AudioMessage, panel::Tab};
    for (width, height) in [(390., 700.), (640., 580.), (1920., 1080.)] {
        let mut app = fixture(0, false);
        app.update(
            Message::Audio(AudioMessage::Observed(Ok(audio_fixture_snapshot()))),
            &mut Effects::default(),
        );
        app.update(Message::Audio(AudioMessage::Open), &mut Effects::default());
        assert!(!app.launcher.open());
        assert!(app.audio.open());
        let layout = Layout::new(fonts(&app));
        let cx = ViewContext {
            surface: "audio",
            width,
            height,
            now: 1.,
        };
        for tab in [Tab::Outputs, Tab::Inputs, Tab::Applications] {
            app.update(
                Message::Audio(AudioMessage::Tab(tab)),
                &mut Effects::default(),
            );
            let mut input = Interaction::default();
            let count = layout
                .build(&app.view(&cx), width, height, &input, 1.)
                .hits
                .iter()
                .filter(|h| h.tab_stop)
                .count();
            let mut visited = std::collections::BTreeSet::new();
            for _ in 0..count {
                let scene = layout.build(&app.view(&cx), width, height, &input, 1.);
                input.synchronize(&scene);
                for message in input.key(&scene, &Key::Tab) {
                    app.update(message, &mut Effects::default());
                }
                let rendered = layout.build(&app.view(&cx), width, height, &input, 1.);
                let hit = rendered
                    .hits
                    .iter()
                    .find(|h| Some(&h.id) == input.focus.as_ref())
                    .unwrap();
                assert!(
                    fully_visible(hit.rect, hit.clip.bounds),
                    "cropped {} at {width}x{height}: {:?} clip {:?}",
                    hit.id,
                    hit.rect,
                    hit.clip.bounds
                );
                visited.insert(hit.id.clone());
            }
            assert_eq!(
                visited.len(),
                count,
                "unreachable audio control at {width}x{height}"
            );
        }
        assert!(matches!(
            app.event(Event::Key {
                surface: "audio",
                key: Key::Escape
            }),
            Some(Message::Audio(AudioMessage::Close))
        ));
        assert!(
            app.event(Event::Key {
                surface: "audio",
                key: Key::Tab
            })
            .is_none()
        );
        app.update(
            Message::Launcher(launcher::Message::Mode(Mode::Apps)),
            &mut Effects::default(),
        );
        assert!(!app.audio.open());
    }
}

#[test]
fn network_overlay_keeps_one_keyboard_owner_and_masks_the_form_in_all_public_state() {
    use crate::network::{
        Message as N,
        tests::{fixture as network_fixture, id},
    };
    let mut app = fixture(0, false);
    let mut effects = Effects::default();
    app.update(
        Message::Network(N::Observed(Ok(network_fixture()))),
        &mut effects,
    );
    app.update(Message::Network(N::Open), &mut effects);
    assert!(!app.launcher.open());
    assert!(!app.audio.open());
    app.update(Message::Network(N::Join(id(2))), &mut effects);
    app.update(
        Message::Network(N::Password(
            1,
            InputEdit::Insert("private-test-value".into()),
        )),
        &mut effects,
    );
    let layout = Layout::new(fonts(&app));
    let cx = ViewContext {
        surface: "network",
        width: 390.,
        height: 580.,
        now: 1.,
    };
    let tree = app.view(&cx);
    let mut input = Interaction::default();
    input.synchronize(&layout.build(&tree, cx.width, cx.height, &input, cx.now));
    let scene = layout.build(&tree, cx.width, cx.height, &input, cx.now);
    assert_eq!(input.focus.as_deref(), Some("network-password-1"));
    assert!(
        scene
            .hits
            .iter()
            .all(|hit| !hit.value.contains("private-test-value"))
    );
    assert!(!app.inspect().contains("private-test-value"));
    assert!(
        scene
            .paint
            .iter()
            .any(|p| matches!(p,Paint::Text{text,..} if text=="••••••••••••••••••"))
    );
    assert!(
        scene
            .paint
            .iter()
            .all(|p| !matches!(p,Paint::Text{text,..} if text.contains("private-test-value")))
    );
    assert_eq!(
        app.surfaces()
            .iter()
            .filter(|s| s.visible && s.keyboard == Keyboard::Exclusive)
            .count(),
        1
    );
    app.update(Message::Audio(crate::audio::Message::Open), &mut effects);
    assert!(!app.network.open());
    assert_eq!(app.network.inspect()["form"], serde_json::Value::Null);
    let network = app
        .surfaces()
        .into_iter()
        .find(|s| s.id == "network")
        .unwrap();
    assert_eq!(network.pointer_input, PointerInput::None);
    assert_eq!(network.keyboard, Keyboard::None);
    app.update(Message::Network(N::Open), &mut effects);
    app.update(
        Message::Launcher(launcher::Message::Mode(Mode::Themes)),
        &mut effects,
    );
    assert!(!app.network.open());
    assert!(app.launcher.open());
}

#[test]
fn bluetooth_overlay_routes_keyboard_submission_without_exposing_input_or_competing_focus() {
    use crate::bluetooth::{
        Message as B,
        tests::{pairing_fixture, pairing_id},
    };
    use lucent_domain::BluetoothPairingPrompt;
    let mut app = fixture(0, false);
    let mut effects = Effects::default();
    app.update(
        Message::Network(crate::network::Message::Open),
        &mut effects,
    );
    app.update(
        Message::Bluetooth(B::Observed(Ok(pairing_fixture(
            BluetoothPairingPrompt::PinCode,
        )))),
        &mut effects,
    );
    assert!(app.bluetooth.open());
    assert!(!app.network.open());
    assert!(!app.audio.open());
    assert!(!app.launcher.open());
    assert_eq!(
        app.surfaces()
            .iter()
            .filter(|s| s.visible && s.keyboard == Keyboard::Exclusive)
            .count(),
        1
    );
    let layout = Layout::new(fonts(&app));
    let cx = ViewContext {
        surface: "bluetooth",
        width: 390.,
        height: 580.,
        now: 1.,
    };
    let mut input = Interaction::default();
    let scene = layout.build(&app.view(&cx), cx.width, cx.height, &input, cx.now);
    input.synchronize(&scene);
    assert_eq!(input.focus.as_deref(), Some("bluetooth-input-41"));
    for message in input.key(&scene, &Key::Text("fixturePin1".into())) {
        app.update(message, &mut effects);
    }
    let scene = layout.build(&app.view(&cx), cx.width, cx.height, &input, cx.now);
    assert!(
        scene
            .paint
            .iter()
            .all(|p| !matches!(p,Paint::Text{text,..}if text.contains("fixturePin1")))
    );
    assert!(scene.hits.iter().all(|h| !h.value.contains("fixturePin1")));
    assert!(!app.inspect().contains("fixturePin1"));
    let submitted = input.key(&scene, &Key::Enter);
    assert!(
        matches!(submitted.as_slice(),[Message::Bluetooth(B::Submit(id))] if *id==pairing_id(41))
    );
    for message in submitted {
        app.update(message, &mut effects);
    }
    assert_eq!(effects.tasks.last().unwrap().queue, crate::work::CONTROLS);
    app.update(Message::Bluetooth(B::Close), &mut effects);
    assert!(!app.bluetooth.open());
    assert_eq!(app.bluetooth.inspect()["form"], serde_json::Value::Null);
    let surface = app
        .surfaces()
        .into_iter()
        .find(|s| s.id == "bluetooth")
        .unwrap();
    assert_eq!(surface.keyboard, Keyboard::None);
    assert_eq!(surface.pointer_input, PointerInput::None);
    assert!(app.command("bluetooth open").is_ok());
    assert!(
        app.runtime_failure(RuntimeFailure {
            source: "effect queue bluetooth".into(),
            reason: "fixture panic".into()
        })
        .is_none()
    );
}
#[test]
fn launcher_pins_are_native_actions_and_hidden_dock_keeps_only_its_hotspot() {
    let mut app = fixture(6, false);
    app.settings_writable = true;
    let mut effects = Effects::default();
    app.update(
        Message::Launcher(launcher::Message::Mode(Mode::Apps)),
        &mut effects,
    );
    effects.tasks.clear();
    let layout = Layout::new(fonts(&app));
    let rendered = scene(&app, &layout, &mut Interaction::default(), 1.);
    let id = app.apps[4].id.clone();
    let hit = rendered
        .hits
        .iter()
        .find(|h| h.id == format!("pin-{}", id.as_str()))
        .unwrap();
    let message = hit.click.clone().unwrap();
    app.update(message, &mut effects);
    assert!(app.settings.pinned.contains(&id));
    assert!(
        effects
            .tasks
            .iter()
            .all(|t| t.queue != crate::work::CONTROLS)
    );
    assert!(app.launcher.open());
    let mut hidden_app = fixture(6, false);
    hidden_app.launcher = Default::default();
    hidden_app.settings.dock.auto_hide = true;
    hidden_app.dock.fixture_hidden();
    let tree = hidden_app.view(&ViewContext {
        surface: "dock",
        width: 640.,
        height: 100.,
        now: 10.,
    });
    let hidden = layout.build(&tree, 640., 100., &Interaction::default(), 10.);
    assert!(hidden.paint.is_empty());
    assert_eq!(hidden.hits.len(), 1);
    assert_eq!(hidden.hits[0].id, "dock-hotspot");
}
#[test]
fn live_catalog_updates_keep_query_selected_identity_and_missing_saved_pins() {
    let mut app = fixture(6, false);
    let mut effects = Effects::default();
    app.update(
        Message::Launcher(launcher::Message::Mode(Mode::Apps)),
        &mut effects,
    );
    app.update(
        Message::Launcher(launcher::Message::Query("r".into())),
        &mut effects,
    );
    let selected = app
        .launcher
        .selected_application(&app.apps)
        .unwrap()
        .clone();
    let missing = AppId::new("missing.desktop").unwrap();
    app.settings.pinned = vec![missing.clone()];
    let mut next = app.apps.clone();
    next.reverse();
    app.update(
        Message::Catalog(crate::catalog::Message::Observed(Ok(next))),
        &mut effects,
    );
    assert_eq!(app.launcher.query(), "r");
    assert_eq!(
        app.launcher.selected_application(&app.apps),
        Some(&selected)
    );
    assert_eq!(app.settings.pinned, vec![missing]);
    let key = Event::Key {
        surface: "dock",
        key: Key::Shortcut {
            character: 'p',
            modifiers: KeyModifiers {
                control: true,
                ..Default::default()
            },
        },
    };
    assert!(
        matches!(app.event(key),Some(Message::Dock(crate::dock::Message::Pin(id)))if id==selected)
    );
}

#[test]
fn native_widget_resize_uses_the_grip_and_preserves_source_capture_until_release() {
    use crate::widgets::Message as W;
    let mut app = fixture(0, false);
    resize_fixture(&mut app, 800., 600.);
    app.settings_writable = true;
    app.settings.visible_widgets = vec!["notes".into()];
    app.settings.positions.insert(
        "notes".into(),
        lucent_domain::Placement::new(128., 160.).unwrap(),
    );
    let layout = Layout::new(fonts(&app));
    let group = app.widgets.surfaces(&app.settings, app.viewport)[0].clone();
    let cx = ViewContext {
        surface: group.id,
        width: group.rect.w,
        height: group.rect.h,
        now: 1.,
    };
    let mut input = Interaction::default();
    let ordinary = layout.build(&app.view(&cx), cx.width, cx.height, &input, cx.now);
    assert!(
        !ordinary
            .hits
            .iter()
            .any(|hit| hit.id == "widget-resize-notes")
    );
    app.update(Message::Widgets(W::Edit(true)), &mut Effects::default());
    let group = app.widgets.surfaces(&app.settings, app.viewport)[0].clone();
    let cx = ViewContext {
        surface: group.id,
        width: group.rect.w,
        height: group.rect.h,
        now: 1.,
    };
    let before = layout.build(&app.view(&cx), cx.width, cx.height, &input, cx.now);
    let grip = before
        .hits
        .iter()
        .find(|hit| hit.id == "widget-resize-notes")
        .unwrap();
    let (x, y) = (
        grip.rect.x + grip.rect.w / 2.,
        grip.rect.y + grip.rect.h / 2.,
    );
    assert!(grip.contains(x, y));
    let card = before
        .hits
        .iter()
        .find(|hit| hit.id == "widget-notes")
        .unwrap();
    for hit in before
        .hits
        .iter()
        .filter(|hit| hit.id.starts_with("widget-size-") || hit.id.starts_with("widget-resize-"))
    {
        assert!(
            hit.rect.y
                >= card.rect.y
                    + app
                        .widgets
                        .size("notes", &app.settings, app.viewport, true)
                        .height()
        );
        assert!(fully_visible(hit.rect, card.rect));
    }
    let message = app
        .event(Event::PointerButton {
            surface: group.id,
            button: MouseButton::Primary,
            pressed: true,
            x,
            y,
        })
        .unwrap();
    app.update(message, &mut Effects::default());
    input.press(&before, x, y);
    for message in input.motion(&before, x + 53., y + 47., cx.now) {
        app.update(message, &mut Effects::default());
    }
    assert!(app.widgets.grid_visible());
    assert!(app.settings.widget_sizes.is_empty());
    let surfaces = app.surfaces();
    let preview = surfaces.iter().find(|spec| spec.id == "widgets").unwrap();
    assert!(preview.visible);
    assert_eq!(preview.pointer_input, PointerInput::None);
    assert_eq!(preview.keyboard, Keyboard::None);
    let during = layout.build(&app.view(&cx), cx.width, cx.height, &input, cx.now);
    assert!(during.paint.is_empty());
    assert_eq!(before.regions, during.regions);
    input.synchronize(&during);
    app.update(
        input.release(x + 53., y + 47.).unwrap(),
        &mut Effects::default(),
    );
    assert_eq!(app.settings.widget_sizes["notes"].tuple(), (320., 208.));
    assert!(!app.widgets.grid_visible());
    assert!(
        app.surfaces()
            .iter()
            .find(|spec| spec.id == group.id)
            .unwrap()
            .width
            > group.rect.w as u32
    );
}

#[test]
fn widget_editor_routes_launcher_keyboard_size_controls_escape_and_checked_ipc() {
    use crate::widgets::Message as W;
    let mut app = fixture(0, false);
    resize_fixture(&mut app, 800., 600.);
    app.settings_writable = true;
    app.settings.visible_widgets = vec!["notes".into()];
    app.update(
        Message::Launcher(launcher::Message::Mode(Mode::Widgets)),
        &mut Effects::default(),
    );
    app.update(
        Message::Launcher(launcher::Message::Select(
            crate::widgets::catalog::WIDGETS.len() + 1,
        )),
        &mut Effects::default(),
    );
    let mut effects = Effects::default();
    app.update(Message::Launcher(launcher::Message::Activate), &mut effects);
    assert!(app.widgets.editing());
    assert!(!app.launcher.open());
    assert!(effects.tasks.is_empty());
    let group = app.widgets.surfaces(&app.settings, app.viewport)[0].clone();
    let cx = ViewContext {
        surface: group.id,
        width: group.rect.w,
        height: group.rect.h,
        now: 1.,
    };
    let layout = Layout::new(fonts(&app));
    let mut input = Interaction::default();
    let scene = layout.build(&app.view(&cx), cx.width, cx.height, &input, cx.now);
    input.focus = Some("widget-size-notes-1".into());
    let messages = input.key(&scene, &Key::Enter);
    assert_eq!(messages.len(), 1);
    for message in messages {
        app.update(message, &mut Effects::default());
    }
    assert_eq!(app.settings.widget_sizes["notes"].tuple(), (288., 176.));
    for command in [
        "widgets size notes 0 300",
        "widgets size unknown 320 240",
        "widgets size notes NaN 300",
        "widgets size notes 400",
    ] {
        assert!(app.command(command).is_err());
    }
    app.update(
        app.command("widgets size notes 400 240").unwrap().unwrap(),
        &mut Effects::default(),
    );
    assert_eq!(app.settings.widget_sizes["notes"].tuple(), (400., 240.));
    let exit = app
        .event(Event::Key {
            surface: group.id,
            key: Key::Escape,
        })
        .unwrap();
    assert!(matches!(exit, Message::Widgets(W::Edit(false))));
    app.update(exit, &mut Effects::default());
    assert!(!app.widgets.editing());
}
