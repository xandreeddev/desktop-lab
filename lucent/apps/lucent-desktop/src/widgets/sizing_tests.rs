use super::*;
fn send(host: &mut Host, settings: &mut DesktopSettings, message: Message) -> Option<Intent> {
    host.update(
        message,
        Update {
            settings,
            viewport: (800., 600.),
            unix_seconds: 0,
        },
        &mut Effects::default(),
    )
}
fn settings() -> DesktopSettings {
    let mut settings = DesktopSettings {
        visible_widgets: vec!["notes".into()],
        ..Default::default()
    };
    settings
        .positions
        .insert("notes".into(), Placement::new(128., 160.).unwrap());
    settings
}
fn resize(finished: bool) -> Message {
    Message::Resize(
        "notes".into(),
        DragEvent {
            dx: 53.,
            dy: 47.,
            finished,
        },
    )
}
fn begin(host: &mut Host, settings: &mut DesktopSettings) {
    send(host, settings, Message::Edit(true));
    send(host, settings, Message::Press);
}
#[test]
fn resize_preview_freezes_native_sources_and_unrelated_saves_commit_only_old_size() {
    let mut host = Host::default();
    let mut settings = settings();
    begin(&mut host, &mut settings);
    let source = host.surfaces(&settings, (800., 600.));
    assert!(send(&mut host, &mut settings, resize(false)).is_none());
    assert!(host.grid_visible());
    assert!(settings.widget_sizes.is_empty());
    assert_eq!(
        host.size("notes", &settings, (800., 600.), false).tuple(),
        (323., 207.)
    );
    assert_eq!(
        host.size("notes", &settings, (800., 600.), true).tuple(),
        widget_size("notes")
    );
    assert_eq!(host.surfaces(&settings, (800., 600.)), source);
    assert!(matches!(
        send(
            &mut host,
            &mut settings,
            Message::EditNotes(TextEdit::Replace("saved text".into()))
        ),
        Some(Intent::Save)
    ));
    let snapshot = serde_json::to_value(&settings).unwrap();
    assert_eq!(snapshot["widget_sizes"], serde_json::json!({}));
    assert!(matches!(
        send(&mut host, &mut settings, resize(true)),
        Some(Intent::Save)
    ));
    assert_eq!(settings.widget_sizes["notes"].tuple(), (320., 208.));
    assert!(!host.grid_visible());
    assert!(host.surfaces(&settings, (800., 600.))[0].rect.w > source[0].rect.w);
    let restored: DesktopSettings =
        serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
    assert_eq!(restored.widget_sizes, settings.widget_sizes);
}
#[test]
fn hide_reset_exit_and_output_changes_cancel_stale_resize_releases() {
    for cancellation in [
        Message::Toggle("notes".into()),
        Message::ResetLayout,
        Message::Edit(false),
        Message::CancelDrags,
    ] {
        let mut host = Host::default();
        let mut settings = settings();
        begin(&mut host, &mut settings);
        send(&mut host, &mut settings, resize(false));
        send(&mut host, &mut settings, cancellation);
        let committed = settings.clone();
        assert!(!host.grid_visible());
        assert!(send(&mut host, &mut settings, resize(true)).is_none());
        assert_eq!(settings, committed);
    }
    let mut host = Host::default();
    let mut settings = settings();
    begin(&mut host, &mut settings);
    send(&mut host, &mut settings, resize(false));
    send(&mut host, &mut settings, Message::CancelDrags);
    send(&mut host, &mut settings, Message::Press);
    send(&mut host, &mut settings, resize(false));
    assert!(matches!(
        send(&mut host, &mut settings, resize(true)),
        Some(Intent::Save)
    ));
}
#[test]
fn malformed_resize_geometry_cannot_panic_or_mutate_preferences() {
    let mut host = Host::default();
    let mut settings = settings();
    begin(&mut host, &mut settings);
    let before = settings.clone();
    let result = host.update(
        resize(false),
        Update {
            settings: &mut settings,
            viewport: (f32::NAN, 600.),
            unix_seconds: 0,
        },
        &mut Effects::default(),
    );
    assert!(matches!(result, Some(Intent::Error(_))));
    assert_eq!(settings, before);
    assert!(!host.grid_visible());
    send(&mut host, &mut settings, Message::Press);
    assert!(matches!(
        send(
            &mut host,
            &mut settings,
            Message::Resize(
                "notes".into(),
                DragEvent {
                    dx: f32::INFINITY,
                    dy: 0.,
                    finished: false
                }
            )
        ),
        Some(Intent::Error(_))
    ));
    assert_eq!(settings, before);
}
#[test]
fn output_limits_are_presentational_and_explicit_size_changes_cancel_capture() {
    let mut host = Host::default();
    let mut settings = settings();
    settings
        .widget_sizes
        .insert("notes".into(), WidgetSize::new(640., 400.).unwrap());
    assert_eq!(
        host.size("notes", &settings, (320., 240.), false).tuple(),
        (320., 240.)
    );
    assert_eq!(settings.widget_sizes["notes"].tuple(), (640., 400.));
    assert_eq!(
        host.size("notes", &settings, (800., 600.), false).tuple(),
        (640., 400.)
    );
    begin(&mut host, &mut settings);
    send(&mut host, &mut settings, resize(false));
    assert!(matches!(
        send(
            &mut host,
            &mut settings,
            Message::Size("notes".into(), WidgetSize::new(400., 240.).unwrap())
        ),
        Some(Intent::Save)
    ));
    assert!(!host.grid_visible());
    send(&mut host, &mut settings, resize(true));
    assert_eq!(settings.widget_sizes["notes"].tuple(), (400., 240.));
}
#[test]
fn keyboard_size_steps_require_edit_mode_and_unknown_widgets_are_rejected() {
    let mut host = Host::default();
    let mut settings = settings();
    assert!(
        send(
            &mut host,
            &mut settings,
            Message::SizeStep("notes".into(), 1)
        )
        .is_none()
    );
    assert!(settings.widget_sizes.is_empty());
    begin(&mut host, &mut settings);
    assert!(matches!(
        send(
            &mut host,
            &mut settings,
            Message::SizeStep("notes".into(), 1)
        ),
        Some(Intent::Save)
    ));
    assert_eq!(settings.widget_sizes["notes"].tuple(), (288., 176.));
    let before = settings.clone();
    assert!(matches!(
        send(
            &mut host,
            &mut settings,
            Message::Size("unknown".into(), WidgetSize::new(400., 240.).unwrap())
        ),
        Some(Intent::Error(_))
    ));
    assert_eq!(settings, before);
    let old: DesktopSettings = serde_json::from_str("{\"version\":1}").unwrap();
    assert!(old.widget_sizes.is_empty());
    assert!(
        serde_json::from_str::<DesktopSettings>(
            "{\"widget_sizes\":{\"notes\":{\"width\":0,\"height\":200}}}"
        )
        .is_err()
    );
}
