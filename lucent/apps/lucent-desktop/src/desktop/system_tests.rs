//! Client routing tests: no service sampling, native surfaces or OS IO.
use super::{Desktop, Message};
use crate::widgets;
use lucent_api::{
    ActivationOutcome, Application, Component, DisplayOutput, DragEvent, Effects,
    InputReleaseOutcome, Keyboard, KeyboardFocusChange, KeyboardFocusOutcome, MouseButton,
    NativeSurfaceToken, OutputName, PointerPress, SurfaceAddress, SurfaceInstance, SurfaceSpec,
};
use lucent_domain::{
    DomainError, MonitorId, Notification, NotificationSnapshot, NotificationTarget, Placement,
    SystemSnapshot, WidgetLayout,
};
use std::collections::{BTreeMap, BTreeSet};

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
fn system_first_and_repeated_unchanged_samples_do_not_redraw_the_standalone_bar() {
    let mut app = desktop();
    app.settings.layout = separated();
    let initial = app.system.clone();
    for _ in 0..2 {
        let effects = dispatch(&mut app, Ok(initial.clone()));
        assert_redraw(&app, &effects, &[]);
        assert!(app.bar_presentation.standalone(None));
    }
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

// These assertions establish client intent, request cancellation and routing.
// Native first-click/immediate-key and same-click outside delivery require the
// compositor trial; constructing an effect is not a focus acknowledgement.
fn widget_specs(app: &Desktop) -> Vec<SurfaceSpec> {
    app.surfaces()
        .into_iter()
        .filter(|spec| widgets::catalog::find(spec.id).is_some())
        .collect()
}
fn widget_keyboard(app: &Desktop) -> BTreeMap<SurfaceAddress, Keyboard> {
    widget_specs(app)
        .into_iter()
        .map(|spec| (spec.address(), spec.keyboard))
        .collect()
}
fn widget_host<'a>(app: &'a Desktop, monitor: Option<&str>) -> &'a widgets::Host {
    match monitor {
        Some(name) => &app.monitors.connected[&id(name)].widgets,
        None => &app.widgets,
    }
}
fn press_member(
    app: &mut Desktop,
    monitor: Option<&str>,
    member: &str,
    focusable: bool,
) -> Effects<Message> {
    let message = Message::Widgets(widgets::Message::PressMember(
        member.into(),
        PointerPress {
            button: MouseButton::Primary,
            keyboard_focusable: focusable,
        },
    ));
    let message = match monitor {
        Some(name) => Message::OnMonitor(
            app.monitors.connected[&id(name)].route(&id(name)),
            Box::new(message),
        ),
        None => message,
    };
    let mut effects = Effects::default();
    app.update(message, &mut effects);
    effects
}
fn assert_suppressed_widgets(app: &Desktop, baseline: &[SurfaceSpec]) {
    let expected: Vec<_> = baseline
        .iter()
        .cloned()
        .map(|mut spec| {
            spec.keyboard = Keyboard::None;
            spec
        })
        .collect();
    assert!(!expected.is_empty());
    assert_eq!(widget_specs(app), expected, "only keyboard demand changes");
}
fn foreground_openers() -> Vec<(&'static str, Message)> {
    vec![
        ("dock", Message::Launcher(crate::launcher::Message::Toggle)),
        ("audio", Message::Audio(crate::audio::Message::Open)),
        (
            "media-controls",
            Message::Media(crate::media::Message::Open),
        ),
        (
            "weather-controls",
            Message::Weather(crate::weather::Message::Open),
        ),
        ("network", Message::Network(crate::network::Message::Open)),
        (
            "bluetooth",
            Message::Bluetooth(crate::bluetooth::Message::Open),
        ),
        ("power", Message::Power(crate::power::Message::Open)),
        (
            "displays",
            Message::Displays(crate::displays::Message::Open),
        ),
        ("tray", Message::Tray(crate::tray::Message::Open)),
        (
            "live-wallpaper",
            Message::LiveWallpaper(crate::live_wallpaper::Message::Open),
        ),
        (
            "notifications",
            Message::Notifications(crate::notifications::Message::Show),
        ),
    ]
}
fn link_notification() -> Notification {
    Notification {
        id: 1,
        revision: 1,
        received_unix_ms: 1,
        desktop_entry: None,
        app: "Fixture".into(),
        summary: "Fixture link".into(),
        body: "<a href='https://example.test/details'>Details</a>".into(),
        image: None,
        actions: vec![],
        critical: false,
        resident: false,
        transient: false,
        timeout_ms: 0,
    }
}
fn show_link_history(app: &mut Desktop) -> NotificationTarget {
    let note = link_notification();
    let target = note.target().unwrap();
    app.update(
        Message::Notifications(crate::notifications::Message::Snapshot(Ok(
            NotificationSnapshot {
                history: vec![note],
                ..Default::default()
            },
        ))),
        &mut Effects::default(),
    );
    app.update(
        Message::Notifications(crate::notifications::Message::Show),
        &mut Effects::default(),
    );
    target
}
fn begin_link(app: &mut Desktop, target: NotificationTarget) -> Effects<Message> {
    let mut effects = Effects::default();
    app.update(
        Message::Notifications(crate::notifications::Message::OpenLink(
            target,
            "https://example.test/details".into(),
        )),
        &mut effects,
    );
    effects
}

#[test]
fn widget_keyboard_defaults_preserve_controls_and_exclude_passive_content() {
    for definition in widgets::catalog::WIDGETS.iter() {
        let widget = definition.id;
        let mut app = desktop();
        app.settings.layout = layout(&[(widget, 64., 64.)]);
        assert_eq!(
            widget_keyboard(&app),
            BTreeMap::from([(address(widget, None), Keyboard::None)]),
            "capability is not intent: {widget}"
        );
        let effects = press_member(&mut app, None, widget, definition.keyboard_interactive);
        assert_eq!(
            effects.keyboard_focus.len(),
            usize::from(definition.keyboard_interactive)
        );
        assert_eq!(
            widget_keyboard(&app),
            BTreeMap::from([(
                address(widget, None),
                if definition.keyboard_interactive {
                    Keyboard::OnDemand
                } else {
                    Keyboard::None
                }
            )]),
            "{widget}"
        );
    }
}

#[test]
fn widget_keyboard_uses_pressed_merged_member_and_rechecks_after_hide() {
    let mut app = desktop();
    app.settings.layout = layout(&[("clock", 64., 64.), ("notes", 64., 64.)]);
    let groups = app.widgets.surfaces(&app.settings.layout, app.viewport);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].members, ["clock", "notes"]);
    let effects = press_member(&mut app, None, "notes", true);
    assert_eq!(effects.keyboard_focus.len(), 1);
    assert_eq!(
        widget_keyboard(&app),
        BTreeMap::from([(address("clock", None), Keyboard::OnDemand)])
    );
    // Same native group, different actual member: a passive clock press clears
    // Notes instead of borrowing its retained component focus or capability.
    assert!(
        press_member(&mut app, None, "clock", false)
            .keyboard_focus
            .is_empty()
    );
    assert_eq!(
        widget_keyboard(&app),
        BTreeMap::from([(address("clock", None), Keyboard::None)])
    );
    let mut effects = press_member(&mut app, None, "notes", true);
    let request = effects.keyboard_focus.pop().unwrap();
    app.update(
        Message::Widgets(widgets::Message::Toggle("notes".into())),
        &mut Effects::default(),
    );
    assert!(request.cancellation.cancelled());
    assert_eq!(
        widget_keyboard(&app),
        BTreeMap::from([(address("clock", None), Keyboard::None)])
    );
}

#[test]
fn widget_keyboard_keeps_edit_controls_available_without_bypassing_foreground() {
    let mut app = desktop();
    app.settings.layout = layout(&[("clock", 64., 64.), ("system", 900., 64.)]);
    app.update(
        Message::Widgets(widgets::Message::Edit(true)),
        &mut Effects::default(),
    );
    assert!(
        widget_specs(&app)
            .iter()
            .all(|spec| spec.keyboard == Keyboard::None)
    );
    let mut effects = press_member(&mut app, None, "clock", true);
    assert_eq!(effects.keyboard_focus.len(), 1);
    let cancel = effects.keyboard_focus.pop().unwrap().cancellation;
    let editing = widget_specs(&app);
    assert_eq!(
        editing
            .iter()
            .filter(|spec| spec.keyboard == Keyboard::OnDemand)
            .count(),
        1
    );
    app.update(
        Message::Widgets(widgets::Message::SizeStep("clock".into(), 1)),
        &mut Effects::default(),
    );
    assert!(
        !cancel.cancelled(),
        "keyboard edit action retains explicit ownership"
    );
    let editing = widget_specs(&app);
    app.update(
        Message::Audio(crate::audio::Message::Open),
        &mut Effects::default(),
    );
    assert!(cancel.cancelled());
    assert_suppressed_widgets(&app, &editing);
    app.update(Message::DismissPanels, &mut Effects::default());
    assert_suppressed_widgets(&app, &editing);
}

#[test]
fn widget_keyboard_every_foreground_panel_suppresses_the_legacy_host() {
    for (role, opener) in foreground_openers() {
        let mut app = desktop();
        app.settings.layout = separated();
        press_member(&mut app, None, "notes", true);
        let baseline = widget_specs(&app);
        assert_eq!(baseline.len(), 2);
        app.update(opener, &mut Effects::default());
        assert_eq!(app.panel_surface(), Some(role));
        assert_suppressed_widgets(&app, &baseline);
        app.update(Message::DismissPanels, &mut Effects::default());
        assert!(app.panel_surface().is_none());
        assert_suppressed_widgets(&app, &baseline);
    }
}

#[test]
fn widget_keyboard_foreground_suppresses_every_connected_monitor_group() {
    for (role, opener) in foreground_openers() {
        let mut app = desktop();
        app.settings.layout = separated();
        app.settings
            .monitor_layouts
            .insert(id("DP-2"), transitive());
        connect(&mut app);
        press_member(&mut app, Some("DP-2"), "notes", true);
        let baseline = widget_specs(&app);
        assert_eq!(baseline.len(), 4);
        assert!(baseline.iter().all(|spec| spec.instance.is_some()));
        app.update(opener, &mut Effects::default());
        assert_eq!(app.panel_surface(), Some(role));
        assert_suppressed_widgets(&app, &baseline);
        app.update(Message::DismissPanels, &mut Effects::default());
        assert_suppressed_widgets(&app, &baseline);
        app.update(
            Message::Outputs(vec![output(Some("DP-2"), 2)]),
            &mut Effects::default(),
        );
        assert_eq!(
            widget_specs(&app),
            baseline
                .into_iter()
                .filter(|spec| spec.instance.as_ref().unwrap().as_str() == "DP-2")
                .map(|mut spec| {
                    spec.keyboard = Keyboard::None;
                    spec
                })
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn widget_keyboard_passive_toasts_do_not_suppress_notes() {
    let mut app = desktop();
    app.settings.layout = separated();
    press_member(&mut app, None, "notes", true);
    let baseline = widget_specs(&app);
    app.update(
        Message::Notifications(crate::notifications::Message::Snapshot(Ok(
            NotificationSnapshot {
                active: vec![link_notification()],
                ..Default::default()
            },
        ))),
        &mut Effects::default(),
    );
    assert!(app.notifications.visible());
    assert!(app.panel_surface().is_none());
    assert_eq!(widget_specs(&app), baseline);
}

#[test]
fn widget_keyboard_foreground_changes_preserve_each_hosts_drag_capture() {
    for monitor in [None, Some("DP-2")] {
        let mut app = desktop();
        app.settings.layout = separated();
        if monitor.is_some() {
            connect(&mut app);
        }
        drag(&mut app, monitor, "notes");
        let preview = widget_host(&app, monitor).preview_position("notes");
        assert!(preview.is_some());
        assert!(widget_host(&app, monitor).grid_visible());
        let frozen = widget_specs(&app);
        app.update(
            Message::Audio(crate::audio::Message::Open),
            &mut Effects::default(),
        );
        assert_suppressed_widgets(&app, &frozen);
        assert!(widget_host(&app, monitor).grid_visible());
        assert_eq!(
            widget_host(&app, monitor).preview_position("notes"),
            preview
        );
        app.update(Message::DismissPanels, &mut Effects::default());
        assert_eq!(widget_specs(&app), frozen);
        assert!(widget_host(&app, monitor).grid_visible());
        assert_eq!(
            widget_host(&app, monitor).preview_position("notes"),
            preview
        );
    }
}

#[test]
fn widget_keyboard_history_suppression_spans_both_ack_orders_and_uri_dispatch() {
    for release_first in [false, true] {
        let mut app = desktop();
        app.settings.layout = separated();
        press_member(&mut app, None, "notes", true);
        let baseline = widget_specs(&app);
        let target = show_link_history(&mut app);
        let mut effects = begin_link(&mut app, target);
        let token = effects
            .activation
            .pop()
            .unwrap()
            .finish(ActivationOutcome::Unavailable);
        let released = effects
            .input_release
            .pop()
            .unwrap()
            .finish(InputReleaseOutcome::Released)
            .unwrap();
        assert_suppressed_widgets(&app, &baseline);
        let (first, second) = if release_first {
            (released, token)
        } else {
            (token, released)
        };
        app.update(first, &mut effects);
        assert!(effects.tasks.is_empty());
        assert_suppressed_widgets(&app, &baseline);
        app.update(second, &mut effects);
        assert!(app.notifications.history_open());
        assert!(app.notifications.yielding_input());
        assert_eq!(effects.tasks.len(), 1);
        assert_suppressed_widgets(&app, &baseline);
        let task = effects.tasks.pop().unwrap();
        assert_eq!(task.queue, crate::work::CONTROLS);
        let done = task.run(); // Injected in-memory URI adapter only.
        assert!(app.notifications.history_open());
        assert_suppressed_widgets(&app, &baseline);
        app.update(done, &mut effects);
        assert!(!app.notifications.history_open());
        assert_suppressed_widgets(&app, &baseline);
    }
}

#[test]
fn widget_keyboard_cancelled_link_cannot_override_a_new_foreground_panel() {
    let mut app = desktop();
    app.settings.layout = separated();
    press_member(&mut app, None, "notes", true);
    let baseline = widget_specs(&app);
    let target = show_link_history(&mut app);
    let mut effects = begin_link(&mut app, target);
    let release = effects.input_release.pop().unwrap();
    let token = effects
        .activation
        .pop()
        .unwrap()
        .finish(ActivationOutcome::Unavailable);
    app.update(Message::Audio(crate::audio::Message::Open), &mut effects);
    assert!(release.cancellation.cancelled());
    assert!(release.finish(InputReleaseOutcome::Released).is_none());
    app.update(token, &mut effects);
    assert_eq!(app.panel_surface(), Some("audio"));
    assert!(effects.tasks.is_empty());
    assert_suppressed_widgets(&app, &baseline);
    app.update(Message::DismissPanels, &mut effects);
    assert_suppressed_widgets(&app, &baseline);
}

#[test]
fn widget_keyboard_release_failure_keeps_history_suppression_and_skips_uri_dispatch() {
    for outcome in [InputReleaseOutcome::Timeout, InputReleaseOutcome::Retired] {
        let mut app = desktop();
        app.settings.layout = separated();
        press_member(&mut app, None, "notes", true);
        let baseline = widget_specs(&app);
        let target = show_link_history(&mut app);
        let mut effects = begin_link(&mut app, target);
        let token = effects
            .activation
            .pop()
            .unwrap()
            .finish(ActivationOutcome::Unavailable);
        let failed = effects
            .input_release
            .pop()
            .unwrap()
            .finish(outcome)
            .unwrap();
        app.update(failed, &mut effects);
        app.update(token, &mut effects);
        assert!(app.notifications.history_open());
        assert!(!app.notifications.yielding_input());
        assert!(effects.tasks.is_empty());
        assert_suppressed_widgets(&app, &baseline);
    }
}

#[test]
fn widget_keyboard_uri_adapter_failure_keeps_history_suppression() {
    struct RejectedUri;
    impl lucent_domain::UriPort for RejectedUri {
        fn open(
            &self,
            _: &lucent_domain::ExternalUri,
            _: lucent_domain::ApplicationLaunchContext,
        ) -> lucent_domain::Result<()> {
            Err(DomainError::Failed("Controlled URI failure".into()))
        }
    }
    let mut adapters = crate::test_adapters::adapters();
    adapters.uri = std::sync::Arc::new(RejectedUri);
    let mut app = Desktop::new(adapters);
    app.settings.layout = separated();
    press_member(&mut app, None, "notes", true);
    let baseline = widget_specs(&app);
    let target = show_link_history(&mut app);
    let mut effects = begin_link(&mut app, target);
    let token = effects
        .activation
        .pop()
        .unwrap()
        .finish(ActivationOutcome::Unavailable);
    let released = effects
        .input_release
        .pop()
        .unwrap()
        .finish(InputReleaseOutcome::Released)
        .unwrap();
    app.update(token, &mut effects);
    app.update(released, &mut effects);
    assert_eq!(effects.tasks.len(), 1);
    let task = effects.tasks.pop().unwrap();
    assert_eq!(task.queue, crate::work::CONTROLS);
    app.update(task.run(), &mut effects);
    assert!(app.notifications.history_open());
    assert!(!app.notifications.yielding_input());
    assert_suppressed_widgets(&app, &baseline);
}

#[test]
fn widget_keyboard_foreground_cancels_queued_focus_ack_without_rearming_on_close() {
    let mut app = desktop();
    app.settings.layout = separated();
    let mut effects = press_member(&mut app, None, "notes", true);
    let request = effects.keyboard_focus.pop().unwrap();
    let cancel = request.cancellation.clone();
    let queued = request
        .finish(KeyboardFocusOutcome::Focused(NativeSurfaceToken::new()))
        .unwrap();
    app.update(Message::Audio(crate::audio::Message::Open), &mut effects);
    assert!(cancel.cancelled());
    app.update(queued, &mut effects);
    app.update(Message::DismissPanels, &mut effects);
    assert!(
        widget_specs(&app)
            .iter()
            .all(|spec| spec.keyboard == Keyboard::None)
    );
}

#[test]
fn widget_keyboard_loss_requires_acknowledged_exact_native_lifetime() {
    let mut app = desktop();
    app.settings.layout = separated();
    let mut effects = press_member(&mut app, None, "notes", true);
    let old = NativeSurfaceToken::new();
    let native = NativeSurfaceToken::new();
    for change in [KeyboardFocusChange::Left, KeyboardFocusChange::Retired] {
        let event = lucent_api::Event::KeyboardFocus {
            surface: "notes",
            native: old.clone(),
            change,
        };
        app.update(app.event(event).unwrap(), &mut effects);
        assert_eq!(
            widget_keyboard(&app)[&address("notes", None)],
            Keyboard::OnDemand
        );
    }
    let focused = effects
        .keyboard_focus
        .pop()
        .unwrap()
        .finish(KeyboardFocusOutcome::Focused(native.clone()))
        .unwrap();
    app.update(focused, &mut effects);
    let event = lucent_api::Event::KeyboardFocus {
        surface: "notes",
        native,
        change: KeyboardFocusChange::Left,
    };
    app.update(app.event(event).unwrap(), &mut effects);
    assert!(
        widget_specs(&app)
            .iter()
            .all(|spec| spec.keyboard == Keyboard::None)
    );
}

#[test]
fn widget_keyboard_latest_output_press_replaces_owner_and_retired_route_cannot_rearm() {
    let mut app = desktop();
    app.settings.layout = separated();
    connect(&mut app);
    let mut first = press_member(&mut app, Some("DP-1"), "notes", true);
    let first = first.keyboard_focus.pop().unwrap();
    let mut second = press_member(&mut app, Some("DP-2"), "notes", true);
    assert!(first.cancellation.cancelled());
    assert_eq!(
        widget_specs(&app)
            .iter()
            .filter(|spec| spec.keyboard == Keyboard::OnDemand)
            .map(SurfaceSpec::address)
            .collect::<Vec<_>>(),
        vec![address("notes", Some("DP-2"))]
    );
    let request = second.keyboard_focus.pop().unwrap();
    let cancellation = request.cancellation.clone();
    let queued = request
        .finish(KeyboardFocusOutcome::Focused(NativeSurfaceToken::new()))
        .unwrap();
    app.update(
        Message::Outputs(vec![output(Some("DP-1"), 1)]),
        &mut Effects::default(),
    );
    assert!(cancellation.cancelled());
    app.update(queued, &mut Effects::default());
    assert!(
        widget_specs(&app)
            .iter()
            .all(|spec| spec.keyboard == Keyboard::None)
    );
}

#[test]
fn widget_keyboard_unavailable_request_is_visible_and_never_leaves_eligibility_armed() {
    let mut app = desktop();
    app.settings.layout = separated();
    let mut effects = press_member(&mut app, None, "notes", true);
    let failure = effects
        .keyboard_focus
        .pop()
        .unwrap()
        .finish(KeyboardFocusOutcome::Unavailable)
        .unwrap();
    app.update(failure, &mut effects);
    assert!(app.error.contains("compositor"));
    assert!(
        widget_specs(&app)
            .iter()
            .all(|spec| spec.keyboard == Keyboard::None)
    );
}

#[test]
fn widget_keyboard_delivered_first_notes_edit_keeps_intent_and_programmatic_controls_do_not_arm() {
    let mut app = desktop();
    app.settings.layout = separated();
    app.settings.notes.clear();
    let mut effects = press_member(&mut app, None, "notes", true);
    app.update(
        Message::Widgets(widgets::Message::EditNotes(lucent_api::TextEdit::Insert(
            "a".into(),
        ))),
        &mut effects,
    );
    assert_eq!(app.settings.notes, "a");
    assert_eq!(
        widget_keyboard(&app)[&address("notes", None)],
        Keyboard::OnDemand
    );
    assert_eq!(effects.keyboard_focus.len(), 1);
    app.update(Message::Audio(crate::audio::Message::Open), &mut effects);
    app.update(Message::DismissPanels, &mut effects);
    for message in [
        widgets::Message::Month(1),
        widgets::Message::TimerToggle,
        widgets::Message::TimerReset,
    ] {
        app.update(Message::Widgets(message), &mut effects);
        assert!(
            widget_specs(&app)
                .iter()
                .all(|spec| spec.keyboard == Keyboard::None)
        );
    }
    // This is controlled message ordering, not proof that a native compositor
    // delivered the first physical key; that remains an integration requirement.
}

#[test]
fn widget_keyboard_named_output_transition_cancels_dormant_legacy_intent() {
    let mut app = desktop();
    app.settings.layout = separated();
    let mut effects = press_member(&mut app, None, "notes", true);
    let request = effects.keyboard_focus.pop().unwrap();
    connect(&mut app);
    assert!(request.cancellation.cancelled());
    app.update(
        Message::Outputs(vec![output(None, 99)]),
        &mut Effects::default(),
    );
    assert!(app.monitors.legacy());
    assert!(
        widget_specs(&app)
            .iter()
            .all(|spec| spec.keyboard == Keyboard::None)
    );
}

#[test]
fn desktop_requalifies_retired_native_current_while_new_asset_has_no_frame() {
    use crate::background_presentation::{BackgroundPresentation, Source};
    use crate::presentation_frame::PresentationFrame;
    use lucent_api::{
        Color, Element, ImageData, ImageStream,
        video_frame::{Bt709, Bytes, ChromaSiting, Plane, Range, Transfer, Yuv420Frame},
    };
    use lucent_domain::{LiveWallpaperAsset, LiveWallpaperSettings, Wallpaper};
    use std::sync::Arc;
    let mut app = desktop();
    app.live_wallpaper.enable_native_planes(true);
    let poster = Arc::new(
        ImageData::from_rgba("retired-native-poster", 2, 2, [90, 20, 10, 255].repeat(4)).unwrap(),
    );
    let y = Plane::new(Bytes::from(vec![160; 16]), 4, 4, 4).unwrap();
    let uv = Plane::new(Bytes::from(vec![128; 4]), 2, 2, 2).unwrap();
    let planes = Yuv420Frame::new(
        4,
        4,
        [y, uv.clone(), uv],
        Bt709 {
            range: Range::Full,
            transfer: Transfer::Srgb,
            chroma: ChromaSiting::Left,
        },
    )
    .unwrap();
    let frame = PresentationFrame::native(
        Arc::new(ImageStream::new().frame_yuv420(planes).unwrap()),
        poster.clone(),
    );
    let mut presentation = BackgroundPresentation::default();
    presentation.reconcile_frame(Some(Source::Live("retired".into())), Some(frame), 0.);
    app.backgrounds.insert(None, (0, presentation));
    let source = Wallpaper {
        path: "/fixture/pending.mp4".into(),
        name: "Pending source".into(),
    };
    app.settings.live_wallpapers.default = Some(LiveWallpaperSettings {
        asset: LiveWallpaperAsset {
            source: source.clone(),
            path: source.path,
            poster: Wallpaper {
                path: "/fixture/pending.png".into(),
                name: "Pending poster".into(),
            },
        },
        fps: Default::default(),
        paused: false,
        enabled: true,
    });
    app.update(Message::Ignore, &mut Effects::default());
    let view: Element<()> = app.backgrounds[&None].1.view(1., Color::hex(0));
    let scene = lucent_ui::Layout::new(Arc::new(vec![])).build(
        &view,
        8.,
        8.,
        &lucent_ui::Interaction::default(),
        1.,
    );
    let images: Vec<_> = scene
        .paint
        .iter()
        .filter_map(|paint| match paint {
            lucent_ui::Paint::Image { data, .. } => Some(data),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].cache_id(), poster.cache_id());
    assert!(
        scene
            .paint
            .iter()
            .all(|paint| !matches!(paint, lucent_ui::Paint::Video { .. }))
    );
    assert_eq!(app.live_wallpaper.status(), "Preparing live wallpaper…");
}
