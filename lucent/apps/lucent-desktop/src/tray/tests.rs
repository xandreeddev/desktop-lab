use super::*;
use crate::test_adapters;
#[test]
fn close_and_retirement_reject_late_menu_results() {
    let mut tray = Tray::new(
        test_adapters::adapters().tray,
        test_adapters::adapters().assets,
    );
    let id = TrayItemId::new(1).unwrap();
    tray.open = true;
    tray.mounted = true;
    tray.target = Some(id);
    tray.active = Some(1);
    let mut effects = Effects::default();
    tray.update(Message::Close, &mut effects);
    let menu = TrayMenu {
        item: id,
        revision: 1,
        root: TrayMenuEntry {
            id: 0,
            label: String::new(),
            enabled: true,
            visible: true,
            separator: false,
            check: TrayCheck::None,
            submenu: false,
            children: vec![],
        },
    };
    tray.update(Message::Loaded(1, Ok(menu)), &mut effects);
    assert!(tray.menu.is_none());
    assert!(!tray.open());
}
pub fn fixture() -> TraySnapshot {
    TraySnapshot {
        items: (1..=8)
            .map(|id| TrayItem {
                id: TrayItemId::new(id).unwrap(),
                application: format!("Application {id}"),
                title: format!("Application {id}"),
                tooltip: "Application status and available actions".into(),
                status: if id == 2 {
                    TrayStatus::NeedsAttention
                } else {
                    TrayStatus::Active
                },
                icon: TrayIcon {
                    name: String::new(),
                    pixmap: Some(TrayPixmap::new(2, 2, [64, 180, 160, 255].repeat(4)).unwrap()),
                },
                attention_icon: TrayIcon::default(),
                overlay_icon: TrayIcon::default(),
                item_is_menu: false,
                has_menu: true,
                menu_generation: 0,
            })
            .collect(),
    }
}
pub fn menu_fixture() -> TrayMenu {
    TrayMenu {
        item: TrayItemId::new(1).unwrap(),
        revision: 3,
        root: TrayMenuEntry {
            id: 0,
            label: String::new(),
            enabled: true,
            visible: true,
            separator: false,
            check: TrayCheck::None,
            submenu: true,
            children: (1..=16)
                .map(|id| TrayMenuEntry {
                    id,
                    label: format!("_Action {id}"),
                    enabled: id != 2,
                    visible: id != 3,
                    separator: id == 4,
                    check: if id == 5 {
                        TrayCheck::Check(true)
                    } else {
                        TrayCheck::None
                    },
                    submenu: false,
                    children: vec![],
                })
                .collect(),
        },
    }
}
fn client() -> Tray {
    let adapters = test_adapters::adapters();
    Tray::new(adapters.tray, adapters.assets)
}
#[test]
fn closed_retired_and_disabled_controls_do_not_schedule_app_actions() {
    let mut tray = client();
    let mut effects = Effects::default();
    tray.update(Message::Observed(Ok(fixture())), &mut effects);
    effects.tasks.clear();
    let id = TrayItemId::new(1).unwrap();
    tray.update(
        Message::Act(id, TrayAction::Activate { x: 0, y: 0 }),
        &mut effects,
    );
    assert!(effects.tasks.is_empty());
    tray.update(Message::Open, &mut effects);
    tray.update(
        Message::Act(
            TrayItemId::new(99).unwrap(),
            TrayAction::Activate { x: 0, y: 0 },
        ),
        &mut effects,
    );
    assert!(effects.tasks.is_empty());
    tray.menu = Some(menu_fixture());
    tray.target = Some(id);
    tray.parents = vec![0];
    for entry in [0, 2, 3, 4, 99] {
        tray.update(Message::Invoke(entry), &mut effects);
        assert!(effects.tasks.is_empty());
    }
    tray.update(Message::Invoke(1), &mut effects);
    assert_eq!(effects.tasks.len(), 1);
    assert_eq!(effects.tasks[0].queue, crate::work::TRAY);
}
#[test]
fn icon_preparation_coalesces_and_discards_superseded_results() {
    let mut tray = client();
    let mut effects = Effects::default();
    tray.update(Message::Observed(Ok(fixture())), &mut effects);
    let old = effects.tasks.remove(0);
    let mut changed = fixture();
    changed.items[0].icon = TrayIcon::default();
    tray.update(Message::Observed(Ok(changed)), &mut effects);
    assert!(effects.tasks.is_empty());
    tray.update(old.run(), &mut effects);
    assert!(tray.images.is_empty());
    assert_eq!(effects.tasks.len(), 1);
    tray.update(effects.tasks.remove(0).run(), &mut Effects::default());
    assert!(!tray.images.contains_key("tray:1:base"));
    assert!(tray.images.contains_key("tray:2:base"));
}
#[test]
fn observed_menu_change_discards_the_client_menu() {
    let mut tray = client();
    let mut effects = Effects::default();
    tray.update(Message::Observed(Ok(fixture())), &mut effects);
    tray.menu = Some(menu_fixture());
    tray.target = Some(TrayItemId::new(1).unwrap());
    let mut state = fixture();
    state.items[0].menu_generation = 1;
    tray.update(Message::Observed(Ok(state)), &mut effects);
    assert!(tray.menu.is_none());
    assert!(tray.target.is_none());
}
