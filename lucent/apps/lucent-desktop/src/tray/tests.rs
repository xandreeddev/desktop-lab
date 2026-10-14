use super::*;
use crate::test_adapters;
use lucent_ui::{Interaction, Layout, Scene};
use std::sync::Mutex;

#[derive(Debug, PartialEq, Eq)]
enum HeaderCall {
    Activate(TrayItemId, TrayAction),
    Menu(TrayItemId, i32),
}
#[derive(Default)]
struct HeaderAdapter(Mutex<Vec<HeaderCall>>);
impl TrayPort for HeaderAdapter {
    fn watch(&self, _: &mut dyn FnMut(Result<TraySnapshot>), _: &dyn StopSignal) {}
    fn act(&self, item: TrayItemId, action: TrayAction) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .push(HeaderCall::Activate(item, action));
        Ok(())
    }
    fn menu(&self, item: TrayItemId, parent: i32) -> Result<TrayMenu> {
        self.0.lock().unwrap().push(HeaderCall::Menu(item, parent));
        let mut menu = menu_fixture();
        menu.item = item;
        Ok(menu)
    }
    fn invoke(&self, _: TrayItemId, _: u32, _: i32, _: u32) -> Result<()> {
        panic!("Opening a tray item must not invoke a menu entry")
    }
}
fn header_client(item_is_menu: bool) -> (Tray, Arc<HeaderAdapter>) {
    let adapter = Arc::new(HeaderAdapter::default());
    let mut tray = Tray::new(adapter.clone(), test_adapters::adapters().assets);
    let mut snapshot = fixture();
    snapshot.items.truncate(1);
    snapshot.items[0].item_is_menu = item_is_menu;
    tray.update(Message::Observed(Ok(snapshot)), &mut Effects::default());
    tray.update(Message::Open, &mut Effects::default());
    (tray, adapter)
}
fn header_scene(tray: &Tray, input: &Interaction<Message>) -> Scene<Message> {
    let font = fontdue::Font::from_bytes(
        include_bytes!("../../assets/LucentSans.ttf") as &[u8],
        Default::default(),
    )
    .unwrap();
    let context = ViewContext {
        surface: "tray",
        instance: None,
        width: 640.,
        height: 700.,
        now: 3.,
        density: Density::ONE,
    };
    let theme = lucent_design::Theme::new(false);
    Layout::new(Arc::new(vec![font])).build(
        &theme.apply(tray.view(&context, theme)),
        context.width,
        context.height,
        input,
        context.now,
    )
}
fn header_message(tray: &mut Tray, keyboard: bool) -> Message {
    let mut input = Interaction::default();
    let scene = header_scene(tray, &input);
    let id = "tray-item-open-1";
    if keyboard {
        // Reach the actual control through SDK traversal, rather than assigning
        // a fabricated focus or calling the child action directly.
        for _ in 0..=scene.hits.len() {
            for message in input.key(&scene, &Key::Tab) {
                tray.update(message, &mut Effects::default());
            }
            if input.focus.as_deref() == Some(id) {
                break;
            }
        }
        assert_eq!(input.focus.as_deref(), Some(id));
        let scene = header_scene(tray, &input);
        input.key(&scene, &Key::Enter).pop().unwrap()
    } else {
        let hit = scene.hits.iter().find(|hit| hit.id == id).unwrap();
        let x = hit.rect.x + hit.rect.w / 2.;
        let y = hit.rect.y + hit.rect.h / 2.;
        assert!(hit.contains(x, y));
        assert!(input.press(&scene, x, y).is_none());
        input.release(x, y).unwrap()
    }
}
#[test]
fn item_header_pointer_and_keyboard_use_current_native_activate() {
    for keyboard in [false, true] {
        let (mut tray, adapter) = header_client(false);
        let message = header_message(&mut tray, keyboard);
        let mut effects = Effects::default();
        tray.update(message, &mut effects);
        assert_eq!(effects.tasks.len(), 1);
        assert_eq!(effects.tasks[0].queue, crate::work::TRAY);
        tray.update(effects.tasks.pop().unwrap().run(), &mut Effects::default());
        assert_eq!(
            *adapter.0.lock().unwrap(),
            vec![HeaderCall::Activate(
                TrayItemId::new(1).unwrap(),
                TrayAction::Activate { x: 0, y: 0 }
            )]
        );
        assert!(tray.active.is_none());
        assert!(tray.menu.is_none());
    }
}
#[test]
fn menu_only_item_header_pointer_and_keyboard_open_its_native_menu() {
    for keyboard in [false, true] {
        let (mut tray, adapter) = header_client(true);
        let message = header_message(&mut tray, keyboard);
        let mut effects = Effects::default();
        tray.update(message, &mut effects);
        assert_eq!(effects.tasks.len(), 1);
        assert_eq!(effects.tasks[0].queue, crate::work::TRAY);
        tray.update(effects.tasks.pop().unwrap().run(), &mut Effects::default());
        assert_eq!(
            *adapter.0.lock().unwrap(),
            vec![HeaderCall::Menu(TrayItemId::new(1).unwrap(), 0)]
        );
        assert_eq!(
            tray.menu.as_ref().unwrap().item,
            TrayItemId::new(1).unwrap()
        );
    }
}
#[test]
fn stale_header_input_cannot_act_while_busy_or_after_item_retirement() {
    for keyboard in [false, true] {
        for retired in [false, true] {
            let (mut tray, adapter) = header_client(false);
            // Capture genuine SDK output while this incarnation is actionable;
            // deliver it only after observation/operation state has changed.
            let stale = header_message(&mut tray, keyboard);
            if retired {
                tray.update(
                    Message::Observed(Ok(TraySnapshot::default())),
                    &mut Effects::default(),
                );
            } else {
                tray.active = Some(41);
            }
            let mut effects = Effects::default();
            tray.update(stale, &mut effects);
            assert!(effects.tasks.is_empty());
            assert!(adapter.0.lock().unwrap().is_empty());
            let scene = header_scene(&tray, &Interaction::default());
            let header = scene.hits.iter().find(|hit| hit.id == "tray-item-open-1");
            if retired {
                assert!(header.is_none());
            } else {
                let header = header.unwrap();
                assert!(!header.tab_stop);
                let x = header.rect.x + header.rect.w / 2.;
                let y = header.rect.y + header.rect.h / 2.;
                let mut input = Interaction::default();
                assert!(input.press(&scene, x, y).is_none());
                tray.update(input.release(x, y).unwrap(), &mut effects);
                assert!(effects.tasks.is_empty());
                assert!(adapter.0.lock().unwrap().is_empty());
            }
        }
    }
}
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
