use super::*;
use std::sync::Mutex;
pub fn id(n: u64) -> BluetoothObjectId {
    BluetoothObjectId::new(n).unwrap()
}
pub fn pairing_id(n: u64) -> BluetoothPairingId {
    BluetoothPairingId::new(n).unwrap()
}
pub fn fixture() -> BluetoothSnapshot {
    BluetoothSnapshot {
        adapters: vec![BluetoothAdapter {
            id: id(1),
            address: BluetoothAddress::parse("02:00:00:00:00:01").unwrap(),
            name: "Lucent controller".into(),
            powered: true,
            discoverable: false,
            discovering: false,
            discovery_owned: false,
        }],
        devices: vec![
            BluetoothDevice {
                id: id(2),
                adapter: id(1),
                address: BluetoothAddress::parse("02:00:00:00:00:02").unwrap(),
                name: "Example keyboard".into(),
                icon: Some("input-keyboard".into()),
                paired: false,
                bonded: Some(false),
                trusted: false,
                blocked: false,
                connected: false,
                services_resolved: false,
                rssi: Some(-64),
                battery_percent: Some(75),
            },
            BluetoothDevice {
                id: id(3),
                adapter: id(1),
                address: BluetoothAddress::parse("02:00:00:00:00:03").unwrap(),
                name: "A long headphone name that needs to wrap in a narrow panel".into(),
                icon: Some("audio-headphones".into()),
                paired: true,
                bonded: Some(true),
                trusted: false,
                blocked: false,
                connected: false,
                services_resolved: false,
                rssi: None,
                battery_percent: None,
            },
        ],
        ..BluetoothSnapshot::default()
    }
}
pub fn pairing_fixture(prompt: BluetoothPairingPrompt) -> BluetoothSnapshot {
    BluetoothSnapshot {
        pairing: Some(BluetoothPairingAttempt {
            id: pairing_id(40),
            device: id(2),
            phase: BluetoothPairingPhase::Pairing,
        }),
        pairing_requests: vec![BluetoothPairingRequest {
            id: pairing_id(41),
            device: id(2),
            prompt,
        }],
        ..fixture()
    }
}
#[derive(Default)]
struct Adapter {
    calls: Mutex<Vec<&'static str>>,
    commands: Mutex<Vec<BluetoothCommand>>,
    answers: Mutex<Vec<(BluetoothPairingId, &'static str)>>,
}
impl BluetoothPort for Adapter {
    fn watch(&self, emit: &mut dyn FnMut(Result<BluetoothSnapshot>), _: &dyn StopSignal) {
        emit(Ok(fixture()));
    }
    fn control(&self, command: BluetoothCommand) -> Result<()> {
        self.commands.lock().unwrap().push(command);
        Ok(())
    }
    fn start_pairing(&self, _: BluetoothObjectId) -> Result<BluetoothPairingId> {
        self.calls.lock().unwrap().push("pair");
        Ok(pairing_id(40))
    }
    fn answer_pairing(&self, id: BluetoothPairingId, answer: BluetoothPairingAnswer) -> Result<()> {
        let kind = match answer {
            BluetoothPairingAnswer::Accept => "accept",
            BluetoothPairingAnswer::Reject => "reject",
            BluetoothPairingAnswer::PinCode(pin) => {
                assert_eq!(pin.expose(), "fixturePin1");
                "pin"
            }
            BluetoothPairingAnswer::Passkey(value) => {
                assert_eq!(value.number(), 17);
                "passkey"
            }
        };
        self.answers.lock().unwrap().push((id, kind));
        Ok(())
    }
    fn cancel_pairing(&self, _: BluetoothPairingId) -> Result<()> {
        self.calls.lock().unwrap().push("cancel");
        Ok(())
    }
    fn set_incoming_pairing(&self, _: bool) -> Result<()> {
        self.calls.lock().unwrap().push("incoming");
        Ok(())
    }
}
fn client() -> (Bluetooth, Arc<Adapter>, Effects<Message>) {
    let adapter = Arc::new(Adapter::default());
    let mut client = Bluetooth::new(adapter.clone());
    let mut effects = Effects::default();
    client.update(Message::Observed(Ok(fixture())), &mut effects);
    client.update(Message::Open, &mut effects);
    (client, adapter, effects)
}
#[test]
fn observation_and_acknowledgement_never_change_radio_state_or_authority() {
    let (mut client, adapter, mut effects) = client();
    assert!(adapter.calls.lock().unwrap().is_empty());
    client.update(
        Message::Control(BluetoothCommand::SetPowered {
            adapter: id(1),
            powered: false,
        }),
        &mut effects,
    );
    let task = effects.tasks.remove(0);
    assert_eq!(task.queue, crate::work::BLUETOOTH);
    client.update(task.run(), &mut effects);
    assert!(client.snapshot().unwrap().adapters[0].powered);
    assert!(!client.snapshot().unwrap().incoming_pairing_enabled);
    client.update(Message::Pair(id(3)), &mut effects);
    assert!(effects.tasks.is_empty());
    assert!(!client.error.is_empty());
}
#[test]
fn pairing_answers_use_an_independent_lane_and_consume_input_once() {
    let (mut client, adapter, mut effects) = client();
    client.update(
        Message::Control(BluetoothCommand::Connect(id(3))),
        &mut effects,
    );
    let ordinary = effects.tasks.remove(0);
    client.update(
        Message::Observed(Ok(pairing_fixture(BluetoothPairingPrompt::PinCode))),
        &mut effects,
    );
    client.update(
        Message::Edit(pairing_id(41), InputEdit::Insert("fixturePin1".into())),
        &mut effects,
    );
    assert_eq!(client.form.as_ref().unwrap().length(), 11);
    assert!(!client.inspect().to_string().contains("fixturePin1"));
    client.update(Message::Submit(pairing_id(41)), &mut effects);
    assert_eq!(client.form.as_ref().unwrap().length(), 0);
    let response = effects.tasks.remove(0);
    assert_ne!(ordinary.queue, response.queue);
    assert_eq!(response.queue, crate::work::CONTROLS);
    client.update(response.run(), &mut effects);
    assert!(
        client.active.is_some(),
        "A response must not await the pending ordinary call"
    );
    assert_eq!(
        *adapter.answers.lock().unwrap(),
        vec![(pairing_id(41), "pin")]
    );
    client.update(Message::Submit(pairing_id(41)), &mut effects);
    assert!(
        effects.tasks.is_empty(),
        "Empty consumed input cannot be retried as a credential"
    );
    client.update(ordinary.run(), &mut effects);
    assert!(!client.snapshot().unwrap().devices[1].connected);
}
#[test]
fn request_retirement_drops_input_and_late_events_cannot_edit_the_replacement() {
    let (mut client, _, mut effects) = client();
    let mut snapshot = pairing_fixture(BluetoothPairingPrompt::Passkey);
    client.update(Message::Observed(Ok(snapshot.clone())), &mut effects);
    client.update(
        Message::Edit(pairing_id(41), InputEdit::Insert("000017".into())),
        &mut effects,
    );
    snapshot.pairing_requests[0].id = pairing_id(42);
    client.update(Message::Observed(Ok(snapshot)), &mut effects);
    assert_eq!(client.form.as_ref().unwrap().length(), 0);
    client.update(
        Message::Edit(pairing_id(41), InputEdit::Insert("123".into())),
        &mut effects,
    );
    client.update(Message::Decision(pairing_id(41), true), &mut effects);
    assert!(effects.tasks.is_empty());
    assert_eq!(client.form.as_ref().unwrap().length(), 0);
    client.update(
        Message::Observed(Err(DomainError::Unavailable("offline".into()))),
        &mut effects,
    );
    assert!(client.form.is_none());
    assert_eq!(client.status(), "Bluetooth unavailable");
}
#[test]
fn closing_declines_unresolved_requests_and_cancels_only_a_live_attempt() {
    let (mut client, adapter, mut effects) = client();
    client.update(
        Message::Observed(Ok(pairing_fixture(BluetoothPairingPrompt::PinCode))),
        &mut effects,
    );
    client.update(
        Message::Edit(pairing_id(41), InputEdit::Insert("fixturePin1".into())),
        &mut effects,
    );
    client.update(Message::Close, &mut effects);
    assert!(client.form.is_none());
    assert!(!client.open());
    let close = client.close_generation;
    let task = effects.tasks.remove(0);
    client.update(task.run(), &mut effects);
    assert_eq!(
        *adapter.answers.lock().unwrap(),
        vec![(pairing_id(41), "reject")]
    );
    assert_eq!(*adapter.calls.lock().unwrap(), vec!["cancel"]);
    client.update(Message::Open, &mut effects);
    client.update(Message::Closed(close), &mut effects);
    assert!(
        client.visible(),
        "A stale animation timer cannot hide the reopened panel"
    );
}
#[test]
fn input_replace_bounds_and_confirmation_do_not_retain_or_auto_accept_codes() {
    let (mut client, adapter, mut effects) = client();
    client.update(
        Message::Observed(Ok(pairing_fixture(BluetoothPairingPrompt::Passkey))),
        &mut effects,
    );
    let id = pairing_id(41);
    client.update(
        Message::Edit(id, InputEdit::Insert("123456".into())),
        &mut effects,
    );
    client.update(
        Message::Edit(id, InputEdit::Insert("7".into())),
        &mut effects,
    );
    assert_eq!(client.form.as_ref().unwrap().length(), 6);
    client.update(
        Message::Edit(id, InputEdit::Replace("000017".into())),
        &mut effects,
    );
    client.update(Message::Submit(id), &mut effects);
    let task = effects.tasks.remove(0);
    client.update(task.run(), &mut effects);
    assert_eq!(*adapter.answers.lock().unwrap(), vec![(id, "passkey")]);
    client.update(
        Message::Observed(Ok(pairing_fixture(BluetoothPairingPrompt::ConfirmPasskey(
            BluetoothPairingCode::new(17).unwrap(),
        )))),
        &mut effects,
    );
    assert!(client.form.is_none());
    assert!(effects.tasks.is_empty());
    assert!(!client.inspect().to_string().contains("000017"));
}

#[test]
fn closing_during_pair_admission_cancels_the_returned_attempt_without_reopening() {
    let (mut client, adapter, mut effects) = client();
    client.update(Message::Pair(id(2)), &mut effects);
    let admission = effects.tasks.remove(0);
    client.update(Message::Close, &mut effects);
    client.update(
        Message::Observed(Ok(pairing_fixture(BluetoothPairingPrompt::PinCode))),
        &mut effects,
    );
    assert!(!client.open());
    assert!(client.form.is_none());
    client.update(admission.run(), &mut effects);
    let cancel = effects.tasks.remove(0);
    assert_eq!(cancel.queue, crate::work::CONTROLS);
    client.update(cancel.run(), &mut effects);
    assert_eq!(*adapter.calls.lock().unwrap(), vec!["pair", "cancel"]);
    assert!(!client.open());
}

#[test]
fn closing_releases_an_accepted_scan_even_before_its_owned_snapshot_arrives() {
    let (mut client, adapter, mut effects) = client();
    client.update(
        Message::Control(BluetoothCommand::SetDiscovery {
            adapter: id(1),
            active: true,
        }),
        &mut effects,
    );
    let scan = effects.tasks.remove(0);
    client.update(Message::Close, &mut effects);
    assert!(
        effects.tasks.is_empty(),
        "Release follows the accepted scan on its ordered lane"
    );
    client.update(scan.run(), &mut effects);
    let stop = effects.tasks.remove(0);
    assert_eq!(stop.queue, crate::work::BLUETOOTH);
    client.update(stop.run(), &mut effects);
    assert_eq!(
        *adapter.commands.lock().unwrap(),
        vec![
            BluetoothCommand::SetDiscovery {
                adapter: id(1),
                active: true
            },
            BluetoothCommand::SetDiscovery {
                adapter: id(1),
                active: false
            }
        ]
    );
    assert!(!client.open());
}
