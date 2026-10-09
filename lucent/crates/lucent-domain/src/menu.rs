//! A selectable value is separate from its presentation; labels need not be unique.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MenuEntry {
    pub label: String,
    #[serde(default)]
    pub detail: String,
    pub value: String,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuOutcome {
    Accepted(String),
    Cancelled,
    Parent,
}
