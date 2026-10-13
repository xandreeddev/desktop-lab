//! Real D-Bus callbacks on an isolated bus, including calls pending user replies.
use super::*;

fn unpaired(mode: PairingMode) -> Shared {
    Arc::new(Mutex::new(State {
        paired: false,
        pairing_mode: mode,
        ..State::default()
    }))
}
fn prompt(watch: &Observer) -> BluetoothPairingRequest {
    watch
        .state(|s| !s.pairing_requests.is_empty())
        .pairing_requests[0]
        .clone()
}
fn result(watch: &Observer, id: BluetoothPairingId) -> BluetoothSnapshot {
    watch.state(|s| {
        s.pairing
            .as_ref()
            .is_some_and(|a| a.id == id && a.phase != BluetoothPairingPhase::Pairing)
    })
}
async fn agent_call<B: serde::Serialize + zbus::zvariant::DynamicType>(
    connection: &zbus::Connection,
    state: &Shared,
    member: &str,
    body: &B,
) -> zbus::Result<zbus::Message> {
    let (destination, agent_path) = state.lock().unwrap().agent.clone().unwrap();
    connection
        .call_method(
            Some(destination.as_str()),
            agent_path.as_str(),
            Some("org.bluez.Agent1"),
            member,
            body,
        )
        .await
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol; no host radio or bus"]
fn bluetooth_protocol_pairing_requires_a_current_explicit_decision() {
    let bus = Bus::new();
    let state = unpaired(PairingMode::Confirmation);
    let _server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(3));
    let initial = watch.state(|s| s.devices.len() == 1);
    assert_eq!(
        state.lock().unwrap().registrations,
        0,
        "Observation is read-only"
    );
    let start = Instant::now();
    let attempt = watch.adapter.start_pairing(initial.devices[0].id).unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "Admission must not await a user answer"
    );
    let request = prompt(&watch);
    assert!(
        matches!(request.prompt, BluetoothPairingPrompt::ConfirmPasskey(code) if code.digits() == "000017")
    );
    assert!(!state.lock().unwrap().paired);
    assert_eq!(
        state.lock().unwrap().defaults,
        0,
        "Outgoing pairing must not seize the default agent"
    );
    assert!(
        watch
            .adapter
            .answer_pairing(
                request.id,
                BluetoothPairingAnswer::Passkey(
                    BluetoothPasskeyInput::new(Secret::new("17".into())).unwrap(),
                )
            )
            .is_err()
    );
    assert!(!state.lock().unwrap().paired);
    watch
        .adapter
        .answer_pairing(request.id, BluetoothPairingAnswer::Accept)
        .unwrap();
    let completed = result(&watch, attempt);
    assert_eq!(
        completed.pairing.unwrap().phase,
        BluetoothPairingPhase::Succeeded
    );
    assert!(completed.devices[0].paired);
    assert!(
        !completed.devices[0].trusted,
        "Pairing must never implicitly trust"
    );
    assert!(completed.pairing_requests.is_empty());
    assert!(
        watch
            .adapter
            .answer_pairing(request.id, BluetoothPairingAnswer::Accept)
            .is_err()
    );
    assert!(
        watch.adapter.cancel_pairing(attempt).is_err(),
        "Cancel must not undo a completed bond"
    );
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_pairing_owned_input_and_rejection() {
    for mode in [
        PairingMode::Pin,
        PairingMode::Passkey,
        PairingMode::Authorization,
    ] {
        let bus = Bus::new();
        let state = unpaired(mode);
        let _server = futures_lite::future::block_on(server(&bus, &state));
        let watch = Observer::new(&bus, Duration::from_secs(3));
        let first = watch.state(|s| s.devices.len() == 1);
        let attempt = watch.adapter.start_pairing(first.devices[0].id).unwrap();
        let request = prompt(&watch);
        let answer = match mode {
            PairingMode::Pin => BluetoothPairingAnswer::PinCode(
                BluetoothPin::new(Secret::new("fixturePin1".into())).unwrap(),
            ),
            PairingMode::Passkey => BluetoothPairingAnswer::Passkey(
                BluetoothPasskeyInput::new(Secret::new("000017".into())).unwrap(),
            ),
            _ => BluetoothPairingAnswer::Reject,
        };
        watch.adapter.answer_pairing(request.id, answer).unwrap();
        let completed = result(&watch, attempt);
        let s = state.lock().unwrap();
        match mode {
            PairingMode::Pin => {
                assert_eq!(s.received_pin.as_deref(), Some("fixturePin1"));
                assert_eq!(
                    completed.pairing.unwrap().phase,
                    BluetoothPairingPhase::Succeeded
                );
            }
            PairingMode::Passkey => {
                assert_eq!(s.received_passkey, Some(17));
                assert_eq!(
                    completed.pairing.unwrap().phase,
                    BluetoothPairingPhase::Succeeded
                );
            }
            _ => {
                assert!(!s.paired);
                assert!(matches!(
                    completed.pairing.unwrap().phase,
                    BluetoothPairingPhase::Failed(_)
                ));
            }
        }
    }
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_display_progress_retains_identity_and_reject_cancels_natively() {
    let bus = Bus::new();
    let state = unpaired(PairingMode::Display);
    let _server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(3));
    let first = watch.state(|s| s.devices.len() == 1);
    let attempt = watch.adapter.start_pairing(first.devices[0].id).unwrap();
    let request = prompt(&watch);
    assert!(
        matches!(request.prompt, BluetoothPairingPrompt::DisplayPasskey { code, entered: 0 } if code.digits() == "000017")
    );
    let progressed = watch.state(|s| {
        s.pairing_requests.iter().any(|p| {
            matches!(
                p.prompt,
                BluetoothPairingPrompt::DisplayPasskey { entered: 3, .. }
            )
        })
    });
    assert_eq!(progressed.pairing_requests[0].id, request.id);
    assert!(
        watch
            .adapter
            .answer_pairing(request.id, BluetoothPairingAnswer::Accept)
            .is_err(),
        "Display is not a confirmation"
    );
    watch
        .adapter
        .answer_pairing(request.id, BluetoothPairingAnswer::Reject)
        .unwrap();
    let completed = result(&watch, attempt);
    assert_eq!(
        completed.pairing.unwrap().phase,
        BluetoothPairingPhase::Cancelled
    );
    assert!(completed.pairing_requests.is_empty());
    assert!(state.lock().unwrap().pairing_cancelled);
    assert!(!completed.devices[0].paired);
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_agent_peer_scope_and_default_lifecycle() {
    let bus = Bus::new();
    let state = unpaired(PairingMode::Confirmation);
    let server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(3));
    let first = watch.state(|s| s.devices.len() == 1);
    watch.adapter.set_incoming_pairing(true).unwrap();
    watch.state(|s| s.incoming_pairing_enabled);
    let stranger = futures_lite::future::block_on(
        zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .build(),
    )
    .unwrap();
    let denied = futures_lite::future::block_on(agent_call(
        &stranger,
        &state,
        "RequestConfirmation",
        &(path(DEV), 17_u32),
    ));
    assert!(
        matches!(denied, Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == "org.bluez.Error.Rejected")
    );
    let unknown = futures_lite::future::block_on(agent_call(
        &server,
        &state,
        "RequestAuthorization",
        &(path("/org/bluez/hci0/dev_unknown"),),
    ));
    assert!(
        unknown.is_err(),
        "Even the right sender must identify a current device"
    );
    watch.adapter.set_incoming_pairing(false).unwrap();
    watch.state(|s| !s.incoming_pairing_enabled);
    assert_eq!(state.lock().unwrap().defaults, 1);
    assert_eq!(state.lock().unwrap().unregistrations, 1);
    let attempt = watch.adapter.start_pairing(first.devices[0].id).unwrap();
    let request = prompt(&watch);
    watch.adapter.cancel_pairing(attempt).unwrap();
    let completed = result(&watch, attempt);
    assert_eq!(
        completed.pairing.unwrap().phase,
        BluetoothPairingPhase::Cancelled
    );
    assert!(
        watch
            .adapter
            .answer_pairing(request.id, BluetoothPairingAnswer::Accept)
            .is_err()
    );
    assert_eq!(
        state.lock().unwrap().registrations,
        2,
        "Unregister must permit a fresh application agent"
    );
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_pending_pairing_removal_and_timeout_close_the_owner() {
    for remove in [true, false] {
        let bus = Bus::new();
        let state = unpaired(PairingMode::Confirmation);
        let server = futures_lite::future::block_on(server(&bus, &state));
        let watch = Observer::new(&bus, Duration::from_millis(500));
        let first = watch.state(|s| s.devices.len() == 1);
        let attempt = watch.adapter.start_pairing(first.devices[0].id).unwrap();
        let request = prompt(&watch);
        let (owner, _) = state.lock().unwrap().agent.clone().unwrap();
        if remove {
            futures_lite::future::block_on(server.object_server().remove::<Remote, _>(DEV))
                .unwrap();
            watch.state(|s| s.devices.is_empty());
        } else {
            watch.state(|s| s.devices[0].id != first.devices[0].id);
        }
        assert!(
            watch
                .adapter
                .answer_pairing(request.id, BluetoothPairingAnswer::Accept)
                .is_err()
        );
        assert!(watch.adapter.cancel_pairing(attempt).is_err());
        let owned: bool = futures_lite::future::block_on(async {
            server
                .call_method(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    Some("org.freedesktop.DBus"),
                    "NameHasOwner",
                    &(owner,),
                )
                .await
                .unwrap()
                .body()
                .deserialize()
                .unwrap()
        });
        assert!(
            !owned,
            "Uncertain pairing must retire the connection, not retain its agent"
        );
        assert!(!state.lock().unwrap().paired);
    }
}
