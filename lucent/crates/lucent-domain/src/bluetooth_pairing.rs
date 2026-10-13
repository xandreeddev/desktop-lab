//! Pairing conversations without transport paths or UI state.
use crate::{BluetoothObjectId, DomainError, Result, Secret};
use std::fmt;

/// Transient attempt or prompt identity allocated by the observing adapter.
/// Reconnection, completion and cancellation retire the corresponding scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BluetoothPairingId(u64);
impl BluetoothPairingId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid(
                "Empty Bluetooth pairing identity".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}

/// A displayed comparison/keyboard code. Debug never reveals its digits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BluetoothPairingCode(u32);
impl BluetoothPairingCode {
    pub fn new(value: u32) -> Result<Self> {
        if value > 999_999 {
            return Err(DomainError::Invalid("Invalid Bluetooth passkey".into()));
        }
        Ok(Self(value))
    }
    pub fn digits(self) -> String {
        format!("{:06}", self.0)
    }
}
impl fmt::Debug for BluetoothPairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BluetoothPairingCode([redacted])")
    }
}

/// Native Bluetooth service identity, distinct from executable actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothServiceId(String);
impl BluetoothServiceId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.len() != 36
            || value.bytes().enumerate().any(|(i, c)| {
                if [8, 13, 18, 23].contains(&i) {
                    c != b'-'
                } else {
                    !c.is_ascii_hexdigit()
                }
            })
        {
            return Err(DomainError::Invalid(
                "Invalid Bluetooth service identity".into(),
            ));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct BluetoothPin(Secret);
impl BluetoothPin {
    pub fn new(secret: Secret) -> Result<Self> {
        let value = secret.expose();
        if value.is_empty() || value.len() > 16 || !value.bytes().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(DomainError::Invalid(
                "Bluetooth PIN must contain 1–16 letters or digits".into(),
            ));
        }
        Ok(Self(secret))
    }
    pub fn expose(&self) -> &str {
        self.0.expose()
    }
}
pub struct BluetoothPasskeyInput(Secret);
impl BluetoothPasskeyInput {
    pub fn new(secret: Secret) -> Result<Self> {
        let value = secret.expose();
        if value.is_empty() || value.len() > 6 || !value.bytes().all(|c| c.is_ascii_digit()) {
            return Err(DomainError::Invalid(
                "Bluetooth passkey must contain 1–6 digits".into(),
            ));
        }
        Ok(Self(secret))
    }
    pub fn number(&self) -> u32 {
        self.0.expose().parse().expect("validated bounded digits")
    }
}

/// An answer consumes owned input and cannot be cloned or debug-printed.
pub enum BluetoothPairingAnswer {
    Accept,
    Reject,
    PinCode(BluetoothPin),
    Passkey(BluetoothPasskeyInput),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BluetoothPairingPrompt {
    PinCode,
    Passkey,
    DisplayPinCode(BluetoothPairingCode),
    DisplayPasskey {
        code: BluetoothPairingCode,
        entered: u16,
    },
    ConfirmPasskey(BluetoothPairingCode),
    AuthorizePairing,
    AuthorizeService(BluetoothServiceId),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothPairingRequest {
    pub id: BluetoothPairingId,
    pub device: BluetoothObjectId,
    pub prompt: BluetoothPairingPrompt,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BluetoothPairingPhase {
    Pairing,
    Succeeded,
    Cancelled,
    Failed(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothPairingAttempt {
    pub id: BluetoothPairingId,
    pub device: BluetoothObjectId,
    pub phase: BluetoothPairingPhase,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairing_values_validate_without_exposing_codes_or_owned_input_in_debug() {
        assert!(BluetoothPairingId::new(0).is_err());
        let code = BluetoothPairingCode::new(17).unwrap();
        assert_eq!(code.digits(), "000017");
        assert!(!format!("{code:?}").contains("000017"));
        assert!(BluetoothPairingCode::new(1_000_000).is_err());
        for bad in ["", "1234567", "-1", "12x", "１２３"] {
            assert!(BluetoothPasskeyInput::new(Secret::new(bad.into())).is_err());
        }
        assert_eq!(
            BluetoothPasskeyInput::new(Secret::new("000017".into()))
                .unwrap()
                .number(),
            17
        );
        assert!(BluetoothPin::new(Secret::new("fixturePin1".into())).is_ok());
        assert!(BluetoothPin::new(Secret::new("1\n2".into())).is_err());
        assert!(BluetoothPin::new(Secret::new("12345678901234567".into())).is_err());
        assert!(BluetoothServiceId::new("0000110b-0000-1000-8000-00805f9b34fb").is_ok());
        assert!(BluetoothServiceId::new("invalid").is_err());
    }
}
