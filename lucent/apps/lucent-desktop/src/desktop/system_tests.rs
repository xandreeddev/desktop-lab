//! Client routing tests: no service sampling, native surfaces or OS IO.
use super::{Desktop, Message};
use crate::widgets;
use lucent_api::{
    Application, Component, DisplayOutput, DragEvent, Effects, OutputName, SurfaceAddress,
    SurfaceInstance,
};
use lucent_domain::{DomainError, MonitorId, Placement, SystemSnapshot, WidgetLayout};
use std::collections::BTreeSet;

fn desktop() -> Desktop {
    let mut app = Desktop::new(crate::test_adapters::adapters());
    app.settings_writable = true;
    app
}
fn id(name: &str) -> MonitorId {
    MonitorId::new(name).unwrap()
}
fn output(name: Option<&str>, global: u32) -> DisplayOutput {
    DisplayOutput {
        global,
        name: name.map(|name| OutputName::new(name).unwrap()),
        logical_size: Some((1920, 1080)),
        integer_scale: 1,
    }
}
fn connect(app: &mut Desktop) {
    app.update(
        Message::Outputs(vec![output(Some("DP-1"), 1), output(Some("DP-2"), 2)]),
        &mut Effects::default(),
    );
}
fn layout(entries: &[(&str, f32, f32)]) -> WidgetLayout {
    WidgetLayout {
        visible_widgets: entries.iter().map(|(name, _, _)| (*name).into()).collect(),
        positions: entries
            .iter()
            .map(|(name, x, y)| ((*name).into(), Placement::new(*x, *y).unwrap()))
            .collect(),
        ..Default::default()
    }
}
fn separated() -> WidgetLayout {
    layout(&[("system", 64., 64.), ("notes", 1400., 700.)])
}
fn transitive() -> WidgetLayout {
    layout(&[
        ("calendar", 64., 64.),
        ("weather", 300., 64.),
        ("system", 540., 64.),
        ("notes", 1400., 700.),
    ])
}
fn sample(cpu_percent: f32) -> SystemSnapshot {
    SystemSnapshot {
        cpu_percent,
        memory_used_mib: 4096,
        memory_total_mib: 16384,
    }
}
fn dispatch(app: &mut Desktop, value: Result<SystemSnapshot, DomainError>) -> Effects<Message> {
    let settings = app.settings.clone();
    let persistence = app.persistence.status(app.settings_writable);
    let mut effects = Effects::default();
    app.update(Message::System(value), &mut effects);
    assert_eq!(
        app.settings, settings,
        "sampling must not create or save profiles"
    );
    assert_eq!(app.persistence.status(app.settings_writable), persistence);
    assert!(
        effects.tasks.is_empty(),
        "sampling must not enqueue other work"
    );
    assert!(effects.clipboard.is_empty());
    assert!(effects.recreate_surfaces.is_empty());
    assert!(!effects.exit);
    effects
}
fn address(role: &'static str, instance: Option<&str>) -> SurfaceAddress {
    SurfaceAddress {
        role,
        instance: instance.map(|name| SurfaceInstance::new(name).unwrap()),
    }
}
fn assert_redraw(app: &Desktop, effects: &Effects<Message>, expected: &[SurfaceAddress]) {
    assert!(
        effects.redraw.is_empty(),
        "no broadcast redraw group is needed"
    );
    assert_eq!(
        effects.redraw_instances,
        expected.iter().cloned().collect::<BTreeSet<_>>()
    );
    let specs = app.surfaces();
    assert!(effects.redraw_instances.iter().all(|address| {
        specs
            .iter()
            .any(|spec| spec.visible && spec.address() == *address)
    }));
}
fn drag(app: &mut Desktop, monitor: Option<&str>, widget: &str) {
    let message = Message::Widgets(widgets::Message::Move(
        widget.into(),
        DragEvent {
            dx: 600.,
            dy: 300.,
            finished: false,
        },
    ));
    let message = if let Some(name) = monitor {
        let monitor_id = id(name);
        Message::OnMonitor(
            app.monitors.connected[&monitor_id].route(&monitor_id),
            Box::new(message),
        )
    } else {
        message
    };
    app.update(message, &mut Effects::default());
}

#[test]
fn system_unchanged_samples_do_not_redraw_but_fractional_and_memory_changes_do() {
    let mut app = desktop();
    app.settings.layout = separated();
    let initial = app.system.clone();
    let effects = dispatch(&mut app, Ok(initial));
    assert_redraw(&app, &effects, &[]);

    for value in [
        sample(20.25),
        sample(20.26), // Both labels round to 20%, but the progress width changes.
        SystemSnapshot {
            memory_used_mib: 4097,
            ..sample(20.26)
        },
        SystemSnapshot {
            memory_total_mib: 32768,
            ..sample(20.26)
        },
    ] {
        let effects = dispatch(&mut app, Ok(value.clone()));
        assert_eq!(app.system, value);
        assert_redraw(&app, &effects, &[address("system", None)]);
        let effects = dispatch(&mut app, Ok(value));
        assert_redraw(&app, &effects, &[]);
    }
}

#[test]
fn system_hidden_updates_retain_observation_without_drawing_and_errors_keep_last_good() {
    let mut app = desktop();
    app.settings.layout = layout(&[("notes", 64., 64.)]);
    app.settings
        .monitor_layouts
        .insert(id("disconnected"), separated());
    let value = sample(35.);
    let effects = dispatch(&mut app, Ok(value.clone()));
    assert_eq!(app.system, value);
    assert_redraw(&app, &effects, &[]);
    let effects = dispatch(&mut app, Err(DomainError::Unavailable("fixture".into())));
    assert_eq!(app.system, value);
    assert_redraw(&app, &effects, &[]);
}

#[test]
fn system_redraw_follows_transitive_membership_and_not_the_widget_role() {
    let mut app = desktop();
    app.settings.layout = transitive();
    let groups = app.widgets.surfaces(&app.settings.layout, app.viewport);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].id, "calendar");
    assert_eq!(groups[0].members, ["calendar", "weather", "system"]);
    assert_eq!(groups[1].members, ["notes"]);
    let calendar =
        widgets::surfaces::layout(&layout(&[("calendar", 64., 64.)]), app.viewport, false);
    let system = widgets::surfaces::layout(&layout(&[("system", 540., 64.)]), app.viewport, false);
    assert!(calendar[0].rect.x + calendar[0].rect.w <= system[0].rect.x);
    let effects = dispatch(&mut app, Ok(sample(35.)));
    assert_redraw(&app, &effects, &[address("calendar", None)]);
}

#[test]
fn system_routes_each_connected_host_using_its_profile_or_borrowed_default() {
    let mut app = desktop();
    app.settings.layout = separated();
    app.settings
        .monitor_layouts
        .insert(id("DP-2"), transitive());
    app.settings
        .monitor_layouts
        .insert(id("disconnected"), separated());
    connect(&mut app);
    assert!(!app.settings.monitor_layouts.contains_key(&id("DP-1")));
    let effects = dispatch(&mut app, Ok(sample(35.)));
    assert_redraw(
        &app,
        &effects,
        &[
            address("system", Some("DP-1")),
            address("calendar", Some("DP-2")),
        ],
    );
    assert!(!app.settings.monitor_layouts.contains_key(&id("DP-1")));
}

#[test]
fn system_does_not_draw_unused_default_or_disconnected_profiles() {
    let mut app = desktop();
    app.settings.layout = separated();
    for name in ["DP-1", "DP-2"] {
        app.settings
            .monitor_layouts
            .insert(id(name), layout(&[("notes", 64., 64.)]));
    }
    app.settings
        .monitor_layouts
        .insert(id("disconnected"), transitive());
    connect(&mut app);
    let effects = dispatch(&mut app, Ok(sample(35.)));
    assert_redraw(&app, &effects, &[]);
    app.update(Message::Outputs(vec![]), &mut Effects::default());
    assert!(!app.monitors.legacy());
    let effects = dispatch(&mut app, Ok(sample(40.)));
    assert_redraw(&app, &effects, &[]);
}

#[test]
fn system_unnamed_fallback_keeps_the_exact_legacy_address() {
    let mut app = desktop();
    app.settings.layout = separated();
    app.update(
        Message::Outputs(vec![output(None, 1)]),
        &mut Effects::default(),
    );
    assert!(app.monitors.legacy());
    assert!(app.monitors.connected.is_empty());
    let effects = dispatch(&mut app, Ok(sample(35.)));
    assert_redraw(&app, &effects, &[address("system", None)]);
}

#[test]
fn system_legacy_capture_redraws_frozen_group_and_painted_grid_preview() {
    let mut app = desktop();
    app.settings.layout = transitive();
    let initial = app.widgets.surfaces(&app.settings.layout, app.viewport);
    drag(&mut app, None, "system");
    assert!(app.widgets.grid_visible());
    let preview = app.widgets.preview_position("system").unwrap();
    assert_ne!(
        preview,
        app.widgets
            .source_position("system", &app.settings.layout, app.viewport)
    );
    let effects = dispatch(&mut app, Ok(sample(35.)));
    assert_redraw(
        &app,
        &effects,
        &[address("calendar", None), address("widgets", None)],
    );
    assert_eq!(
        app.widgets.surfaces(&app.settings.layout, app.viewport),
        initial
    );
    assert_eq!(app.widgets.preview_position("system"), Some(preview));
}

#[test]
fn system_resize_capture_keeps_source_bounds_while_updating_the_painted_preview() {
    let mut app = desktop();
    app.settings.layout = transitive();
    app.update(
        Message::Widgets(widgets::Message::Edit(true)),
        &mut Effects::default(),
    );
    // Entering edit mode cancels stale gestures. A fresh physical press admits
    // the next resize, matching the native pointer path and sizing fixtures.
    app.update(
        Message::Widgets(widgets::Message::Press),
        &mut Effects::default(),
    );
    let initial = app.widgets.surfaces(&app.settings.layout, app.viewport);
    let original_size = app
        .widgets
        .size("system", &app.settings.layout, app.viewport, true);
    app.update(
        Message::Widgets(widgets::Message::Resize(
            "system".into(),
            DragEvent {
                dx: 80.,
                dy: 48.,
                finished: false,
            },
        )),
        &mut Effects::default(),
    );
    assert!(app.widgets.grid_visible());
    let preview_size = app
        .widgets
        .size("system", &app.settings.layout, app.viewport, false);
    assert_ne!(preview_size, original_size);
    let effects = dispatch(&mut app, Ok(sample(35.)));
    assert_redraw(
        &app,
        &effects,
        &[address("calendar", None), address("widgets", None)],
    );
    assert_eq!(
        app.widgets.surfaces(&app.settings.layout, app.viewport),
        initial
    );
    assert_eq!(
        app.widgets
            .size("system", &app.settings.layout, app.viewport, true),
        original_size
    );
    assert_eq!(
        app.widgets
            .size("system", &app.settings.layout, app.viewport, false),
        preview_size
    );
}

#[test]
fn system_named_capture_updates_only_the_host_that_paints_system() {
    let mut app = desktop();
    app.settings.layout = separated();
    app.settings
        .monitor_layouts
        .insert(id("DP-1"), layout(&[("notes", 64., 64.)]));
    app.settings
        .monitor_layouts
        .insert(id("DP-2"), transitive());
    connect(&mut app);
    let monitor = &app.monitors.connected[&id("DP-2")];
    let initial = monitor
        .widgets
        .surfaces(app.widget_layout(Some(&id("DP-2"))), monitor.viewport);
    drag(&mut app, Some("DP-1"), "notes");
    drag(&mut app, Some("DP-2"), "system");
    assert!(
        app.monitors
            .connected
            .values()
            .all(|host| host.widgets.grid_visible())
    );
    let effects = dispatch(&mut app, Ok(sample(35.)));
    assert_redraw(
        &app,
        &effects,
        &[
            address("calendar", Some("DP-2")),
            address("widgets", Some("DP-2")),
        ],
    );
    let monitor = &app.monitors.connected[&id("DP-2")];
    assert_eq!(
        monitor
            .widgets
            .surfaces(app.widget_layout(Some(&id("DP-2"))), monitor.viewport),
        initial
    );
    assert!(monitor.widgets.preview_position("system").is_some());
}
