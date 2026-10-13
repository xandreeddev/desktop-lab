//! Power capabilities and intents, independent of D-Bus, sysfs and rendering.
use crate::{DomainError, Result, StopSignal};

/// Finite percentage in the inclusive 0..=100 range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Percentage(f64);
impl Percentage {
    pub fn new(value: f64) -> Result<Self> {
        if !value.is_finite() || !(0. ..=100.).contains(&value) {
            return Err(DomainError::Invalid(
                "Percentage must be between 0 and 100".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> f64 {
        self.0
    }
}

/// A currently observed profile or backlight; never persist this handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PowerObjectId(u64);
impl PowerObjectId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid("Power identity is empty".into()));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerOperationId(u64);
impl PowerOperationId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid(
                "Power operation identity is empty".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum PowerCapability<T> {
    Ready(T),
    Unavailable(String),
}
impl<T> Default for PowerCapability<T> {
    fn default() -> Self {
        Self::Unavailable("Not available".into())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryState {
    Unknown,
    Charging,
    Discharging,
    Empty,
    Full,
    PendingCharge,
    PendingDischarge,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryLevel {
    None,
    Unknown,
    Low,
    Critical,
    Normal,
    High,
    Full,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Battery {
    pub charge: Percentage,
    /// Coarse levels must not be presented as an exact percentage.
    pub level: BatteryLevel,
    pub state: BatteryState,
    pub on_battery: bool,
    pub seconds_to_empty: Option<u64>,
    pub seconds_to_full: Option<u64>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PowerProfile {
    pub id: PowerObjectId,
    pub name: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerProfiles {
    pub profiles: Vec<PowerProfile>,
    pub active: Option<PowerObjectId>,
    pub performance_degraded: Option<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct BrightnessDevice {
    pub id: PowerObjectId,
    pub name: String,
    pub brightness: Percentage,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    Suspend,
    Hibernate,
    HybridSleep,
    Reboot,
    PowerOff,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerPermission {
    Allowed,
    Authentication,
    Denied,
    Unsupported,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PowerActionCapability {
    pub action: PowerAction,
    pub permission: PowerPermission,
}
#[derive(Clone, Debug, PartialEq)]
pub enum PowerOperationPhase {
    InProgress,
    Succeeded,
    Failed(String),
}
#[derive(Clone, Debug, PartialEq)]
pub struct PowerOperation {
    pub id: PowerOperationId,
    pub command: PowerCommand,
    pub phase: PowerOperationPhase,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerSnapshot {
    /// Ready(None) means no display battery/UPS; it differs from unavailable.
    pub battery: PowerCapability<Option<Battery>>,
    pub profiles: PowerCapability<PowerProfiles>,
    pub brightness: PowerCapability<Vec<BrightnessDevice>>,
    pub session: PowerCapability<Vec<PowerActionCapability>>,
    pub preparing_for_sleep: bool,
    pub operation: Option<PowerOperation>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum PowerCommand {
    SetProfile(PowerObjectId),
    SetBrightness(PowerObjectId, Percentage),
    Session(PowerAction),
}
pub trait PowerPort: Send + Sync {
    /// Observe independently available capabilities; the subscriber owns lifetime.
    fn watch(&self, emit: &mut dyn FnMut(Result<PowerSnapshot>), stop: &dyn StopSignal);
    /// Admit an explicit operation before native completion or authorization.
    /// Observe its phase and actual values. An accepted action may not be undoable.
    fn control(&self, command: PowerCommand) -> Result<PowerOperationId>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn power_values_reject_nonfinite_ranges_and_empty_handles() {
        for value in [f64::NAN, f64::INFINITY, -1., 100.01] {
            assert!(Percentage::new(value).is_err());
        }
        for value in [0., 50.5, 100.] {
            assert_eq!(Percentage::new(value).unwrap().value(), value);
        }
        assert!(PowerObjectId::new(0).is_err());
        assert!(PowerOperationId::new(0).is_err());
    }
}
