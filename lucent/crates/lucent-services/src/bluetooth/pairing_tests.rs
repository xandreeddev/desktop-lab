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

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_connect_authorization_does_not_block_its_answer_or_auto_trust() {
    let bus = Bus::new();
    let state = Arc::new(Mutex::new(State {
        connect_authorization: true,
        ..State::default()
    }));
    let _server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(3));
    let first = watch.state(|s| s.devices.len() == 1);
    let adapter = watch.adapter.clone();
    let device = first.devices[0].id;
    let attempt = adapter.start_connection(device).unwrap();
    let request = prompt(&watch);
    assert!(
        matches!(request.prompt,BluetoothPairingPrompt::AuthorizeService(ref service) if service.as_str()=="0000110b-0000-1000-8000-00805f9b34fb")
    );
    assert_eq!(state.lock().unwrap().defaults, 0);
    assert!(
        watch
            .adapter
            .control(BluetoothCommand::SetTrusted {
                device,
                trusted: true
            })
            .is_err(),
        "Another write fails busy without running behind the pending method"
    );
    watch
        .adapter
        .answer_pairing(request.id, BluetoothPairingAnswer::Accept)
        .unwrap();
    let connected = watch.state(|s| {
        s.devices[0].connected
            && s.pairing_requests.is_empty()
            && s.connection
                .as_ref()
                .is_some_and(|a| a.id == attempt && a.phase == BluetoothConnectionPhase::Succeeded)
    });
    assert!(connected.pairing.is_none());
    assert!(!connected.devices[0].trusted);
    assert!(
        watch
            .adapter
            .answer_pairing(request.id, BluetoothPairingAnswer::Accept)
            .is_err()
    );
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_connection_cancellation_terminates_a_pending_callback_and_retires_its_id() {
    let bus = Bus::new();
    let state = Arc::new(Mutex::new(State {
        connect_authorization: true,
        ..State::default()
    }));
    let _server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(3));
    let first = watch.state(|s| s.devices.len() == 1);
    let device = first.devices[0].id;
    let started = Instant::now();
    let attempt = watch.adapter.start_connection(device).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "Admission must not await authorization"
    );
    let pending = watch.state(|s| s.connection.is_some() && !s.pairing_requests.is_empty());
    let request = pending.pairing_requests[0].clone();
    assert_eq!(
        pending.connection.unwrap().phase,
        BluetoothConnectionPhase::Connecting
    );
    let wrong = BluetoothConnectionId::new(attempt.value() + 1000).unwrap();
    assert!(watch.adapter.cancel_connection(wrong).is_err());
    assert_eq!(state.lock().unwrap().disconnects, 0);
    watch.adapter.cancel_connection(attempt).unwrap();
    let terminal = watch.state(|s| {
        s.connection
            .as_ref()
            .is_some_and(|a| a.id == attempt && a.phase == BluetoothConnectionPhase::Cancelled)
            && s.pairing_requests.is_empty()
    });
    assert!(
        !terminal.devices[0].connected
            && terminal.devices[0].paired
            && !terminal.devices[0].trusted
    );
    assert_eq!(state.lock().unwrap().disconnects, 1);
    assert!(
        watch
            .adapter
            .answer_pairing(request.id, BluetoothPairingAnswer::Accept)
            .is_err()
    );
    assert!(watch.adapter.cancel_connection(attempt).is_err());
    let replacement = watch.adapter.start_connection(device).unwrap();
    prompt(&watch);
    assert_ne!(replacement, attempt);
    assert!(
        watch.adapter.cancel_connection(attempt).is_err(),
        "A retired ID cannot cancel a replacement"
    );
    assert_eq!(state.lock().unwrap().disconnects, 1);
    watch.adapter.cancel_connection(replacement).unwrap();
    watch.state(|s| {
        s.connection
            .as_ref()
            .is_some_and(|a| a.id == replacement && a.phase == BluetoothConnectionPhase::Cancelled)
    });
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_pending_connection_target_removal_and_timeout_close_its_owner() {
    for removed in [true, false] {
        let bus = Bus::new();
        let state = Arc::new(Mutex::new(State {
            connect_authorization: true,
            ..State::default()
        }));
        let server = futures_lite::future::block_on(server(&bus, &state));
        let watch = Observer::new(&bus, Duration::from_millis(500));
        let first = watch.state(|s| s.devices.len() == 1);
        let device = first.devices[0].id;
        let attempt = watch.adapter.start_connection(device).unwrap();
        let request = prompt(&watch);
        if removed {
            futures_lite::future::block_on(server.object_server().remove::<Remote, _>(DEV))
                .unwrap();
            watch.state(|s| s.devices.is_empty());
        } else {
            watch.state(|s| s.devices.first().is_some_and(|d| d.id != device));
        }
        assert!(watch.adapter.cancel_connection(attempt).is_err());
        assert!(
            watch
                .adapter
                .answer_pairing(request.id, BluetoothPairingAnswer::Accept)
                .is_err()
        );
        assert_eq!(
            state.lock().unwrap().disconnects,
            0,
            "Do not disconnect a retired or reused path"
        );
    }
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_release_clears_default_authority_and_allows_fresh_registration() {
    let bus = Bus::new();
    let state = unpaired(PairingMode::Authorization);
    let server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(3));
    let initial = watch.state(|s| s.devices.len() == 1);
    watch.adapter.set_incoming_pairing(true).unwrap();
    watch.state(|s| s.incoming_pairing_enabled);
    futures_lite::future::block_on(agent_call(&server, &state, "Release", &())).unwrap();
    let released = watch.state(|s| !s.incoming_pairing_enabled);
    assert_eq!(released.devices[0].id, initial.devices[0].id);
    watch.adapter.set_incoming_pairing(true).unwrap();
    watch.state(|s| s.incoming_pairing_enabled);
    assert_eq!(state.lock().unwrap().registrations, 2);
    assert_eq!(state.lock().unwrap().defaults, 2);
    let mut device = initial.devices[0].id;
    // Repeated real method replies exercise release versus pending Pair
    // completion. No sleeps or accepted NoReply errors hide the ordering race.
    for _ in 0..3 {
        let attempt = watch.adapter.start_pairing(device).unwrap();
        let request = prompt(&watch);
        futures_lite::future::block_on(agent_call(&server, &state, "Release", &())).unwrap();
        let reconnected = watch.state(|s| s.devices.len() == 1 && s.devices[0].id != device);
        assert!(
            watch
                .adapter
                .answer_pairing(request.id, BluetoothPairingAnswer::Accept)
                .is_err()
        );
        assert!(watch.adapter.cancel_pairing(attempt).is_err());
        device = reconnected.devices[0].id;
    }
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_incoming_display_decline_disconnects_instead_of_faking_a_reply() {
    let bus = Bus::new();
    let state = unpaired(PairingMode::Display);
    let server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(3));
    watch.state(|s| s.devices.len() == 1);
    watch.adapter.set_incoming_pairing(true).unwrap();
    watch.state(|s| s.incoming_pairing_enabled);
    futures_lite::future::block_on(agent_call(
        &server,
        &state,
        "DisplayPinCode",
        &(path(DEV), "000017"),
    ))
    .unwrap();
    let request = prompt(&watch);
    watch
        .adapter
        .answer_pairing(request.id, BluetoothPairingAnswer::Reject)
        .unwrap();
    let final_state = watch.state(|s| s.pairing_requests.is_empty());
    assert!(final_state.pairing.is_none());
    assert_eq!(state.lock().unwrap().disconnects, 1);
    assert!(!state.lock().unwrap().paired);
}

#[test]
#[ignore = "isolated BlueZ D-Bus protocol"]
fn bluetooth_protocol_cancellation_is_not_queued_behind_an_unrelated_native_call() {
    let bus = Bus::new();
    let state = unpaired(PairingMode::Confirmation);
    let _server = futures_lite::future::block_on(server(&bus, &state));
    let watch = Observer::new(&bus, Duration::from_secs(3));
    let initial = watch.state(|s| s.devices.len() == 1);
    let attempt = watch.adapter.start_pairing(initial.devices[0].id).unwrap();
    prompt(&watch);
    state.lock().unwrap().delay = Duration::from_millis(900);
    let adapter = watch.adapter.clone();
    let controller = initial.adapters[0].id;
    let (send, receive) = mpsc::sync_channel(1);
    let scan = thread::spawn(move || {
        send.send(adapter.control(BluetoothCommand::SetDiscovery {
            adapter: controller,
            active: true,
        }))
        .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(1);
    while state.lock().unwrap().start_requests == 0 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    watch.adapter.cancel_pairing(attempt).unwrap();
    assert!(
        matches!(receive.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "Cancellation must complete while the ordinary method is still pending"
    );
    let cancelled = result(&watch, attempt);
    assert_eq!(
        cancelled.pairing.unwrap().phase,
        BluetoothPairingPhase::Cancelled
    );
    receive
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    scan.join().unwrap();
    watch.state(|s| s.adapters[0].discovery_owned);
}
