//! Policy for Bluetooth controls, independent of transport and desktop geometry.
use lucent_domain::*;

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
        BluetoothCommand::Connect(id) => {
            let d = device(id)?;
            if !adapter(d.adapter)?.powered || d.blocked {
                return Err(DomainError::Invalid(
                    "Bluetooth device is unavailable or blocked".into(),
                ));
            }
            // Connecting an unpaired device could invoke another application's
            // default pairing agent. Keep that separate from explicit native pairing.
            if !d.paired {
                return Err(DomainError::Invalid(
                    "Pair the Bluetooth device before connecting".into(),
                ));
            }
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
        }
    }
    #[test]
    fn controls_revalidate_identity_power_and_pairing_without_mutating_observations() {
        let mut s = state();
        let id = s.devices[0].id;
        assert!(validate_bluetooth_command(&s, &BluetoothCommand::Connect(id)).is_ok());
        assert!(
            validate_bluetooth_command(&s, &BluetoothCommand::Connect(s.adapters[0].id)).is_err()
        );
        s.devices[0].paired = false;
        assert!(validate_bluetooth_command(&s, &BluetoothCommand::Connect(id)).is_err());
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
        assert!(validate_bluetooth_command(&s, &BluetoothCommand::Connect(id)).is_err());
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
