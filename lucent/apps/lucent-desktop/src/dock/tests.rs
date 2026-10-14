use super::*;
#[test]
fn stale_hide_timer_cannot_hide_a_hovered_or_forced_dock() {
    let mut dock = Dock::default();
    let mut settings = DesktopSettings::default();
    let mut effects = Effects::default();
    dock.update(
        Message::AutoHide(true),
        &mut settings,
        &[],
        &[],
        &mut effects,
    );
    let old = dock.hide_generation;
    dock.update(
        Message::Pointer(true),
        &mut settings,
        &[],
        &[],
        &mut effects,
    );
    dock.update(Message::Hide(old), &mut settings, &[], &[], &mut effects);
    assert_eq!(dock.motion.target_value(), 1.);
    dock.update(Message::Forced(true), &mut settings, &[], &[], &mut effects);
    dock.update(
        Message::Pointer(false),
        &mut settings,
        &[],
        &[],
        &mut effects,
    );
    assert!(!dock.hide_pending);
}
fn apps() -> Vec<Application> {
    ["one", "two", "three"]
        .map(|name| Application {
            id: AppId::new(format!("{name}.desktop")).unwrap(),
            name: name.into(),
            description: String::new(),
            keywords: vec![],
            icon: String::new(),
            startup_class: String::new(),
            launch: lucent_domain::ApplicationLaunch::Command(LaunchCommand {
                program: "unused".into(),
                args: vec![],
                directory: None,
                terminal: false,
            }),
        })
        .into()
}
#[test]
fn drag_preview_is_transient_release_commits_order_and_cancel_rejects_old_release() {
    let apps = apps();
    let ids = apps.iter().map(|a| a.id.clone()).collect::<Vec<_>>();
    let mut settings = DesktopSettings {
        pinned: ids.clone(),
        ..Default::default()
    };
    let mut dock = Dock::default();
    let mut effects = Effects::default();
    let step = component::dock_row::ITEM_SIZE + lucent_design::space::SM;
    dock.update(
        Message::Drag(
            ids[0].clone(),
            DragEvent {
                dx: step,
                dy: 0.,
                finished: false,
            },
        ),
        &mut settings,
        &apps,
        &ids,
        &mut effects,
    );
    assert_eq!(settings.pinned, ids);
    assert!(dock.dragging());
    dock.update(Message::Cancel, &mut settings, &apps, &ids, &mut effects);
    dock.update(
        Message::Drag(
            ids[0].clone(),
            DragEvent {
                dx: step,
                dy: 0.,
                finished: true,
            },
        ),
        &mut settings,
        &apps,
        &ids,
        &mut effects,
    );
    assert_eq!(settings.pinned, ids);
    assert!(!dock.dragging());
    dock.update(Message::Press, &mut settings, &apps, &ids, &mut effects);
    assert!(matches!(
        dock.update(
            Message::Drag(
                ids[0].clone(),
                DragEvent {
                    dx: step,
                    dy: 0.,
                    finished: true
                }
            ),
            &mut settings,
            &apps,
            &ids,
            &mut effects
        ),
        Some(Intent::Save)
    ));
    assert_eq!(
        settings.pinned,
        vec![ids[1].clone(), ids[0].clone(), ids[2].clone()]
    );
}
#[test]
fn pinning_is_idempotent_and_preferences_roundtrip() {
    let apps = apps();
    let ids = apps.iter().map(|a| a.id.clone()).collect::<Vec<_>>();
    let mut settings = DesktopSettings::default();
    let mut dock = Dock::default();
    let mut effects = Effects::default();
    assert!(matches!(
        dock.update(
            Message::SetPin(ids[0].clone(), true),
            &mut settings,
            &apps,
            &ids,
            &mut effects
        ),
        Some(Intent::Save)
    ));
    assert!(
        dock.update(
            Message::SetPin(ids[0].clone(), true),
            &mut settings,
            &apps,
            &ids,
            &mut effects
        )
        .is_none()
    );
    dock.update(
        Message::AutoHide(true),
        &mut settings,
        &apps,
        &ids,
        &mut effects,
    );
    let loaded: DesktopSettings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(loaded, settings);
    assert!(loaded.dock.auto_hide);
    assert!(!loaded.dock.default_pins);
}
