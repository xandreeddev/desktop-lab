//! Root-level dismissal integration using in-memory ports only.
use super::tests::open;
use super::*;
use lucent_api::{Application, Keyboard, PointerInput};
use lucent_domain::*;
use std::sync::{Arc, Mutex};

#[test]
fn shell_hide_releases_every_panel_without_hiding_the_desktop_or_saving() {
    let adapter = Arc::new(crate::test_adapters::Fake::default());
    let mut app = Desktop::new(crate::test_adapters::with(adapter.clone()));
    let settings = serde_json::to_value(&app.settings).unwrap();
    for panel in PANELS {
        app.update(open(panel), &mut Effects::default());
        assert!(app.panel_open(panel), "{panel:?}");
        let message = app.command("shell hide").unwrap().unwrap();
        assert!(matches!(message, Message::DismissPanels));
        app.update(
            message,
            &mut Effects {
                now: 1.,
                ..Effects::default()
            },
        );
        assert!(PANELS.into_iter().all(|p| !app.panel_open(p)), "{panel:?}");
        assert!(app.bar_visible);
        assert_eq!(serde_json::to_value(&app.settings).unwrap(), settings);
        assert!(
            !app.surfaces()
                .iter()
                .any(|surface| surface.visible && surface.keyboard == Keyboard::Exclusive),
            "{panel:?}"
        );
        // Existing fades finish normally, but cannot retain overlay input.
        for surface in app.surfaces().iter().filter(|surface| {
            surface.visible
                && PANELS.iter().any(|p| p.surface() == surface.id)
                && surface.id != "dock"
        }) {
            assert_eq!(surface.pointer_input, PointerInput::None, "{panel:?}");
        }
    }
    assert!(adapter.calls.lock().unwrap().is_empty());
}

#[test]
fn shell_hide_retires_a_closed_tray_bar_action_before_its_late_error() {
    let mut app = Desktop::new(crate::test_adapters::adapters());
    let mut effects = Effects::default();
    app.update(
        Message::Tray(crate::tray::Message::Observed(Ok(
            crate::tray::tests::fixture(),
        ))),
        &mut effects,
    );
    effects.tasks.clear(); // Icon preparation is unrelated to this action.
    app.update(
        Message::Tray(crate::tray::Message::Bar(
            TrayItemId::new(1).unwrap(),
            crate::tray::BarAction::Activate,
        )),
        &mut effects,
    );
    assert!(!app.tray.open());
    assert_eq!(app.tray.inspect()["busy"], true);
    let result = effects.tasks.remove(0).run();
    let Message::Tray(crate::tray::Message::Completed(revision, _)) = result else {
        panic!("expected direct action completion");
    };
    app.update(Message::DismissPanels, &mut effects);
    app.update(
        Message::Tray(crate::tray::Message::Completed(
            revision,
            Err(DomainError::Failed("late fixture failure".into())),
        )),
        &mut effects,
    );
    assert!(!app.tray.open());
    assert_eq!(app.tray.inspect()["busy"], false);
}

#[derive(Default)]
struct DismissAdapters {
    answers: Mutex<Vec<(NetworkCredentialRequestId, bool)>>,
    reverts: Mutex<Vec<DisplayTransactionId>>,
    bluetooth: Mutex<Vec<(&'static str, u64)>>,
}
impl NetworkPort for DismissAdapters {
    fn watch(&self, _: &mut dyn FnMut(Result<NetworkSnapshot>), _: &dyn StopSignal) {}
    fn control(&self, _: NetworkCommand) -> Result<()> {
        Ok(())
    }
    fn answer_credentials(
        &self,
        id: NetworkCredentialRequestId,
        value: Option<WifiCredentials>,
    ) -> Result<()> {
        self.answers.lock().unwrap().push((id, value.is_some()));
        Ok(())
    }
}
impl DisplayPort for DismissAdapters {
    fn read(&self) -> Result<DisplaySnapshot> {
        Ok(crate::displays::tests::fixture())
    }
    fn watch(&self, _: &mut dyn FnMut(Result<DisplaySnapshot>), _: &dyn StopSignal) {}
    fn apply(
        &self,
        _: Vec<DisplayConfiguration>,
        _: Vec<DisplayConfiguration>,
    ) -> Result<DisplayTransactionId> {
        DisplayTransactionId::new(7)
    }
    fn keep(&self, _: DisplayTransactionId) -> Result<()> {
        panic!("dismissal must not keep a preview")
    }
    fn revert(&self, id: DisplayTransactionId) -> Result<()> {
        self.reverts.lock().unwrap().push(id);
        Ok(())
    }
}
impl BluetoothPort for DismissAdapters {
    fn watch(&self, _: &mut dyn FnMut(Result<BluetoothSnapshot>), _: &dyn StopSignal) {}
    fn control(&self, _: BluetoothCommand) -> Result<()> {
        Ok(())
    }
    fn start_connection(&self, _: BluetoothObjectId) -> Result<BluetoothConnectionId> {
        BluetoothConnectionId::new(60)
    }
    fn cancel_connection(&self, id: BluetoothConnectionId) -> Result<()> {
        self.bluetooth
            .lock()
            .unwrap()
            .push(("cancel-connection", id.value()));
        Ok(())
    }
    fn start_pairing(&self, _: BluetoothObjectId) -> Result<BluetoothPairingId> {
        BluetoothPairingId::new(40)
    }
    fn answer_pairing(&self, id: BluetoothPairingId, answer: BluetoothPairingAnswer) -> Result<()> {
        assert!(matches!(answer, BluetoothPairingAnswer::Reject));
        self.bluetooth.lock().unwrap().push(("reject", id.value()));
        Ok(())
    }
    fn cancel_pairing(&self, id: BluetoothPairingId) -> Result<()> {
        self.bluetooth
            .lock()
            .unwrap()
            .push(("cancel-pairing", id.value()));
        Ok(())
    }
    fn set_incoming_pairing(&self, _: bool) -> Result<()> {
        panic!("dismissal must not change radio authority")
    }
}
fn with_dismiss_adapters() -> (Desktop, Arc<DismissAdapters>) {
    let adapter = Arc::new(DismissAdapters::default());
    let mut adapters = crate::test_adapters::adapters();
    adapters.network = adapter.clone();
    adapters.displays = adapter.clone();
    adapters.bluetooth = adapter.clone();
    (Desktop::new(adapters), adapter)
}
fn finish_tasks(app: &mut Desktop, effects: &mut Effects<Message>) {
    let mut completed = 0;
    while !effects.tasks.is_empty() {
        completed += 1;
        assert!(completed <= 12, "unexpected effect recursion");
        let task = effects.tasks.remove(0);
        app.update(task.run(), effects);
    }
}
#[test]
fn shell_hide_rolls_back_known_and_late_display_admissions() {
    for late in [false, true] {
        let (mut app, adapter) = with_dismiss_adapters();
        let mut effects = Effects::default();
        app.update(open(Panel::Displays), &mut effects);
        finish_tasks(&mut app, &mut effects);
        app.update(
            Message::Displays(crate::displays::Message::Mode(
                MonitorId::new("DP-1").unwrap(),
                DisplayMode::new(1280, 720, 60000).unwrap(),
            )),
            &mut effects,
        );
        app.update(
            Message::Displays(crate::displays::Message::Apply),
            &mut effects,
        );
        assert_eq!(effects.tasks.len(), 1);
        let mut task = Some(effects.tasks.remove(0));
        if !late {
            app.update(task.take().unwrap().run(), &mut effects);
        }
        app.update(Message::DismissPanels, &mut effects);
        if late {
            app.update(task.take().unwrap().run(), &mut effects);
        }
        finish_tasks(&mut app, &mut effects);
        assert!(!app.displays.open());
        assert_eq!(
            *adapter.reverts.lock().unwrap(),
            [DisplayTransactionId::new(7).unwrap()]
        );
    }
}
#[test]
fn shell_hide_drops_credentials_and_revokes_the_exact_prompt_during_activation() {
    use crate::network::{
        Message as Network,
        tests::{credential_request, fixture, id},
    };
    let (mut app, adapter) = with_dismiss_adapters();
    let mut effects = Effects::default();
    app.update(
        Message::Network(Network::Observed(Ok(fixture()))),
        &mut effects,
    );
    app.update(open(Panel::Network), &mut effects);
    app.update(
        Message::Network(Network::Request(crate::network::Control::Activate(
            id(3),
            Some(id(1)),
        ))),
        &mut effects,
    );
    let activation = effects.tasks.remove(0);
    let request = credential_request(12);
    app.update(
        Message::Network(Network::Credentials(Ok(vec![request.clone()]))),
        &mut effects,
    );
    let generation = app.network.inspect()["form"]["generation"]
        .as_u64()
        .unwrap();
    app.update(
        Message::Network(Network::Password(
            generation,
            lucent_api::InputEdit::Insert("private fixture password".into()),
        )),
        &mut effects,
    );
    app.update(Message::DismissPanels, &mut effects);
    assert!(app.network.inspect()["form"].is_null());
    app.update(Message::Network(Network::Submit(generation)), &mut effects);
    app.update(
        Message::Network(Network::Credentials(Ok(vec![request.clone()]))),
        &mut effects,
    );
    finish_tasks(&mut app, &mut effects);
    app.update(activation.run(), &mut effects);
    assert!(!app.network.open());
    assert_eq!(*adapter.answers.lock().unwrap(), [(request.id, false)]);
    assert!(!app.inspect().contains("private fixture password"));
}
#[test]
fn shell_hide_cancels_late_bluetooth_connection_without_reopening_for_its_prompt() {
    use crate::bluetooth::{
        Message as Bluetooth,
        tests::{fixture, id},
    };
    let (mut app, adapter) = with_dismiss_adapters();
    let mut effects = Effects::default();
    app.update(
        Message::Bluetooth(Bluetooth::Observed(Ok(fixture()))),
        &mut effects,
    );
    app.update(open(Panel::Bluetooth), &mut effects);
    app.update(Message::Bluetooth(Bluetooth::Connect(id(3))), &mut effects);
    let admission = effects.tasks.remove(0);
    app.update(Message::DismissPanels, &mut effects);
    let mut snapshot = fixture();
    snapshot.pairing_requests.push(BluetoothPairingRequest {
        id: BluetoothPairingId::new(42).unwrap(),
        device: id(3),
        prompt: BluetoothPairingPrompt::PinCode,
    });
    app.update(
        Message::Bluetooth(Bluetooth::Observed(Ok(snapshot))),
        &mut effects,
    );
    app.update(admission.run(), &mut effects);
    finish_tasks(&mut app, &mut effects);
    assert!(!app.bluetooth.open());
    assert_eq!(
        *adapter.bluetooth.lock().unwrap(),
        [("cancel-connection", 60)]
    );
}
#[test]
fn shell_hide_revokes_power_confirmation_instead_of_executing_it() {
    let adapter = Arc::new(crate::test_adapters::Fake::default());
    let mut app = Desktop::new(crate::test_adapters::with(adapter.clone()));
    let mut effects = Effects::default();
    app.update(
        Message::Power(crate::power::Message::Observed(Ok(
            crate::power::tests::fixture(),
        ))),
        &mut effects,
    );
    app.update(open(Panel::Power), &mut effects);
    app.update(
        Message::Power(crate::power::Message::Ask(PowerAction::Suspend)),
        &mut effects,
    );
    app.update(Message::DismissPanels, &mut effects);
    app.update(
        Message::Power(crate::power::Message::Confirm(PowerAction::Suspend)),
        &mut effects,
    );
    assert!(!app.power.open());
    assert!(effects.tasks.is_empty());
    assert!(adapter.calls.lock().unwrap().is_empty());
}
