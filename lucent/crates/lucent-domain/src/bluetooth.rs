//! Bluetooth observations and intent, without D-Bus paths or presentation types.
use crate::{DomainError, Result, StopSignal};
use std::fmt;

/// Adapter-local transient handle. Removal, replacement and service reconnect
/// retire it. An address is a separate value and cannot recreate this handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BluetoothObjectId(u64);
impl BluetoothObjectId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid("Bluetooth identity is empty".into()));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BluetoothAddress([u8; 6]);
impl BluetoothAddress {
    pub fn parse(text: &str) -> Result<Self> {
        let invalid = || DomainError::Invalid("Invalid Bluetooth address".into());
        if text.len() != 17 {
            return Err(invalid());
        }
        let mut bytes = [0; 6];
        for (index, part) in text.split(':').enumerate() {
            if index >= 6 || part.len() != 2 || !part.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(invalid());
            }
            bytes[index] = u8::from_str_radix(part, 16).map_err(|_| invalid())?;
        }
        Ok(Self(bytes))
    }
}
impl fmt::Display for BluetoothAddress {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.0.iter().enumerate() {
            if index > 0 {
                out.write_str(":")?;
            }
            write!(out, "{byte:02X}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothAdapter {
    pub id: BluetoothObjectId,
    pub address: BluetoothAddress,
    pub name: String,
    pub powered: bool,
    pub discoverable: bool,
    /// Global permission for incoming pairing, separate from agent authority.
    pub pairable: bool,
    /// Global controller observation, which may belong to another client.
    pub discovering: bool,
    /// Whether this adapter instance acquired a discovery session.
    pub discovery_owned: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothDevice {
    pub id: BluetoothObjectId,
    pub adapter: BluetoothObjectId,
    pub address: BluetoothAddress,
    pub name: String,
    pub icon: Option<String>,
    pub paired: bool,
    pub bonded: Option<bool>,
    pub trusted: bool,
    pub blocked: bool,
    pub connected: bool,
    pub services_resolved: bool,
    /// Native dBm, without inventing a signal percentage.
    pub rssi: Option<i16>,
    pub battery_percent: Option<u8>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BluetoothSnapshot {
    pub adapters: Vec<BluetoothAdapter>,
    pub devices: Vec<BluetoothDevice>,
    pub pairing: Option<crate::BluetoothPairingAttempt>,
    pub pairing_requests: Vec<crate::BluetoothPairingRequest>,
    pub incoming_pairing_enabled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BluetoothCommand {
    SetPowered {
        adapter: BluetoothObjectId,
        powered: bool,
    },
    SetDiscovery {
        adapter: BluetoothObjectId,
        active: bool,
    },
    SetDiscoverable {
        adapter: BluetoothObjectId,
        discoverable: bool,
    },
    SetPairable {
        adapter: BluetoothObjectId,
        pairable: bool,
    },
    Connect(BluetoothObjectId),
    Disconnect(BluetoothObjectId),
    SetTrusted {
        device: BluetoothObjectId,
        trusted: bool,
    },
    Forget(BluetoothObjectId),
}
pub trait BluetoothPort: Send + Sync {
    /// Ready with no adapters differs from unavailable/failed observation.
    /// The subscriber owns the adapter's transport lifetime.
    fn watch(&self, emit: &mut dyn FnMut(Result<BluetoothSnapshot>), stop: &dyn StopSignal);
    /// Acknowledges a backend request, not completed discovery or all profiles.
    /// Observe the resulting snapshot; never blindly replay an uncertain command.
    fn control(&self, command: BluetoothCommand) -> Result<()>;
    /// Starts an asynchronous attempt. The ID acknowledges local admission,
    /// not successful pairing; watch the attempt and actual device observations.
    fn start_pairing(&self, device: BluetoothObjectId) -> Result<crate::BluetoothPairingId>;
    /// Delivers one owned answer to a current prompt, not pairing completion.
    /// Rejecting a display-only card requests cancellation of its native
    /// operation; a reply-bearing rejection can still require cancel_pairing.
    fn answer_pairing(
        &self,
        request: crate::BluetoothPairingId,
        answer: crate::BluetoothPairingAnswer,
    ) -> Result<()>;
    /// Requests cancellation of our current attempt; it does not undo a bond.
    fn cancel_pairing(&self, attempt: crate::BluetoothPairingId) -> Result<()>;
    /// Default-agent registration is explicit, separate from controller visibility.
    fn set_incoming_pairing(&self, enabled: bool) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn addresses_are_octets_and_handles_cannot_be_rebuilt_from_addresses() {
        let a = BluetoothAddress::parse("aa:01:b2:03:04:fe").unwrap();
        assert_eq!(a.to_string(), "AA:01:B2:03:04:FE");
        assert_eq!(a, BluetoothAddress::parse("AA:01:B2:03:04:FE").unwrap());
        for text in [
            "",
            "AA:BB:CC:DD:EE",
            "AA:BB:CC:DD:EE:FF:00",
            "A:01:B2:03:04:FE",
            "GG:01:B2:03:04:FE",
            "AA-01-B2-03-04-FE",
            " AA:01:B2:03:04:FE",
            "é:01:B2:03:04:FE",
        ] {
            assert!(BluetoothAddress::parse(text).is_err());
        }
        assert!(BluetoothObjectId::new(0).is_err());
        assert_eq!(BluetoothObjectId::new(1).unwrap().value(), 1);
    }
}
