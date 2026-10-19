//! Real client hint consumers plus an ordinary generic popover recipe.
use super::*;
use lucent_design::{component, font, space};

pub(super) fn fixture(
    name: &str,
    app: &mut Desktop,
    layout: &Layout,
    now: f64,
    width: f32,
    height: f32,
) -> Scene<Message> {
    let mut input = Interaction::default();
    let (tree, target) = if name.starts_with("tooltip-tray") {
        // The shared fixture opens the launcher; a standalone bar belongs to
        // the ordinary desktop only after that foreground panel is closed.
        app.update(
            Message::Launcher(launcher::Message::Close),
            &mut Effects::default(),
        );
        app.bar_visible = true;
        let mut effects = Effects::default();
        app.update(
            Message::Tray(crate::tray::Message::Observed(Ok(
                crate::tray::tests::fixture(),
            ))),
            &mut effects,
        );
        for task in std::mem::take(&mut effects.tasks) {
            app.update(task.run(), &mut effects);
        }
        let tree = app.view(&ViewContext {
            surface: "bar",
            instance: None,
            width,
            height,
            now,
            density: Density::ONE,
        });
        let initial = layout.build(&tree, width, height, &input, now);
        let target = initial
            .hits
            .iter()
            .find(|hit| hit.id.starts_with("bar-tray-item-"))
            .or_else(|| initial.hits.iter().find(|hit| hit.id == "bar-tray"))
            .expect("actual tray icon or narrow-screen overflow control")
            .id
            .clone();
        (tree, Some(target))
    } else if name.starts_with("tooltip-dock") {
        // The native crop is short, but the display viewport remains authoritative.
        app.viewport = (width, 580.);
        app.launcher = launcher::Launcher::default();
        app.settings.pinned = app.apps.iter().take(3).map(|app| app.id.clone()).collect();
        app.apps[0].name = "Terminal with a deliberately long application name".into();
        app.update(
            Message::Launcher(launcher::Message::Close),
            &mut Effects::default(),
        );
        let tree = app.view(&ViewContext {
            surface: "dock",
            instance: None,
            width,
            height,
            now,
            density: Density::ONE,
        });
        (tree, Some("dock-foot.desktop".into()))
    } else {
        let theme = app.theme();
        let opener_width = component::input::HEIGHT * 2.;
        let content = theme.popover(
            Element::column(vec![
                Element::text("Choose a value")
                    .font(font::BODY)
                    .color(theme.on_surface),
                theme
                    .button("First value", Message::Completed(Ok(())))
                    .id("choice-first"),
                theme
                    .button("Second value", Message::Completed(Ok(())))
                    .id("choice-second"),
                theme
                    .button("Cancel", Message::Completed(Ok(())))
                    .id("choice-cancel"),
            ])
            .gap(space::SM)
            .width(Length::Fill),
        );
        let tree = Element::stack(vec![
            theme
                .button("Choose", Message::Completed(Ok(())))
                .size(opener_width, component::input::HEIGHT)
                .at(width - opener_width - space::MD, space::MD)
                .id("choices-opener"),
        ])
        .fill()
        .popover(Popover {
            id: "choices".into(),
            anchor: "choices-opener".into(),
            placement: AnchoredSpec {
                side: OverlaySide::Below,
                align: OverlayAlign::Start,
                gap: component::popover::GAP,
                inset: component::popover::INSET,
            },
            content: Box::new(content),
            dismiss: Message::Completed(Ok(())),
        });
        input.focus = Some("choices-opener".into());
        (theme.apply(tree), None)
    };
    let initial = layout.build(&tree, width, height, &input, now);
    if let Some(target) = target {
        let hit = initial
            .hits
            .iter()
            .find(|hit| hit.id == target)
            .expect("actual hint anchor");
        input.motion(
            &initial,
            hit.rect.x + hit.rect.w / 2.,
            hit.rect.y + hit.rect.h / 2.,
            0.,
        );
        let prepared = layout.build(&tree, width, height, &input, now);
        input.synchronize(&prepared);
        let visible = layout.build(&tree, width, height, &input, now);
        assert_eq!(
            initial.regions, visible.regions,
            "passive label expands no native input"
        );
        assert_eq!(
            initial.hits.len(),
            visible.hits.len(),
            "passive label adds no controls"
        );
        assert!(
            visible.paint.len() > initial.paint.len(),
            "actual client label appears after dwell"
        );
        assert!(
            !input.animating(now),
            "settled label creates no idle frames"
        );
        visible
    } else {
        input.synchronize(&initial);
        // This is the scope route the native runtime offers before global keys.
        assert!(input.popover_key(&initial, &Key::Tab).is_some());
        assert_eq!(input.focus.as_deref(), Some("choice-second"));
        layout.build(&tree, width, height, &input, now)
    }
}

#[test]
fn real_tray_and_dock_hints_are_passive_in_short_native_buffers() {
    let mut app = super::fixture(9, false);
    let layout = Layout::new(fonts(&app));
    for width in [390., 640.] {
        for name in ["tooltip-tray", "tooltip-dock"] {
            let height = if name.ends_with("tray") {
                component::panel::BAR_HEIGHT + component::tooltip::SURFACE_ROOM
            } else {
                crate::shell_layout::dock_surface_height(580.) as f32
            };
            let scene = fixture(name, &mut app, &layout, 1., width, height);
            let label = if name.ends_with("tray") {
                if width < 640. {
                    "Application tray"
                } else {
                    "Application status"
                }
            } else {
                "Terminal with"
            };
            assert!(
                scene
                    .paint
                    .iter()
                    .any(|paint| matches!(paint, Paint::Text { text, .. } if text.contains(label)))
            );
            assert!(!scene.overlays.has_popover());
        }
    }
}

#[test]
fn popover_recipe_keeps_focused_controls_inside_narrow_viewport() {
    let mut app = super::fixture(0, false);
    let layout = Layout::new(fonts(&app));
    for width in [320., 390., 640.] {
        let scene = fixture("popover-controls", &mut app, &layout, 1., width, 320.);
        for id in ["choice-first", "choice-second", "choice-cancel"] {
            let hit = scene.hits.iter().find(|hit| hit.id == id).unwrap();
            assert!(
                fully_visible(hit.rect, hit.clip.bounds),
                "{id} remains reachable"
            );
            assert!(hit.rect.x >= component::popover::INSET);
            assert!(hit.rect.x + hit.rect.w <= width - component::popover::INSET);
        }
    }
}
