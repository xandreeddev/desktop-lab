//! One owned connection operation, separate from pairing and the observed link.
use crate::{BluetoothObjectId, DomainError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BluetoothConnectionId(u64);
impl BluetoothConnectionId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid(
                "Bluetooth connection identity is empty".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BluetoothConnectionPhase {
    Connecting,
    Succeeded,
    Cancelled,
    Failed(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothConnectionAttempt {
    pub id: BluetoothConnectionId,
    pub device: BluetoothObjectId,
    pub phase: BluetoothConnectionPhase,
}
