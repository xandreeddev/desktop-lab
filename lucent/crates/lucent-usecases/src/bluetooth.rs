//! Policy for Bluetooth controls, independent of transport and desktop geometry.
use lucent_domain::*;

pub fn validate_pairing_target(state: &BluetoothSnapshot, id: BluetoothObjectId) -> Result<()> {
    let device = state
        .devices
        .iter()
        .find(|d| d.id == id)
        .ok_or_else(|| DomainError::Invalid("Choose a current Bluetooth device".into()))?;
    let controller = state
        .adapters
        .iter()
        .find(|a| a.id == device.adapter)
        .ok_or_else(|| DomainError::Invalid("Bluetooth controller disappeared".into()))?;
    if !controller.powered || device.blocked || device.paired {
        return Err(DomainError::Invalid(
            "Choose an unpaired, available Bluetooth device".into(),
        ));
    }
    Ok(())
}
pub fn validate_connection_target(state: &BluetoothSnapshot, id: BluetoothObjectId) -> Result<()> {
    let device = state
        .devices
        .iter()
        .find(|d| d.id == id)
        .ok_or_else(|| DomainError::Invalid("Choose a current Bluetooth device".into()))?;
    let controller = state
        .adapters
        .iter()
        .find(|a| a.id == device.adapter)
        .ok_or_else(|| DomainError::Invalid("Bluetooth controller disappeared".into()))?;
    if !controller.powered || device.blocked || !device.paired {
        return Err(DomainError::Invalid(
            "Choose a paired, available Bluetooth device".into(),
        ));
    }
    Ok(())
}
pub fn validate_pairing_answer(
    prompt: &BluetoothPairingPrompt,
    answer: &BluetoothPairingAnswer,
) -> Result<()> {
    let valid = matches!(answer, BluetoothPairingAnswer::Reject)
        || matches!(
            (prompt, answer),
            (
                BluetoothPairingPrompt::PinCode,
                BluetoothPairingAnswer::PinCode(_)
            ) | (
                BluetoothPairingPrompt::Passkey,
                BluetoothPairingAnswer::Passkey(_)
            ) | (
                BluetoothPairingPrompt::ConfirmPasskey(_)
                    | BluetoothPairingPrompt::AuthorizePairing
                    | BluetoothPairingPrompt::AuthorizeService(_),
                BluetoothPairingAnswer::Accept
            )
        );
    if valid {
        Ok(())
    } else {
        Err(DomainError::Invalid(
            "Answer does not match the current Bluetooth prompt".into(),
        ))
    }
}

/// Declining a reply-bearing prompt also cancels the matching attempt we own.
/// Incoming requests must not cancel an unrelated local attempt. Display-only
/// cards have already replied; rejecting them invokes cancellation through the port.
pub fn pairing_attempt_for_rejected_reply(
    state: &BluetoothSnapshot,
    request: BluetoothPairingId,
) -> Option<BluetoothPairingId> {
    let request = state.pairing_requests.iter().find(|p| p.id == request)?;
    if matches!(
        request.prompt,
        BluetoothPairingPrompt::DisplayPinCode(_) | BluetoothPairingPrompt::DisplayPasskey { .. }
    ) {
        return None;
    }
    state
        .pairing
        .as_ref()
        .filter(|a| a.device == request.device && a.phase == BluetoothPairingPhase::Pairing)
        .map(|a| a.id)
}

pub fn validate_bluetooth_command(
    state: &BluetoothSnapshot,
    command: &BluetoothCommand,
) -> Result<()> {
    let adapter = |id| {
        state
            .adapters
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| DomainError::Invalid("Choose a current Bluetooth controller".into()))
    };
    let device = |id| {
        state
            .devices
            .iter()
            .find(|d| d.id == id)
            .ok_or_else(|| DomainError::Invalid("Choose a current Bluetooth device".into()))
    };
    match *command {
        BluetoothCommand::SetPowered { adapter: id, .. } => {
            adapter(id)?;
        }
        BluetoothCommand::SetDiscoverable { adapter: id, .. } => {
            if !adapter(id)?.powered {
                return Err(DomainError::Invalid(
                    "Power on the Bluetooth controller first".into(),
                ));
            }
        }
        BluetoothCommand::SetPairable { adapter: id, .. } => {
            adapter(id)?;
        }
        BluetoothCommand::SetDiscovery {
            adapter: id,
            active,
        } => {
            if active && !adapter(id)?.powered {
                return Err(DomainError::Invalid(
                    "Power on the Bluetooth controller first".into(),
                ));
            }
            adapter(id)?;
        }
        BluetoothCommand::Disconnect(id) | BluetoothCommand::Forget(id) => {
            adapter(device(id)?.adapter)?;
        }
        BluetoothCommand::SetTrusted {
            device: id,
            trusted,
        } => {
            if trusted && !device(id)?.paired {
                return Err(DomainError::Invalid(
                    "Pair the Bluetooth device before trusting it".into(),
                ));
            }
            adapter(device(id)?.adapter)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> BluetoothSnapshot {
        let adapter = BluetoothObjectId::new(1).unwrap();
        BluetoothSnapshot {
            adapters: vec![BluetoothAdapter {
                id: adapter,
                address: BluetoothAddress::parse("02:00:00:00:00:01").unwrap(),
                name: "Controller".into(),
                powered: true,
                discoverable: false,
                pairable: true,
                discovering: true,
                discovery_owned: false,
            }],
            devices: vec![BluetoothDevice {
                id: BluetoothObjectId::new(2).unwrap(),
                adapter,
                address: BluetoothAddress::parse("02:00:00:00:00:02").unwrap(),
                name: "Headphones".into(),
                icon: None,
                paired: true,
                bonded: None,
                trusted: false,
                blocked: false,
                connected: false,
                services_resolved: false,
                rssi: None,
                battery_percent: None,
            }],
            ..BluetoothSnapshot::default()
        }
    }
    #[test]
    fn rejected_reply_cancels_only_its_live_owned_attempt() {
        let mut s = state();
        let attempt = BluetoothPairingId::new(10).unwrap();
        let request = BluetoothPairingId::new(11).unwrap();
        s.pairing = Some(BluetoothPairingAttempt {
            id: attempt,
            device: s.devices[0].id,
            phase: BluetoothPairingPhase::Pairing,
        });
        s.pairing_requests.push(BluetoothPairingRequest {
            id: request,
            device: s.devices[0].id,
            prompt: BluetoothPairingPrompt::ConfirmPasskey(BluetoothPairingCode::new(17).unwrap()),
        });
        assert_eq!(
            pairing_attempt_for_rejected_reply(&s, request),
            Some(attempt)
        );
        assert_eq!(pairing_attempt_for_rejected_reply(&s, attempt), None);
        s.pairing_requests[0].device = BluetoothObjectId::new(3).unwrap();
        assert_eq!(pairing_attempt_for_rejected_reply(&s, request), None);
        s.pairing_requests[0].device = s.devices[0].id;
        s.pairing.as_mut().unwrap().phase = BluetoothPairingPhase::Succeeded;
        assert_eq!(pairing_attempt_for_rejected_reply(&s, request), None);
        s.pairing.as_mut().unwrap().phase = BluetoothPairingPhase::Pairing;
        s.pairing_requests[0].prompt = BluetoothPairingPrompt::DisplayPasskey {
            code: BluetoothPairingCode::new(17).unwrap(),
            entered: 0,
        };
        assert_eq!(pairing_attempt_for_rejected_reply(&s, request), None);
        s.pairing = None;
        s.pairing_requests[0].prompt = BluetoothPairingPrompt::AuthorizePairing;
        assert_eq!(pairing_attempt_for_rejected_reply(&s, request), None);
    }
    #[test]
    fn radio_visibility_and_incoming_permission_have_distinct_policy() {
        let mut s = state();
        let id = s.adapters[0].id;
        for command in [
            BluetoothCommand::SetDiscoverable {
                adapter: id,
                discoverable: true,
            },
            BluetoothCommand::SetPairable {
                adapter: id,
                pairable: false,
            },
        ] {
            assert!(validate_bluetooth_command(&s, &command).is_ok());
        }
        s.adapters[0].pairable = false;
        s.devices[0].paired = false;
        assert!(
            validate_pairing_target(&s, s.devices[0].id).is_ok(),
            "Pairable limits incoming requests, not our explicit outgoing Pair"
        );
        s.adapters[0].powered = false;
        for discoverable in [true, false] {
            assert!(
                validate_bluetooth_command(
                    &s,
                    &BluetoothCommand::SetDiscoverable {
                        adapter: id,
                        discoverable
                    }
                )
                .is_err()
            );
        }
        assert!(
            validate_bluetooth_command(
                &s,
                &BluetoothCommand::SetPairable {
                    adapter: id,
                    pairable: false
                }
            )
            .is_ok()
        );
        for adapter in [s.devices[0].id, BluetoothObjectId::new(999).unwrap()] {
            assert!(
                validate_bluetooth_command(
                    &s,
                    &BluetoothCommand::SetPairable {
                        adapter,
                        pairable: true
                    }
                )
                .is_err()
            );
            assert!(
                validate_bluetooth_command(
                    &s,
                    &BluetoothCommand::SetDiscoverable {
                        adapter,
                        discoverable: false
                    }
                )
                .is_err()
            );
        }
    }
    #[test]
    fn controls_revalidate_identity_power_and_pairing_without_mutating_observations() {
        let mut s = state();
        let id = s.devices[0].id;
        assert!(validate_connection_target(&s, id).is_ok());
        assert!(validate_connection_target(&s, s.adapters[0].id).is_err());
        s.devices[0].paired = false;
        assert!(validate_connection_target(&s, id).is_err());
        assert!(
            validate_bluetooth_command(
                &s,
                &BluetoothCommand::SetTrusted {
                    device: id,
                    trusted: true
                }
            )
            .is_err()
        );
        assert!(
            validate_bluetooth_command(
                &s,
                &BluetoothCommand::SetTrusted {
                    device: id,
                    trusted: false
                }
            )
            .is_ok()
        );
        s.devices[0].paired = true;
        s.devices[0].blocked = true;
        assert!(validate_connection_target(&s, id).is_err());
        assert!(validate_bluetooth_command(&s, &BluetoothCommand::Disconnect(id)).is_ok());
        s.adapters[0].powered = false;
        assert!(
            validate_bluetooth_command(
                &s,
                &BluetoothCommand::SetDiscovery {
                    adapter: s.adapters[0].id,
                    active: true
                }
            )
            .is_err()
        );
        assert!(
            validate_bluetooth_command(
                &s,
                &BluetoothCommand::SetDiscovery {
                    adapter: s.adapters[0].id,
                    active: false
                }
            )
            .is_ok()
        );
    }
}
