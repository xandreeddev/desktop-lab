//! Renderer-independent semantic colors. Palettes are persisted values, not mode flags.
use crate::{DomainError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb(u32);
impl Rgb {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
    }
    pub fn channels(self) -> [u8; 3] {
        [(self.0 >> 16) as u8, (self.0 >> 8) as u8, self.0 as u8]
    }
    pub fn hex(self) -> String {
        format!("#{:06x}", self.0)
    }
    pub fn luminance(self) -> f32 {
        let [r, g, b] = self.channels().map(|v| {
            let c = f32::from(v) / 255.;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        });
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }
    pub fn contrast(self, other: Self) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    pub fn mix(self, other: Self, amount: f32) -> Self {
        let a = self.channels();
        let b = other.channels();
        let t = amount.clamp(0., 1.);
        let c = std::array::from_fn::<_, 3, _>(|i| {
            (f32::from(a[i]) * (1. - t) + f32::from(b[i]) * t).round() as u8
        });
        Self::new(c[0], c[1], c[2])
    }
}
impl From<Rgb> for String {
    fn from(value: Rgb) -> Self {
        value.hex()
    }
}
impl TryFrom<String> for Rgb {
    type Error = String;
    fn try_from(value: String) -> std::result::Result<Self, String> {
        if value.len() != 7
            || !value.starts_with('#')
            || !value[1..].bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("Expected #rrggbb color".into());
        }
        u32::from_str_radix(&value[1..], 16)
            .map(Self)
            .map_err(|_| "Invalid color".into())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteColors {
    pub surface: Rgb,
    pub surface_container: Rgb,
    pub on_surface: Rgb,
    pub primary: Rgb,
    pub on_primary: Rgb,
    pub error: Rgb,
    pub success: Rgb,
    pub warning: Rgb,
    pub info: Rgb,
    pub focus: Rgb,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemePalette {
    pub id: String,
    pub name: String,
    pub colors: PaletteColors,
}
impl ThemePalette {
    pub fn light(&self) -> bool {
        self.colors.surface.luminance() > 0.5
    }
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.id.len() > 96
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || self.name.trim().is_empty()
            || self.name.len() > 120
        {
            return Err(DomainError::Invalid("Invalid palette identity".into()));
        }
        let c = &self.colors;
        for (fg, bg) in [
            (c.on_surface, c.surface),
            (c.on_surface, c.surface_container),
            (c.on_primary, c.primary),
            (c.error, c.surface),
        ] {
            if fg.contrast(bg) < 4.5 {
                return Err(DomainError::Invalid(
                    "Palette text contrast must be at least 4.5:1".into(),
                ));
            }
        }
        Ok(())
    }
}
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
/// Export an already-selected Lucent palette. Never choose a distribution theme.
pub trait ThemePort: Send + Sync {
    fn apply(&self, palette: &ThemePalette) -> Result<()>;
}
/// Image decoding is an adapter concern; callers receive semantic domain colors.
pub trait PaletteGenerationPort: Send + Sync {
    fn seed(&self, path: &str) -> Result<Rgb>;
}
