use super::*;
use std::sync::Mutex;

pub fn id(value: u64) -> NetworkObjectId {
    NetworkObjectId::new(value).unwrap()
}
pub fn fixture() -> NetworkSnapshot {
    NetworkSnapshot {
        state: NetworkState::Global,
        connectivity: NetworkConnectivity::Full,
        wifi_enabled: true,
        wifi_hardware_enabled: true,
        primary_connection: Some(id(4)),
        devices: vec![NetworkDevice {
            id: id(1),
            interface: "wifi-fixture".into(),
            kind: NetworkKind::Wifi,
            managed: true,
            state: NetworkDeviceState::Connected,
            failure: NetworkFailure::None,
            active_connection: Some(id(4)),
            active_access_point: Some(id(2)),
            scan_generation: Some(12),
        }],
        access_points: vec![NetworkAccessPoint {
            id: id(2),
            device: id(1),
            ssid: Some(NetworkSsid::new(b"Lucent fixture".to_vec()).unwrap()),
            strength: NetworkStrength::new(80).unwrap(),
            security: WifiSecurity::WpaPersonal,
        }],
        profiles: vec![NetworkProfile {
            id: id(3),
            key: NetworkProfileKey::new("fixture-profile").unwrap(),
            name: "Lucent fixture".into(),
            kind: NetworkKind::Wifi,
            ssid: Some(NetworkSsid::new(b"Lucent fixture".to_vec()).unwrap()),
        }],
        connections: vec![NetworkConnection {
            id: id(4),
            profile: Some(id(3)),
            name: "Lucent fixture".into(),
            kind: NetworkKind::Wifi,
            devices: vec![id(1)],
            state: NetworkConnectionState::Connected,
        }],
    }
}
#[derive(Default)]
struct Adapter {
    commands: Mutex<Vec<NetworkCommand>>,
}
impl NetworkPort for Adapter {
    fn watch(&self, emit: &mut dyn FnMut(Result<NetworkSnapshot>), _: &dyn StopSignal) {
        emit(Ok(fixture()));
    }
    fn control(&self, command: NetworkCommand) -> Result<()> {
        self.commands.lock().unwrap().push(command);
        Ok(())
    }
}
fn client() -> (Network, Arc<Adapter>, Effects<Message>) {
    let adapter = Arc::new(Adapter::default());
    let mut client = Network::new(adapter.clone());
    let mut effects = Effects::default();
    client.update(Message::Observed(Ok(fixture())), &mut effects);
    client.update(Message::Open, &mut effects);
    (client, adapter, effects)
}
fn complete(client: &mut Network, effects: &mut Effects<Message>) {
    let task = effects.tasks.remove(0);
    assert_eq!(task.queue, "network");
    client.update(task.run(), effects);
}
#[test]
fn acknowledgements_do_not_change_observed_state_or_complete_scans() {
    let (mut client, _, mut effects) = client();
    client.update(Message::Request(Control::Wifi(false)), &mut effects);
    complete(&mut client, &mut effects);
    assert!(client.snapshot().unwrap().wifi_enabled);
    client.update(Message::Request(Control::Scan(id(1))), &mut effects);
    complete(&mut client, &mut effects);
    assert!(client.scan.is_some());
    client.update(Message::Observed(Ok(fixture())), &mut effects);
    assert!(client.scan.is_some());
    let mut changed = fixture();
    changed.devices[0].scan_generation = Some(13);
    client.update(Message::Observed(Ok(changed)), &mut effects);
    assert!(client.scan.is_none());
}
#[test]
fn credentials_are_consumed_once_and_cannot_be_retried_or_inspected() {
    let (mut client, adapter, mut effects) = client();
    client.update(Message::Join(id(2)), &mut effects);
    let generation = client.form.as_ref().unwrap().generation;
    client.update(
        Message::Password(generation, InputEdit::Insert("fixture-secret".into())),
        &mut effects,
    );
    assert_eq!(client.form.as_ref().unwrap().password_length(), 14);
    assert!(!client.inspect().to_string().contains("fixture-secret"));
    client.update(Message::Submit(generation), &mut effects);
    assert!(client.form.is_none());
    complete(&mut client, &mut effects);
    let commands = adapter.commands.lock().unwrap();
    let NetworkCommand::Join {
        target,
        credentials,
        remember,
    } = &commands[0]
    else {
        panic!("expected join")
    };
    assert_eq!(*target, NetworkJoinTarget::AccessPoint(id(2)));
    assert!(*remember);
    assert_eq!(credentials.password().unwrap().expose(), "fixture-secret");
    drop(commands);
    client.update(Message::Submit(generation), &mut effects);
    assert!(effects.tasks.is_empty());
    client.update(Message::Retry, &mut effects);
    assert!(effects.tasks.is_empty());
    assert!(client.failure.as_ref().unwrap().control.is_none());
}
#[test]
fn forms_retire_on_stale_handles_service_loss_and_close() {
    let (mut client, _, mut effects) = client();
    client.update(Message::Join(id(2)), &mut effects);
    let old = client.form.as_ref().unwrap().generation;
    client.update(Message::Cancel, &mut effects);
    client.update(Message::Join(id(2)), &mut effects);
    client.update(
        Message::Password(old, InputEdit::Insert("stale-value".into())),
        &mut effects,
    );
    assert_eq!(client.form.as_ref().unwrap().password_length(), 0);
    let mut changed = fixture();
    changed.access_points[0].ssid = Some(NetworkSsid::new(b"Different network".to_vec()).unwrap());
    client.update(Message::Observed(Ok(changed)), &mut effects);
    assert!(client.form.is_none());
    client.update(Message::Hidden, &mut effects);
    assert!(client.form.is_some());
    client.update(
        Message::Observed(Err(DomainError::Unavailable("service restarted".into()))),
        &mut effects,
    );
    assert!(client.form.is_none());
    assert!(client.snapshot().is_none());
    client.update(Message::Observed(Ok(fixture())), &mut effects);
    client.update(Message::Join(id(2)), &mut effects);
    client.update(Message::Close, &mut effects);
    assert!(client.form.is_none());
    client.update(Message::Join(id(2)), &mut effects);
    assert!(client.form.is_none());
}
#[test]
fn stale_controls_are_not_retargeted_and_newer_errors_survive_old_completions() {
    let (mut client, _, mut effects) = client();
    client.update(Message::Request(Control::Scan(id(1))), &mut effects);
    client.update(Message::Request(Control::Disconnect(id(99))), &mut effects);
    let error = client.failure.as_ref().unwrap().reason.clone();
    complete(&mut client, &mut effects);
    assert_eq!(client.failure.as_ref().unwrap().reason, error);
    client.update(Message::Retry, &mut effects);
    assert!(effects.tasks.is_empty());
    client.update(Message::Request(Control::Disconnect(id(4))), &mut effects);
    assert!(client.failure.is_none());
    assert_eq!(effects.tasks.len(), 1);
}
#[test]
fn hidden_forms_use_raw_name_supported_security_and_explicit_remember_choice() {
    let (mut client, adapter, mut effects) = client();
    client.update(Message::Hidden, &mut effects);
    let generation = client.form.as_ref().unwrap().generation;
    client.update(
        Message::Name(
            generation,
            lucent_api::TextEdit::Replace("Hidden fixture".into()),
        ),
        &mut effects,
    );
    client.update(
        Message::Password(generation, InputEdit::Insert("discarded-secret".into())),
        &mut effects,
    );
    client.update(
        Message::Security(generation, WifiSecurity::Open),
        &mut effects,
    );
    assert_eq!(client.form.as_ref().unwrap().password_length(), 0);
    client.update(Message::Remember(generation), &mut effects);
    client.update(Message::Submit(generation), &mut effects);
    complete(&mut client, &mut effects);
    let commands = adapter.commands.lock().unwrap();
    let NetworkCommand::Join {
        target,
        credentials,
        remember,
    } = &commands[0]
    else {
        panic!("expected join")
    };
    assert_eq!(
        *target,
        NetworkJoinTarget::Hidden {
            device: id(1),
            ssid: NetworkSsid::new(b"Hidden fixture".to_vec()).unwrap()
        }
    );
    assert!(!remember);
    assert!(credentials.password().is_none());
}
#[test]
fn forgetting_needs_confirmation_and_a_current_profile() {
    let (mut client, adapter, mut effects) = client();
    client.update(Message::AskForget(id(3)), &mut effects);
    assert!(effects.tasks.is_empty());
    client.update(Message::Cancel, &mut effects);
    client.update(Message::ConfirmForget, &mut effects);
    assert!(effects.tasks.is_empty());
    client.update(Message::AskForget(id(3)), &mut effects);
    client.update(Message::ConfirmForget, &mut effects);
    complete(&mut client, &mut effects);
    assert!(
        matches!(adapter.commands.lock().unwrap()[0], NetworkCommand::Forget(profile) if profile==id(3))
    );
    client.update(Message::AskForget(id(3)), &mut effects);
    let mut changed = fixture();
    changed.profiles.clear();
    client.update(Message::Observed(Ok(changed)), &mut effects);
    client.update(Message::ConfirmForget, &mut effects);
    assert!(effects.tasks.is_empty());
}
#[test]
fn reopening_retains_motion_continuity_and_rejects_stale_close_timer() {
    let (mut client, _, mut effects) = client();
    effects.now = 0.1;
    client.update(Message::Close, &mut effects);
    let old = client.close_generation;
    effects.now = 0.14;
    let position = client.motion.value(effects.now);
    client.update(Message::Open, &mut effects);
    assert_eq!(client.motion.value(effects.now), position);
    client.update(Message::Closed(old), &mut effects);
    assert!(client.visible());
    assert!(client.open());
    client.update(Message::Close, &mut effects);
    client.update(Message::Closed(old), &mut effects);
    assert!(client.visible());
    client.update(Message::Closed(client.close_generation), &mut effects);
    assert!(!client.visible());
}

#[test]
fn hidden_network_text_is_current_bounded_and_clipboard_tickets_do_not_cross_forms() {
    use lucent_api::{ClipboardOutcome, ClipboardTarget, TextEdit};
    let (mut client, adapter, mut effects) = client();
    client.update(Message::Hidden, &mut effects);
    let generation = client.form.as_ref().unwrap().generation;
    for text in ["Fixture ", "café"] {
        client.update(
            Message::Name(generation, TextEdit::Insert(text.into())),
            &mut effects,
        );
    }
    assert_eq!(client.form.as_ref().unwrap().name, "Fixture café");
    assert!(adapter.commands.lock().unwrap().is_empty());
    client.update(
        Message::Password(
            generation,
            InputEdit::Insert("private-form-password".into()),
        ),
        &mut effects,
    );
    let secret_count = client.form.as_ref().unwrap().password_length();
    client.update(Message::Name(generation, TextEdit::SelectAll), &mut effects);
    client.update(Message::Name(generation, TextEdit::Paste), &mut effects);
    let request = effects.clipboard.pop().unwrap();
    assert_eq!(
        request.target,
        ClipboardTarget::new("network", format!("network-name-{generation}"))
    );
    client.update(
        request.finish(ClipboardOutcome::Text("x".repeat(33))),
        &mut effects,
    );
    assert_eq!(client.form.as_ref().unwrap().name, "Fixture café");
    assert_eq!(
        client.form.as_ref().unwrap().password_length(),
        secret_count
    );
    assert!(client.failure.is_some());
    client.update(Message::Name(generation, TextEdit::Paste), &mut effects);
    let request = effects.clipboard.pop().unwrap();
    client.update(
        request.finish(ClipboardOutcome::Text("new network".into())),
        &mut effects,
    );
    assert_eq!(client.form.as_ref().unwrap().name, "new network");
    client.update(Message::Name(generation, TextEdit::Paste), &mut effects);
    let old = effects.clipboard.pop().unwrap();
    client.update(Message::Cancel, &mut effects);
    client.update(Message::Hidden, &mut effects);
    let current = client.form.as_ref().unwrap().generation;
    assert_ne!(generation, current);
    client.update(
        old.finish(ClipboardOutcome::Text("old form".into())),
        &mut effects,
    );
    assert_eq!(client.form.as_ref().unwrap().name, "");
    assert_eq!(client.form.as_ref().unwrap().password_length(), 0);
}
