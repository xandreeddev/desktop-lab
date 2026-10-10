//! Appearance policy is selected by Lucent; adapters propagate it to other apps.
use crate::Result;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}
impl ThemeMode {
    pub fn from_light(light: bool) -> Self {
        if light { Self::Light } else { Self::Dark }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}
/// Applies the client's exported token palette. It never selects a distro theme.
pub trait ThemePort: Send + Sync {
    fn apply(&self, mode: ThemeMode) -> Result<()>;
}
