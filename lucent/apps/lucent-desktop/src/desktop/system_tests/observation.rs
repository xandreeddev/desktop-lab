use super::*;
use crate::system::Status;

#[test]
fn failure_preserves_last_good_and_identical_recovery_redraws_availability() {
    let mut app = desktop();
    app.settings.layout = separated();
    assert_eq!(app.system.status(), Status::Loading);
    assert!(app.system.snapshot().is_none());
    let reading = sample(35.);
    dispatch(&mut app, Ok(reading.clone()));
    let effects = dispatch(
        &mut app,
        Err(DomainError::Unavailable("first failure".into())),
    );
    assert_eq!(app.system.status(), Status::Unavailable);
    assert_eq!(app.system.snapshot(), Some(&reading));
    assert_redraw(&app, &effects, &[address("system", None)]);
    let effects = dispatch(
        &mut app,
        Err(DomainError::Unavailable("later failure".into())),
    );
    assert_redraw(&app, &effects, &[]);
    assert!(
        app.system.inspect()["error"]
            .as_str()
            .unwrap()
            .contains("later failure")
    );
    let effects = dispatch(&mut app, Ok(reading.clone()));
    assert_eq!(app.system.status(), Status::Ready);
    assert_eq!(app.system.snapshot(), Some(&reading));
    assert_eq!(app.system.inspect()["error"], "");
    assert_redraw(&app, &effects, &[address("system", None)]);
}

#[test]
fn first_failure_and_cpu_warmup_never_manufacture_idle_readings() {
    let mut app = desktop();
    app.settings.layout = separated();
    let effects = dispatch(&mut app, Err(DomainError::Unavailable("not ready".into())));
    assert_redraw(&app, &effects, &[address("system", None)]);
    assert!(app.system.snapshot().is_none());
    let warmup = SystemSnapshot {
        cpu_percent: None,
        ..sample(0.)
    };
    let effects = dispatch(&mut app, Ok(warmup.clone()));
    assert_redraw(&app, &effects, &[address("system", None)]);
    assert_eq!(app.system.status(), Status::Ready);
    assert_eq!(app.system.snapshot().unwrap().cpu_percent, None);
    let effects = dispatch(&mut app, Ok(sample(0.)));
    assert_redraw(&app, &effects, &[address("system", None)]);
    assert_eq!(app.system.snapshot().unwrap().cpu_percent, Some(0.));
    let effects = dispatch(&mut app, Ok(warmup));
    assert_redraw(&app, &effects, &[address("system", None)]);
    assert_eq!(app.system.snapshot().unwrap().cpu_percent, None);
}

#[test]
fn invalid_injected_samples_become_unavailable_without_poisoning_last_good() {
    let invalid = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1., 101.]
        .into_iter()
        .map(sample)
        .chain([
            SystemSnapshot {
                memory_total_mib: 0,
                ..sample(0.)
            },
            SystemSnapshot {
                memory_used_mib: 16385,
                ..sample(0.)
            },
        ]);
    for value in invalid {
        let mut app = desktop();
        app.settings.layout = separated();
        let good = sample(50.);
        dispatch(&mut app, Ok(good.clone()));
        let effects = dispatch(&mut app, Ok(value.clone()));
        assert_eq!(app.system.status(), Status::Unavailable);
        assert_eq!(app.system.snapshot(), Some(&good));
        assert_redraw(&app, &effects, &[address("system", None)]);
        let effects = dispatch(&mut app, Ok(value));
        assert_redraw(&app, &effects, &[]);
        let effects = dispatch(&mut app, Ok(good));
        assert_redraw(&app, &effects, &[address("system", None)]);
        assert_eq!(app.system.status(), Status::Ready);
    }
}

#[test]
fn availability_changes_follow_each_visible_output_group_and_skip_unrelated_widgets() {
    let mut app = desktop();
    app.settings.layout = separated();
    app.settings
        .monitor_layouts
        .insert(id("DP-2"), transitive());
    app.settings
        .monitor_layouts
        .insert(id("disconnected"), separated());
    connect(&mut app);
    dispatch(&mut app, Ok(sample(35.)));
    for value in [
        Err(DomainError::Unavailable("offline".into())),
        Ok(sample(35.)),
    ] {
        let effects = dispatch(&mut app, value);
        assert_redraw(
            &app,
            &effects,
            &[
                address("system", Some("DP-1")),
                address("calendar", Some("DP-2")),
            ],
        );
    }
}

#[test]
fn hidden_failures_recover_without_drawing_and_capture_status_preserves_source_geometry() {
    let mut app = desktop();
    app.settings.layout = layout(&[("notes", 64., 64.)]);
    for value in [
        Err(DomainError::Unavailable("offline".into())),
        Ok(sample(35.)),
    ] {
        let effects = dispatch(&mut app, value);
        assert_redraw(&app, &effects, &[]);
    }
    app.settings.layout = transitive();
    drag(&mut app, None, "system");
    let geometry = app.widgets.surfaces(&app.settings.layout, app.viewport);
    let preview = app.widgets.preview_position("system");
    for value in [
        Err(DomainError::Unavailable("offline".into())),
        Ok(sample(35.)),
    ] {
        let effects = dispatch(&mut app, value);
        assert_redraw(
            &app,
            &effects,
            &[address("calendar", None), address("widgets", None)],
        );
        assert_eq!(
            app.widgets.surfaces(&app.settings.layout, app.viewport),
            geometry
        );
        assert_eq!(app.widgets.preview_position("system"), preview);
    }
}
