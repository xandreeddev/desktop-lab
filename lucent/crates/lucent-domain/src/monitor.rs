//! Persisted monitor identity and widget layout contain no protocol/rendering types.
use crate::{DomainError, Placement, Result, WidgetSize};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MonitorId(String);
impl MonitorId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err(DomainError::Invalid(
                "Monitor identity must be bounded, nonempty and contain no controls".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for MonitorId {
    type Error = DomainError;
    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}
impl From<MonitorId> for String {
    fn from(value: MonitorId) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetLayout {
    pub positions: BTreeMap<String, Placement>,
    pub widget_sizes: BTreeMap<String, WidgetSize>,
    pub visible_widgets: Vec<String>,
}
impl Default for WidgetLayout {
    fn default() -> Self {
        Self {
            positions: BTreeMap::new(),
            widget_sizes: BTreeMap::new(),
            visible_widgets: ["calendar", "clock", "weather", "media"]
                .map(str::to_owned)
                .into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn monitor_identity_is_literal_and_validated_on_deserialization() {
        use serde::de::value::{Error, StringDeserializer};
        for value in ["", "bad\nname", "bad\0name"] {
            assert!(MonitorId::new(value).is_err());
            assert!(
                MonitorId::deserialize(StringDeserializer::<Error>::new(value.into())).is_err()
            );
        }
        assert!(MonitorId::new("x".repeat(257)).is_err());
        assert_ne!(
            MonitorId::new("DP-1").unwrap(),
            MonitorId::new("dp-1").unwrap()
        );
    }
}
