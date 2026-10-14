use super::*;
use lucent_domain::{NotificationAction, NotificationHistoryStatus, Result, StopSignal};
use std::sync::Mutex;
#[derive(Default)]
struct Adapter {
    calls: Mutex<Vec<NotificationTarget>>,
}
impl NotificationPort for Adapter {
    fn watch(&self, _: &mut dyn FnMut(Result<NotificationSnapshot>), _: &dyn StopSignal) {}
    fn dismiss(&self, target: NotificationTarget) -> Result<()> {
        self.calls.lock().unwrap().push(target);
        Ok(())
    }
    fn invoke(&self, target: NotificationTarget, _: &str) -> Result<()> {
        self.calls.lock().unwrap().push(target);
        Ok(())
    }
    fn set_do_not_disturb(&self, _: bool) -> Result<()> {
        Ok(())
    }
    fn clear_history(&self) -> Result<()> {
        Ok(())
    }
    fn retry_history(&self) -> Result<()> {
        Ok(())
    }
}
fn note(id: u32, revision: u64, app: &str) -> Notification {
    Notification {
        id,
        revision,
        received_unix_ms: revision,
        desktop_entry: None,
        app: app.into(),
        summary: "Fixture event".into(),
        body: "Fixture body".into(),
        actions: vec![NotificationAction {
            id: "open".into(),
            label: "Open".into(),
        }],
        critical: false,
        resident: false,
        transient: false,
        timeout_ms: 0,
    }
}
fn center() -> (Center, Arc<Adapter>) {
    let adapter = Arc::new(Adapter::default());
    (Center::new(adapter.clone()), adapter)
}
#[test]
fn history_groups_active_and_saved_entries_with_stable_selection_and_bounded_pages() {
    let (mut center, _) = center();
    let mut effects = Effects::default();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![note(1, 10, "Editor")],
            history: vec![note(2, 9, "Browser"), note(3, 8, "Editor")],
            ..Default::default()
        })),
        &mut effects,
    );
    let groups = center.groups();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].label, "Editor");
    assert_eq!(groups[0].entries.len(), 2);
    let key = groups[0].key.clone();
    center.update(Message::Group(key.clone()), &mut effects);
    center.update(Message::Page(i32::MAX), &mut effects);
    assert_eq!(center.page, 1);
    center.update(Message::Page(-1), &mut effects);
    assert_eq!(center.page, 0);
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![note(1, 11, "Editor")],
            ..Default::default()
        })),
        &mut effects,
    );
    assert_eq!(center.selected_group, Some(key));
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot::default())),
        &mut effects,
    );
    assert_eq!(center.selected_group, None);
}
#[test]
fn history_reveal_reverses_continuously_and_retires_only_after_settled_submission() {
    let (mut center, _) = center();
    let mut effects = Effects::default();
    center.update(Message::Show, &mut effects);
    assert!(center.visible());
    assert!(center.animating(0.));
    assert!(!center.animating(1.));
    effects.now = 0.1;
    let before = center.reveal.value(effects.now);
    center.update(Message::Hide, &mut effects);
    assert_eq!(center.reveal.value(effects.now), before);
    assert!(center.visible());
    effects.now = 0.12;
    let before = center.reveal.value(effects.now);
    center.update(Message::Show, &mut effects);
    assert_eq!(center.reveal.value(effects.now), before);
    center.update(Message::Settled, &mut effects);
    assert!(center.history_open());
    effects.now = 1.;
    center.update(Message::Hide, &mut effects);
    assert!(center.visible());
    assert!(center.settled(3.));
    effects.now = 3.;
    center.update(Message::Settled, &mut effects);
    assert!(!center.visible());
}
#[test]
fn action_pages_retire_on_replacement_and_history_has_no_action_controls() {
    let (mut center, _) = center();
    let mut effects = Effects::default();
    let mut live = note(1, 1, "Fixture");
    live.actions.push(NotificationAction {
        id: "other".into(),
        label: "Other".into(),
    });
    let old = live.target().unwrap();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![live.clone()],
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(Message::ActionPage(old, -1), &mut effects);
    assert_eq!(center.action_pages[&old], 1);
    live.revision = 2;
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![live],
            ..Default::default()
        })),
        &mut effects,
    );
    assert!(center.action_pages.is_empty());
    center.update(Message::ActionPage(old, 1), &mut effects);
    assert!(center.action_pages.is_empty());
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            history: vec![note(1, 1, "Fixture")],
            history_status: NotificationHistoryStatus::Ready,
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(Message::Show, &mut effects);
    center.update(Message::Group("name:Fixture".into()), &mut effects);
    let tree = center.view(&ViewContext {
        surface: "notifications",
        width: 390.,
        height: 580.,
        now: 1.,
    });
    fn actions<M>(node: &Element<M>) -> bool {
        node.id.starts_with("notification-action-") || node.children.iter().any(actions)
    }
    assert!(!actions(&tree));
}
#[test]
fn current_targets_reach_only_injected_adapter_and_mapped_completion_has_no_ui_io() {
    let (mut center, adapter) = center();
    let mut effects = Effects::default();
    let live = note(1, 5, "Fixture");
    let target = live.target().unwrap();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![live],
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(Message::Invoke(target, "open".into()), &mut effects);
    assert!(adapter.calls.lock().unwrap().is_empty());
    let task = effects.tasks.pop().unwrap();
    let result = task.run();
    center.update(result, &mut effects);
    assert_eq!(*adapter.calls.lock().unwrap(), vec![target]);
}

#[test]
fn retired_or_unavailable_actions_are_not_queued_from_old_scenes() {
    let (mut center, adapter) = center();
    let mut effects = Effects::default();
    let old = note(1, 1, "Fixture").target().unwrap();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![note(1, 2, "Fixture")],
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(Message::Invoke(old, "open".into()), &mut effects);
    center.update(Message::Dismiss(old), &mut effects);
    assert!(effects.tasks.is_empty());
    assert!(adapter.calls.lock().unwrap().is_empty());
    let current = center.snapshot.active[0].target().unwrap();
    center.update(
        Message::Snapshot(Err(lucent_domain::DomainError::Unavailable(
            "fixture".into(),
        ))),
        &mut effects,
    );
    center.update(Message::Invoke(current, "open".into()), &mut effects);
    assert!(effects.tasks.is_empty());
}

#[test]
fn grouped_buttons_scroll_and_close_use_actual_sdk_input_routing() {
    let (mut center, _) = center();
    let mut effects = Effects::default();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            history: (1..8).map(|id| note(id, id as u64, "Fixture")).collect(),
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(Message::Show, &mut effects);
    let fonts = vec![
        fontdue::Font::from_bytes(
            include_bytes!("../../assets/LucentSans.ttf").as_slice(),
            Default::default(),
        )
        .unwrap(),
    ];
    let layout = lucent_ui::Layout::new(Arc::new(fonts));
    let cx = ViewContext {
        surface: "notifications",
        width: 390.,
        height: 580.,
        now: 1.,
    };
    let mut interaction = lucent_ui::Interaction::default();
    let scene = layout.build(&center.view(&cx), cx.width, cx.height, &interaction, cx.now);
    interaction.synchronize(&scene);
    interaction.focus = Some("notification-group-name:Fixture".into());
    for message in interaction.key(&scene, &Key::Enter) {
        center.update(message, &mut effects);
    }
    assert_eq!(center.selected_group.as_deref(), Some("name:Fixture"));
    let scene = layout.build(&center.view(&cx), cx.width, cx.height, &interaction, cx.now);
    interaction.synchronize(&scene);
    interaction.focus = Some("notification-next".into());
    for message in interaction.key(&scene, &Key::Enter) {
        center.update(message, &mut effects);
    }
    assert_eq!(center.page, 1);
    effects.now = 1.;
    interaction.focus = Some("notification-history-close".into());
    for message in interaction.key(&scene, &Key::Enter) {
        center.update(message, &mut effects);
    }
    assert!(!center.history_open());
    assert!(center.visible());
}
