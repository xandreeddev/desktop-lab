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
            pairable: true,
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
    operations: Mutex<Vec<(&'static str, BluetoothPairingId)>>,
    answer_error: Mutex<Option<DomainError>>,
    cancel_error: Mutex<Option<DomainError>>,
}
impl BluetoothPort for Adapter {
    fn watch(&self, emit: &mut dyn FnMut(Result<BluetoothSnapshot>), _: &dyn StopSignal) {
        emit(Ok(fixture()));
    }
    fn control(&self, command: BluetoothCommand) -> Result<()> {
        self.commands.lock().unwrap().push(command);
        Ok(())
    }
    fn start_connection(&self, _: BluetoothObjectId) -> Result<BluetoothConnectionId> {
        self.calls.lock().unwrap().push("connect");
        Ok(BluetoothConnectionId::new(60).unwrap())
    }
    fn cancel_connection(&self, id: BluetoothConnectionId) -> Result<()> {
        assert_eq!(id.value(), 60);
        self.calls.lock().unwrap().push("cancel-connection");
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
        self.operations.lock().unwrap().push(("answer", id));
        self.answer_error.lock().unwrap().take().map_or(Ok(()), Err)
    }
    fn cancel_pairing(&self, id: BluetoothPairingId) -> Result<()> {
        self.calls.lock().unwrap().push("cancel");
        self.operations.lock().unwrap().push(("cancel", id));
        self.cancel_error.lock().unwrap().take().map_or(Ok(()), Err)
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
fn device_actions_are_reachable_and_radio_authority_lives_on_its_own_tab() {
    let (mut client, _, mut effects) = client();
    let theme = lucent_design::Theme::from_palette(&lucent_design::palettes()[0]);
    let cx = lucent_api::ViewContext {
        surface: "bluetooth",
        width: 390.,
        height: 580.,
        now: 1.,
    };
    let fonts = [
        include_bytes!("../../assets/LucentSans.ttf").as_slice(),
        include_bytes!("../../assets/LucentDisplay.ttf").as_slice(),
        include_bytes!("../../assets/GoogleSansFlex.ttf").as_slice(),
    ]
    .into_iter()
    .map(|bytes| fontdue::Font::from_bytes(bytes, Default::default()).unwrap())
    .collect();
    let layout = lucent_ui::Layout::new(Arc::new(fonts));
    let scene = layout.build(
        &client.view(&cx, theme),
        cx.width,
        cx.height,
        &lucent_ui::Interaction::default(),
        cx.now,
    );
    let pair = scene
        .hits
        .iter()
        .find(|h| h.id == "bluetooth-pair-2")
        .unwrap();
    assert!(pair.clip.contains(
        pair.rect.x + pair.rect.w / 2.,
        pair.rect.y + pair.rect.h / 2.
    ));
    assert!(matches!(pair.click, Some(Message::Pair(device)) if device == id(2)));
    assert!(
        !scene
            .hits
            .iter()
            .any(|h| h.id == "bluetooth-visibility" || h.id == "bluetooth-incoming")
    );
    let mut connected = fixture();
    connected.devices[0].connected = true;
    client.update(Message::Observed(Ok(connected)), &mut effects);
    let scene = layout.build(
        &client.view(&cx, theme),
        cx.width,
        cx.height,
        &lucent_ui::Interaction::default(),
        cx.now,
    );
    let disconnect = scene
        .hits
        .iter()
        .find(|h| h.id == "bluetooth-connect-2")
        .unwrap();
    assert!(
        matches!(disconnect.click, Some(Message::Control(BluetoothCommand::Disconnect(device))) if device == id(2))
    );
    assert!(disconnect.clip.contains(
        disconnect.rect.x + disconnect.rect.w / 2.,
        disconnect.rect.y + disconnect.rect.h / 2.
    ));
    assert!(scene.hits.iter().any(|h| h.id == "bluetooth-pair-2"));
    assert!(!scene.hits.iter().any(|h| h.id == "bluetooth-trust-2"));
    client.update(Message::Tab(Tab::Radio), &mut effects);
    let scene = layout.build(
        &client.view(&cx, theme),
        cx.width,
        cx.height,
        &lucent_ui::Interaction::default(),
        cx.now,
    );
    let visibility = scene
        .hits
        .iter()
        .find(|h| h.id == "bluetooth-visibility")
        .unwrap();
    assert!(
        matches!(visibility.click, Some(Message::Control(BluetoothCommand::SetDiscoverable { adapter, discoverable: true })) if adapter == id(1))
    );
    assert!(scene.hits.iter().any(|h| h.id == "bluetooth-incoming"));
    assert!(!scene.hits.iter().any(|h| h.id == "bluetooth-pair-2"));
    assert!(
        effects.tasks.is_empty(),
        "Changing the presentation must not change radio or authority"
    );
}
#[test]
fn radio_controls_wait_for_observation_and_do_not_enable_incoming_authority() {
    let (mut client, adapter, mut effects) = client();
    for command in [
        BluetoothCommand::SetDiscoverable {
            adapter: id(1),
            discoverable: true,
        },
        BluetoothCommand::SetPairable {
            adapter: id(1),
            pairable: false,
        },
    ] {
        client.update(Message::Control(command.clone()), &mut effects);
        let task = effects.tasks.remove(0);
        assert_eq!(task.queue, crate::work::BLUETOOTH);
        client.update(task.run(), &mut effects);
        assert_eq!(adapter.commands.lock().unwrap().last(), Some(&command));
        let current = client.snapshot().unwrap();
        assert!(!current.adapters[0].discoverable && current.adapters[0].pairable);
        assert!(!current.incoming_pairing_enabled);
    }
    let mut changed = fixture();
    changed.adapters[0].discoverable = true;
    changed.adapters[0].pairable = false;
    client.update(Message::Observed(Ok(changed)), &mut effects);
    let current = client.snapshot().unwrap();
    assert!(current.adapters[0].discoverable && !current.adapters[0].pairable);
    assert!(!current.incoming_pairing_enabled);
    assert!(adapter.calls.lock().unwrap().is_empty());
}
#[test]
fn pairing_answers_use_an_independent_lane_and_consume_input_once() {
    let (mut client, adapter, mut effects) = client();
    client.update(Message::Connect(id(3)), &mut effects);
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
fn decline_answers_then_cancels_the_captured_attempt_without_faking_completion() {
    let (mut client, adapter, mut effects) = client();
    client.update(
        Message::Observed(Ok(pairing_fixture(
            BluetoothPairingPrompt::AuthorizePairing,
        ))),
        &mut effects,
    );
    client.update(Message::Decision(pairing_id(41), false), &mut effects);
    let task = effects.tasks.remove(0);
    assert_eq!(task.queue, crate::work::CONTROLS);
    let mut replacement = pairing_fixture(BluetoothPairingPrompt::AuthorizePairing);
    replacement.pairing.as_mut().unwrap().id = pairing_id(50);
    replacement.pairing_requests[0].id = pairing_id(51);
    client.update(Message::Observed(Ok(replacement)), &mut effects);
    *adapter.answer_error.lock().unwrap() = Some(DomainError::Invalid("old prompt retired".into()));
    client.update(task.run(), &mut effects);
    assert_eq!(
        *adapter.operations.lock().unwrap(),
        vec![("answer", pairing_id(41)), ("cancel", pairing_id(40))]
    );
    assert!(client.error.is_empty());
    assert_eq!(
        client.snapshot().unwrap().pairing.as_ref().unwrap().id,
        pairing_id(50)
    );
    assert_eq!(client.inspect()["pairing"]["phase"], "pairing");
}
#[test]
fn rejection_preserves_native_failures_and_attempts_cancellation_after_delivery_failure() {
    let (mut client, adapter, mut effects) = client();
    client.update(
        Message::Observed(Ok(pairing_fixture(
            BluetoothPairingPrompt::AuthorizePairing,
        ))),
        &mut effects,
    );
    *adapter.answer_error.lock().unwrap() =
        Some(DomainError::Unavailable("delivery uncertain".into()));
    client.update(Message::Decision(pairing_id(41), false), &mut effects);
    let task = effects.tasks.remove(0);
    client.update(task.run(), &mut effects);
    assert_eq!(
        *adapter.operations.lock().unwrap(),
        vec![("answer", pairing_id(41)), ("cancel", pairing_id(40))]
    );
    assert!(client.error.contains("delivery uncertain"));
    *adapter.cancel_error.lock().unwrap() =
        Some(DomainError::Unavailable("cancellation uncertain".into()));
    client.update(Message::Decision(pairing_id(41), false), &mut effects);
    let task = effects.tasks.remove(0);
    client.update(task.run(), &mut effects);
    assert!(client.error.contains("cancellation uncertain"));
}
#[test]
fn incoming_rejection_and_display_cancellation_do_not_cancel_an_unrelated_attempt_twice() {
    let (mut client, adapter, mut effects) = client();
    for prompt in [
        BluetoothPairingPrompt::AuthorizePairing,
        BluetoothPairingPrompt::DisplayPasskey {
            code: BluetoothPairingCode::new(17).unwrap(),
            entered: 0,
        },
    ] {
        let mut snapshot = pairing_fixture(prompt.clone());
        if prompt == BluetoothPairingPrompt::AuthorizePairing {
            snapshot.pairing_requests[0].device = id(3);
        }
        client.update(Message::Observed(Ok(snapshot)), &mut effects);
        client.update(Message::Decision(pairing_id(41), false), &mut effects);
        let task = effects.tasks.remove(0);
        client.update(task.run(), &mut effects);
    }
    assert_eq!(
        *adapter.operations.lock().unwrap(),
        vec![("answer", pairing_id(41)), ("answer", pairing_id(41))]
    );
    assert!(adapter.calls.lock().unwrap().is_empty());
    assert!(!client.inspect().to_string().contains("000017"));
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

pub fn connecting_fixture() -> BluetoothSnapshot {
    BluetoothSnapshot {
        connection: Some(BluetoothConnectionAttempt {
            id: BluetoothConnectionId::new(60).unwrap(),
            device: id(3),
            phase: BluetoothConnectionPhase::Connecting,
        }),
        ..fixture()
    }
}

#[test]
fn pending_connection_cancel_is_reachable_while_ordinary_controls_are_disabled() {
    let (mut client, _, mut effects) = client();
    let theme = lucent_design::Theme::from_palette(&lucent_design::palettes()[0]);
    let fonts = [
        include_bytes!("../../assets/LucentSans.ttf").as_slice(),
        include_bytes!("../../assets/LucentDisplay.ttf").as_slice(),
        include_bytes!("../../assets/GoogleSansFlex.ttf").as_slice(),
    ]
    .into_iter()
    .map(|bytes| fontdue::Font::from_bytes(bytes, Default::default()).unwrap())
    .collect();
    let layout = lucent_ui::Layout::new(Arc::new(fonts));
    for width in [320., 640.] {
        let cx = lucent_api::ViewContext {
            surface: "bluetooth",
            width,
            height: 580.,
            now: 1.,
        };
        client.update(Message::Observed(Ok(connecting_fixture())), &mut effects);
        let scene = layout.build(
            &client.view(&cx, theme),
            width,
            cx.height,
            &lucent_ui::Interaction::default(),
            cx.now,
        );
        let cancel = scene
            .hits
            .iter()
            .find(|h| h.id == "bluetooth-cancel-connection")
            .unwrap();
        assert!(matches!(cancel.click, Some(Message::CancelConnection(id)) if id.value() == 60));
        assert!(
            cancel.tab_stop,
            "Keyboard users must be able to reach cancellation"
        );
        assert!(cancel.clip.contains(
            cancel.rect.x + cancel.rect.w / 2.,
            cancel.rect.y + cancel.rect.h / 2.
        ));
        let power = scene
            .hits
            .iter()
            .find(|h| h.id == "bluetooth-power")
            .unwrap();
        assert!(matches!(power.click, Some(Message::Ignore)));
        assert!(
            !power.tab_stop,
            "Busy ordinary controls must not capture keyboard focus"
        );
        assert!(client.ordinary_busy());
    }
}
#[test]
fn connection_admission_and_cancellation_wait_for_observed_termination() {
    let (mut client, adapter, mut effects) = client();
    client.update(Message::Connect(id(3)), &mut effects);
    let admission = effects.tasks.remove(0);
    client.update(admission.run(), &mut effects);
    assert!(
        client.ordinary_busy(),
        "Remain busy between admission and its snapshot"
    );
    client.update(Message::Observed(Ok(connecting_fixture())), &mut effects);
    client.update(
        Message::CancelConnection(BluetoothConnectionId::new(60).unwrap()),
        &mut effects,
    );
    let cancellation = effects.tasks.remove(0);
    assert_eq!(cancellation.queue, crate::work::CONTROLS);
    client.update(cancellation.run(), &mut effects);
    assert!(
        client.ordinary_busy(),
        "Cancellation acknowledgement is not terminal state"
    );
    assert_eq!(client.inspect()["connection"]["phase"], "connecting");
    let mut terminal = connecting_fixture();
    terminal.connection.as_mut().unwrap().phase = BluetoothConnectionPhase::Cancelled;
    client.update(Message::Observed(Ok(terminal)), &mut effects);
    assert!(!client.ordinary_busy());
    assert_eq!(
        *adapter.calls.lock().unwrap(),
        vec!["connect", "cancel-connection"]
    );
}
#[test]
fn cancellation_before_connection_admission_and_close_do_not_reopen_a_late_prompt() {
    for close in [false, true] {
        let (mut client, adapter, mut effects) = client();
        client.update(Message::Connect(id(3)), &mut effects);
        let admission = effects.tasks.remove(0);
        client.update(
            if close {
                Message::Close
            } else {
                Message::CancelConnectionAdmission
            },
            &mut effects,
        );
        let mut snapshot = connecting_fixture();
        snapshot.pairing_requests.push(BluetoothPairingRequest {
            id: pairing_id(41),
            device: id(3),
            prompt: BluetoothPairingPrompt::AuthorizePairing,
        });
        client.update(Message::Observed(Ok(snapshot)), &mut effects);
        assert_eq!(client.open(), !close);
        client.update(admission.run(), &mut effects);
        let cancellation = effects.tasks.remove(0);
        client.update(cancellation.run(), &mut effects);
        assert_eq!(
            *adapter.calls.lock().unwrap(),
            vec!["connect", "cancel-connection"]
        );
        assert_eq!(client.open(), !close);
    }
}

#[test]
fn late_connection_admission_cannot_keep_a_retired_observation_busy() {
    for mode in ["unavailable", "recovered", "removed"] {
        let (mut client, _, mut effects) = client();
        client.update(Message::Connect(id(3)), &mut effects);
        let admission = effects.tasks.remove(0);
        if mode == "removed" {
            let mut state = fixture();
            state.devices.retain(|d| d.id != id(3));
            client.update(Message::Observed(Ok(state)), &mut effects);
        } else {
            client.update(
                Message::Observed(Err(DomainError::Unavailable("Service restarted".into()))),
                &mut effects,
            );
            if mode == "recovered" {
                client.update(Message::Observed(Ok(fixture())), &mut effects);
            }
        }
        client.update(admission.run(), &mut effects);
        assert!(
            !client.ordinary_busy(),
            "Late admission must not stick after {mode}"
        );
        assert!(client.pending_connection.is_none());
        assert!(
            effects.tasks.is_empty(),
            "Never replay a retired connection"
        );
        assert!(client.inspect()["connection"].is_null());
    }
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
