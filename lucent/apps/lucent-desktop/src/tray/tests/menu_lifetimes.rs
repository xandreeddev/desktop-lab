use super::*;

fn menu_client() -> (Tray, Arc<HeaderAdapter>) {
    let adapter = Arc::new(HeaderAdapter::default());
    let mut tray = Tray::new(adapter.clone(), test_adapters::adapters().assets);
    let mut snapshot = fixture();
    snapshot.items.truncate(1);
    let mut effects = Effects::default();
    observe(&mut tray, snapshot, &mut effects);
    tray.update(Message::Open, &mut effects);
    assert!(effects.tasks.is_empty());
    (tray, adapter)
}

fn observe(tray: &mut Tray, snapshot: TraySnapshot, effects: &mut Effects<Message>) {
    tray.update(Message::Observed(Ok(snapshot)), effects);
    // Icon refresh is independently accepted work. Finish only that queue;
    // menu tests still require an exact TRAY task count and captured command.
    while let Some(index) = effects
        .tasks
        .iter()
        .position(|task| task.queue == crate::work::ASSETS)
    {
        let task = effects.tasks.remove(index);
        tray.update(task.run(), effects);
    }
    assert!(tray.icon_active.is_none());
    assert!(tray.icon_pending.is_none());
}

fn complete(tray: &mut Tray, effects: &mut Effects<Message>) {
    assert_eq!(effects.tasks.len(), 1);
    let task = effects.tasks.remove(0);
    assert_eq!(task.queue, crate::work::TRAY);
    tray.update(task.run(), effects);
}

fn open_menu(tray: &mut Tray, item: TrayItemId, effects: &mut Effects<Message>) {
    tray.update(Message::Menu(item, 0), effects);
    complete(tray, effects);
    assert_eq!(tray.menu.as_ref().unwrap().item, item);
}

#[test]
fn captured_menu_row_cannot_invoke_the_same_number_in_another_application() {
    for keyboard in [false, true] {
        let (mut tray, adapter) = menu_client();
        observe(&mut tray, fixture(), &mut Effects::default());
        let mut effects = Effects::default();
        let first = TrayItemId::new(1).unwrap();
        let second = TrayItemId::new(2).unwrap();
        open_menu(&mut tray, first, &mut effects);
        let stale = control_message(&mut tray, "tray-menu-1", keyboard);
        open_menu(&mut tray, second, &mut effects);

        tray.update(stale, &mut effects);
        assert!(effects.tasks.is_empty());
        assert_eq!(tray.menu.as_ref().unwrap().item, second);
        let current = control_message(&mut tray, "tray-menu-1", keyboard);
        tray.update(current, &mut effects);
        complete(&mut tray, &mut effects);
        assert_eq!(
            *adapter.0.lock().unwrap(),
            [
                HeaderCall::Menu(first, 0),
                HeaderCall::Menu(second, 0),
                HeaderCall::Invoke(second, 3, 1),
            ]
        );
        assert!(!tray.open());
    }
}

#[test]
fn closing_and_reopening_the_same_native_menu_retires_row_and_back_callbacks() {
    let (mut tray, adapter) = menu_client();
    let mut effects = Effects::default();
    let item = TrayItemId::new(1).unwrap();
    open_menu(&mut tray, item, &mut effects);
    let row = control_message(&mut tray, "tray-menu-1", false);
    let back = control_message(&mut tray, "tray-back", false);
    tray.update(Message::Close, &mut effects);
    tray.update(Message::Open, &mut effects);
    open_menu(&mut tray, item, &mut effects);

    tray.update(row, &mut effects);
    tray.update(back, &mut effects);
    assert!(effects.tasks.is_empty());
    assert!(tray.menu.is_some());
    let current = control_message(&mut tray, "tray-menu-1", false);
    tray.update(current, &mut effects);
    complete(&mut tray, &mut effects);
    assert_eq!(
        *adapter.0.lock().unwrap(),
        [
            HeaderCall::Menu(item, 0),
            HeaderCall::Menu(item, 0),
            HeaderCall::Invoke(item, 3, 1),
        ]
    );
}

#[test]
fn observed_menu_replacement_cannot_rebind_an_old_row_to_the_new_revision() {
    let (mut tray, adapter) = menu_client();
    let mut effects = Effects::default();
    let item = TrayItemId::new(1).unwrap();
    open_menu(&mut tray, item, &mut effects);
    let stale = control_message(&mut tray, "tray-menu-1", false);
    let mut snapshot = tray.snapshot.clone();
    snapshot.items[0].menu_generation += 1;
    observe(&mut tray, snapshot, &mut effects);
    assert!(tray.menu.is_none());
    let mut changed = menu_fixture();
    changed.revision = 4;
    changed.root.children[0].label = "Different action".into();
    *adapter.1.lock().unwrap() = Some(changed);
    open_menu(&mut tray, item, &mut effects);

    tray.update(stale, &mut effects);
    assert!(effects.tasks.is_empty());
    let current = control_message(&mut tray, "tray-menu-1", false);
    tray.update(current, &mut effects);
    complete(&mut tray, &mut effects);
    assert_eq!(
        adapter.0.lock().unwrap().last(),
        Some(&HeaderCall::Invoke(item, 4, 1))
    );
}

#[test]
fn initial_menu_load_retires_when_its_observed_generation_or_capability_changes() {
    for change in 0..4 {
        let (mut tray, adapter) = menu_client();
        let mut effects = Effects::default();
        let item = TrayItemId::new(1).unwrap();
        tray.update(Message::Menu(item, 0), &mut effects);
        let late = effects.tasks.remove(0).run();
        assert!(tray.menu.is_none());
        assert!(tray.pending_menu.is_some());
        let mut snapshot = tray.snapshot.clone();
        match change {
            0 => snapshot.items[0].menu_generation += 1,
            1 => snapshot.items[0].has_menu = false,
            2 => snapshot.items[0].status = TrayStatus::Passive,
            _ => snapshot.items.clear(),
        }
        observe(&mut tray, snapshot, &mut effects);
        assert!(tray.menu.is_none());
        assert!(tray.pending_menu.is_none());
        assert!(tray.active.is_none());

        let mut current = fixture();
        current.items.truncate(1);
        current.items[0].menu_generation = 2;
        observe(&mut tray, current, &mut effects);
        tray.update(Message::Menu(item, 0), &mut effects);
        let active = tray.active;
        tray.update(late, &mut effects);
        assert_eq!(tray.active, active);
        assert!(tray.menu.is_none());
        assert!(tray.pending_menu.is_some());
        complete(&mut tray, &mut effects);
        assert_eq!(tray.menu.as_ref().unwrap().item, item);
        assert_eq!(
            *adapter.0.lock().unwrap(),
            [HeaderCall::Menu(item, 0), HeaderCall::Menu(item, 0)]
        );
    }
}

#[test]
fn a_menu_reply_for_another_item_cannot_leave_the_previous_menu_actionable() {
    let (mut tray, _) = menu_client();
    observe(&mut tray, fixture(), &mut Effects::default());
    let mut effects = Effects::default();
    open_menu(&mut tray, TrayItemId::new(1).unwrap(), &mut effects);
    tray.update(Message::Menu(TrayItemId::new(2).unwrap(), 0), &mut effects);
    let revision = tray.active.unwrap();
    tray.update(Message::Loaded(revision, Ok(menu_fixture())), &mut effects);
    assert!(tray.menu.is_none());
    assert!(tray.target.is_none());
    assert!(tray.active.is_none());
    // The actual accepted reply is retired with that failed request identity.
    complete(&mut tray, &mut effects);
    assert!(tray.menu.is_none());
}

fn submenu() -> TrayMenu {
    let mut menu = menu_fixture();
    let mut parent = menu.root.children[0].clone();
    let mut leaf = parent.clone();
    leaf.id = 11;
    leaf.label = "Submenu action".into();
    parent.label = "Submenu".into();
    parent.submenu = true;
    parent.children = vec![leaf];
    menu.root.children = vec![parent];
    menu
}

#[test]
fn submenu_navigation_retires_old_rows_and_back_callbacks_even_when_revisited() {
    let (mut tray, adapter) = menu_client();
    *adapter.1.lock().unwrap() = Some(submenu());
    let mut effects = Effects::default();
    let item = TrayItemId::new(1).unwrap();
    open_menu(&mut tray, item, &mut effects);
    let enter = control_message(&mut tray, "tray-menu-1", false);
    tray.update(enter, &mut effects);
    complete(&mut tray, &mut effects);
    assert_eq!(tray.parents, [0, 1]);
    let leaf = control_message(&mut tray, "tray-menu-11", false);
    let back = control_message(&mut tray, "tray-back", false);
    tray.update(back.clone(), &mut effects);
    assert_eq!(tray.parents, [0]);
    tray.update(leaf.clone(), &mut effects);
    assert!(effects.tasks.is_empty());
    // Even a current root authority cannot name an undisplayed descendant.
    let root = tray.menu_authority(tray.menu.as_ref().unwrap());
    tray.update(Message::Invoke(root, 11), &mut effects);
    assert!(effects.tasks.is_empty());

    let enter = control_message(&mut tray, "tray-menu-1", false);
    tray.update(enter, &mut effects);
    complete(&mut tray, &mut effects);
    tray.update(back, &mut effects);
    tray.update(leaf, &mut effects);
    assert_eq!(tray.parents, [0, 1]);
    assert!(effects.tasks.is_empty());
    let current = control_message(&mut tray, "tray-menu-11", false);
    tray.update(current, &mut effects);
    complete(&mut tray, &mut effects);
    assert_eq!(
        *adapter.0.lock().unwrap(),
        [
            HeaderCall::Menu(item, 0),
            HeaderCall::Menu(item, 1),
            HeaderCall::Menu(item, 1),
            HeaderCall::Invoke(item, 3, 11),
        ]
    );
}

#[test]
fn backing_out_of_a_pending_submenu_retires_its_late_reply() {
    let (mut tray, adapter) = menu_client();
    *adapter.1.lock().unwrap() = Some(submenu());
    let mut effects = Effects::default();
    let item = TrayItemId::new(1).unwrap();
    open_menu(&mut tray, item, &mut effects);
    let enter = control_message(&mut tray, "tray-menu-1", false);
    tray.update(enter, &mut effects);
    let late = effects.tasks.remove(0).run();
    let back = control_message(&mut tray, "tray-back", false);
    tray.update(back, &mut effects);
    tray.update(late, &mut effects);
    assert_eq!(tray.parents, [0]);
    assert!(tray.active.is_none());
    assert!(tray.pending_menu.is_none());
    assert!(tray.menu.is_some());
    let enter = control_message(&mut tray, "tray-menu-1", false);
    tray.update(enter, &mut effects);
    complete(&mut tray, &mut effects);
    assert_eq!(tray.parents, [0, 1]);
}

#[test]
fn closed_menu_callbacks_do_not_retire_a_fresh_direct_bar_action() {
    let (mut tray, adapter) = menu_client();
    let mut effects = Effects::default();
    let item = TrayItemId::new(1).unwrap();
    open_menu(&mut tray, item, &mut effects);
    let back = control_message(&mut tray, "tray-back", false);
    let row = control_message(&mut tray, "tray-menu-1", false);
    tray.update(Message::Close, &mut effects);
    tray.update(bar_message(&tray, MouseButton::Primary), &mut effects);
    let active = tray.active;
    tray.update(back, &mut effects);
    tray.update(row, &mut effects);
    tray.update(Message::Scroll(ScrollEvent { offset: 40. }), &mut effects);
    assert_eq!(tray.active, active);
    assert!(active.is_some());
    assert_eq!(tray.scroll, 0.);
    complete(&mut tray, &mut effects);
    assert!(tray.active.is_none());
    assert!(!tray.open());
    assert_eq!(
        *adapter.0.lock().unwrap(),
        [
            HeaderCall::Menu(item, 0),
            HeaderCall::Activate(item, TrayAction::Activate { x: 0, y: 0 }),
        ]
    );
}

#[test]
fn invalid_tray_offsets_cannot_poison_the_controlled_scroll_view() {
    let (mut tray, _) = menu_client();
    let mut effects = Effects::default();
    tray.update(Message::Scroll(ScrollEvent { offset: 40. }), &mut effects);
    for offset in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        tray.update(Message::Scroll(ScrollEvent { offset }), &mut effects);
        assert_eq!(tray.scroll, 40.);
        header_scene(&tray, &Interaction::default());
    }
    tray.update(Message::Scroll(ScrollEvent { offset: -10. }), &mut effects);
    assert_eq!(tray.scroll, 0.);
}

#[test]
fn becoming_passive_retires_captured_panel_actions() {
    for control in [
        "tray-item-open-1",
        "tray-activate-1",
        "tray-secondary-1",
        "tray-menu-open-1",
    ] {
        let (mut tray, adapter) = menu_client();
        let message = control_message(&mut tray, control, false);
        let mut snapshot = tray.snapshot.clone();
        snapshot.items[0].status = TrayStatus::Passive;
        let mut effects = Effects::default();
        observe(&mut tray, snapshot, &mut effects);
        tray.update(message, &mut effects);
        assert!(effects.tasks.is_empty());
        assert!(adapter.0.lock().unwrap().is_empty());
        assert!(tray.active.is_none());
    }
}
