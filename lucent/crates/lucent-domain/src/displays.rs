//! Display configuration values and transaction authority, independent of native IPC.
use crate::{DomainError, MonitorId, Result, StopSignal};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "(u32, u32, u32)", into = "(u32, u32, u32)")]
pub struct DisplayMode {
    width: u32,
    height: u32,
    millihertz: u32,
}
impl DisplayMode {
    pub fn new(width: u32, height: u32, millihertz: u32) -> Result<Self> {
        if width == 0
            || height == 0
            || width > 32768
            || height > 32768
            || !(1000..=1_000_000).contains(&millihertz)
        {
            return Err(DomainError::Invalid(
                "Display mode dimensions or refresh rate are invalid".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            millihertz,
        })
    }
    pub fn width(self) -> u32 {
        self.width
    }
    pub fn height(self) -> u32 {
        self.height
    }
    pub fn millihertz(self) -> u32 {
        self.millihertz
    }
}
impl TryFrom<(u32, u32, u32)> for DisplayMode {
    type Error = DomainError;
    fn try_from(v: (u32, u32, u32)) -> Result<Self> {
        Self::new(v.0, v.1, v.2)
    }
}
impl From<DisplayMode> for (u32, u32, u32) {
    fn from(v: DisplayMode) -> Self {
        (v.width, v.height, v.millihertz)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct DisplayScale(u32);
impl DisplayScale {
    /// Thousandths of a logical scale factor, with explicit finite bounds.
    pub fn new(thousandths: u32) -> Result<Self> {
        if !(500..=8000).contains(&thousandths) {
            return Err(DomainError::Invalid(
                "Display scale must be between 0.5 and 8".into(),
            ));
        }
        Ok(Self(thousandths))
    }
    pub fn thousandths(self) -> u32 {
        self.0
    }
    pub fn value(self) -> f64 {
        f64::from(self.0) / 1000.
    }
}
impl TryFrom<u32> for DisplayScale {
    type Error = DomainError;
    fn try_from(v: u32) -> Result<Self> {
        Self::new(v)
    }
}
impl From<DisplayScale> for u32 {
    fn from(v: DisplayScale) -> Self {
        v.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "(i32,i32)", into = "(i32,i32)")]
pub struct DisplayPosition(i32, i32);
impl DisplayPosition {
    pub fn new(x: i32, y: i32) -> Result<Self> {
        if x.unsigned_abs() > 65536 || y.unsigned_abs() > 65536 {
            return Err(DomainError::Invalid(
                "Display position is outside the supported layout".into(),
            ));
        }
        Ok(Self(x, y))
    }
    pub fn x(self) -> i32 {
        self.0
    }
    pub fn y(self) -> i32 {
        self.1
    }
}
impl TryFrom<(i32, i32)> for DisplayPosition {
    type Error = DomainError;
    fn try_from(v: (i32, i32)) -> Result<Self> {
        Self::new(v.0, v.1)
    }
}
impl From<DisplayPosition> for (i32, i32) {
    fn from(v: DisplayPosition) -> Self {
        (v.0, v.1)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayConfiguration {
    pub output: MonitorId,
    pub enabled: bool,
    pub mode: DisplayMode,
    pub scale: DisplayScale,
    pub position: DisplayPosition,
    /// Wayland transform codes are validated by the layout use case.
    pub transform: u8,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayDevice {
    pub current: DisplayConfiguration,
    pub modes: Vec<DisplayMode>,
    pub description: String,
    /// Some native states (e.g. mirroring) need richer editing before safe changes.
    pub editable: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct DisplayTransactionId(u64);
impl DisplayTransactionId {
    pub fn new(v: u64) -> Result<Self> {
        if v == 0 {
            return Err(DomainError::Invalid(
                "Display transaction identity is empty".into(),
            ));
        }
        Ok(Self(v))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}
impl TryFrom<u64> for DisplayTransactionId {
    type Error = DomainError;
    fn try_from(v: u64) -> Result<Self> {
        Self::new(v)
    }
}
impl From<DisplayTransactionId> for u64 {
    fn from(v: DisplayTransactionId) -> Self {
        v.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayTransactionPhase {
    Preparing,
    AwaitingConfirmation { remaining_seconds: u32 },
    Kept,
    Reverted { reason: String },
    Failed { reason: String },
}
impl DisplayTransactionPhase {
    pub fn pending(&self) -> bool {
        matches!(self, Self::Preparing | Self::AwaitingConfirmation { .. })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayTransaction {
    pub id: DisplayTransactionId,
    pub phase: DisplayTransactionPhase,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplaySnapshot {
    /// Monotonic observation order within one supplied adapter's lifetime.
    pub revision: u64,
    pub outputs: Vec<DisplayDevice>,
    pub transaction: Option<DisplayTransaction>,
}
impl DisplaySnapshot {
    pub fn configuration(&self) -> Vec<DisplayConfiguration> {
        self.outputs.iter().map(|o| o.current.clone()).collect()
    }
}
/// Implementations own native discovery, transient transactions and independent rollback.
/// Admission is distinct from native completion: always observe the returned identity.
pub trait DisplayPort: Send + Sync {
    /// Query current native state, independently of compositor event coverage.
    fn read(&self) -> Result<DisplaySnapshot>;
    fn watch(&self, emit: &mut dyn FnMut(Result<DisplaySnapshot>), stop: &dyn StopSignal);
    fn apply(
        &self,
        baseline: Vec<DisplayConfiguration>,
        draft: Vec<DisplayConfiguration>,
    ) -> Result<DisplayTransactionId>;
    fn keep(&self, id: DisplayTransactionId) -> Result<()>;
    fn revert(&self, id: DisplayTransactionId) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_values_reject_invalid_wire_values() {
        assert!(DisplayMode::new(0, 1080, 60000).is_err());
        assert!(DisplayMode::new(1920, 1080, 0).is_err());
        assert!(DisplayScale::new(0).is_err());
        assert!(DisplayPosition::new(i32::MIN, 0).is_err());
        assert!(DisplayTransactionId::new(0).is_err());
        assert!(DisplayPosition::new(-1920, 0).is_ok());
    }
}
