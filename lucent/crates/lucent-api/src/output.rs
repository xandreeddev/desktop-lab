//! Protocol output names, without compositor-specific or renderer handles.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutputName(String);
impl OutputName {
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err("An output name must be bounded, nonempty and contain no controls");
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// A current native output observation. An unnamed output cannot be selected
/// by name; its global number is transient and must not be persisted as identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayOutput {
    pub global: u32,
    pub name: Option<OutputName>,
    pub logical_size: Option<(u32, u32)>,
    pub integer_scale: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_names_are_literal_case_sensitive_and_validated() {
        for value in ["", "bad\0name", "line\nname"] {
            assert!(OutputName::new(value).is_err());
        }
        assert!(OutputName::new("a".repeat(257)).is_err());
        assert_eq!(OutputName::new("DP-1").unwrap().as_str(), "DP-1");
        assert_ne!(
            OutputName::new("DP-1").unwrap(),
            OutputName::new("dp-1").unwrap()
        );
    }
}
