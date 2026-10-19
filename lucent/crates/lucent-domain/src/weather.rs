//! Checked, provider-neutral weather values. No network, storage or UI types.
use crate::{Date, DomainError, Result, StopSignal};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CoordinateRecord", into = "CoordinateRecord")]
pub struct WeatherCoordinates {
    latitude: f64,
    longitude: f64,
}
#[derive(Serialize, Deserialize)]
struct CoordinateRecord {
    latitude: f64,
    longitude: f64,
}
impl WeatherCoordinates {
    pub fn new(latitude: f64, longitude: f64) -> Result<Self> {
        if !latitude.is_finite()
            || !longitude.is_finite()
            || latitude.abs() > 90.
            || longitude.abs() > 180.
        {
            return Err(DomainError::Invalid(
                "Latitude must be −90…90 and longitude −180…180".into(),
            ));
        }
        Ok(Self {
            latitude,
            longitude,
        })
    }
    pub fn latitude(self) -> f64 {
        self.latitude
    }
    pub fn longitude(self) -> f64 {
        self.longitude
    }
}
impl TryFrom<CoordinateRecord> for WeatherCoordinates {
    type Error = DomainError;
    fn try_from(value: CoordinateRecord) -> Result<Self> {
        Self::new(value.latitude, value.longitude)
    }
}
impl From<WeatherCoordinates> for CoordinateRecord {
    fn from(value: WeatherCoordinates) -> Self {
        Self {
            latitude: value.latitude,
            longitude: value.longitude,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "LocationRecord", into = "LocationRecord")]
pub struct WeatherLocation {
    name: String,
    coordinates: WeatherCoordinates,
}
#[derive(Serialize, Deserialize)]
struct LocationRecord {
    name: String,
    coordinates: WeatherCoordinates,
}
impl WeatherLocation {
    pub const NAME_LIMIT: usize = 96;
    pub fn new(name: impl Into<String>, coordinates: WeatherCoordinates) -> Result<Self> {
        let name = name.into().trim().to_owned();
        if name.is_empty() || name.len() > Self::NAME_LIMIT || name.chars().any(char::is_control) {
            return Err(DomainError::Invalid(
                "Use a location label of 1…96 bytes without control characters".into(),
            ));
        }
        Ok(Self { name, coordinates })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn coordinates(&self) -> WeatherCoordinates {
        self.coordinates
    }
}
impl TryFrom<LocationRecord> for WeatherLocation {
    type Error = DomainError;
    fn try_from(value: LocationRecord) -> Result<Self> {
        Self::new(value.name, value.coordinates)
    }
}
impl From<WeatherLocation> for LocationRecord {
    fn from(value: WeatherLocation) -> Self {
        Self {
            name: value.name,
            coordinates: value.coordinates,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureUnit {
    #[default]
    Celsius,
    Fahrenheit,
}
impl TemperatureUnit {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Celsius => "°C",
            Self::Fahrenheit => "°F",
        }
    }
    pub fn value(self, temperature: WeatherTemperature) -> f64 {
        let celsius = f64::from(temperature.celsius());
        match self {
            Self::Celsius => celsius,
            Self::Fahrenheit => celsius * 1.8 + 32.,
        }
    }
}
/// None in DesktopSettings means legacy configuration has not been imported.
/// Some(disabled) is explicit authority to make no weather requests.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherPreferences {
    pub enabled: bool,
    pub location: Option<WeatherLocation>,
    pub unit: TemperatureUnit,
}
impl WeatherPreferences {
    pub fn validate(&self) -> Result<()> {
        if self.enabled && self.location.is_none() {
            return Err(DomainError::Invalid(
                "Choose a location before enabling weather".into(),
            ));
        }
        Ok(())
    }
}
/// Canonical Celsius storage makes changing display units a pure operation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeatherTemperature(f32);
impl WeatherTemperature {
    pub fn new(celsius: f64) -> Result<Self> {
        let value = celsius as f32;
        if !celsius.is_finite() || !value.is_finite() || celsius < -273.15 {
            return Err(DomainError::Invalid(
                "Weather temperature is invalid".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn celsius(self) -> f32 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeatherCondition {
    Clear,
    MostlyClear,
    PartlyCloudy,
    Overcast,
    Fog,
    Drizzle,
    FreezingDrizzle,
    FreezingRain,
    Rain,
    Snow,
    Showers,
    SnowShowers,
    Thunderstorm,
    Unknown,
}
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherDay {
    pub date: Date,
    pub condition: WeatherCondition,
    low: WeatherTemperature,
    high: WeatherTemperature,
}
impl WeatherDay {
    pub fn new(
        date: Date,
        condition: WeatherCondition,
        low: WeatherTemperature,
        high: WeatherTemperature,
    ) -> Result<Self> {
        if low.celsius() > high.celsius() {
            return Err(DomainError::Invalid(
                "Forecast minimum exceeds maximum".into(),
            ));
        }
        Ok(Self {
            date,
            condition,
            low,
            high,
        })
    }
    pub fn low(&self) -> WeatherTemperature {
        self.low
    }
    pub fn high(&self) -> WeatherTemperature {
        self.high
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherSnapshot {
    pub temperature: WeatherTemperature,
    pub condition: WeatherCondition,
    pub is_day: bool,
    pub observed_at: u64,
    pub fetched_at: u64,
    days: Vec<WeatherDay>,
}
impl WeatherSnapshot {
    pub const DAYS: usize = 3;
    pub fn new(
        temperature: WeatherTemperature,
        condition: WeatherCondition,
        is_day: bool,
        observed_at: u64,
        fetched_at: u64,
        days: Vec<WeatherDay>,
    ) -> Result<Self> {
        let key = |date: Date| (date.year(), date.month(), date.day());
        if days.len() != Self::DAYS
            || days.windows(2).any(|pair| {
                let date = pair[0].date;
                let next = Date::new(date.year(), date.month(), date.day() + 1)
                    .or_else(|_| date.month_start(1));
                next.ok() != Some(pair[1].date) || key(pair[0].date) >= key(pair[1].date)
            })
        {
            return Err(DomainError::Invalid(
                "Weather needs three ordered forecast dates".into(),
            ));
        }
        Ok(Self {
            temperature,
            condition,
            is_day,
            observed_at,
            fetched_at,
            days,
        })
    }
    pub fn days(&self) -> &[WeatherDay] {
        &self.days
    }
}
pub trait WeatherPort: Send + Sync {
    /// Read-only, bounded legacy import. Explicit saved preferences outrank it.
    fn legacy_preferences(&self) -> Result<Option<WeatherPreferences>>;
    /// One checked location; adapters own provider syntax, deadlines and decoding.
    fn forecast(
        &self,
        coordinates: WeatherCoordinates,
        stop: &dyn StopSignal,
    ) -> Result<WeatherSnapshot>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_values_reject_nonfinite_overflow_and_impossible_input() {
        for value in [f64::NAN, f64::INFINITY, 91.] {
            assert!(WeatherCoordinates::new(value, 0.).is_err());
        }
        assert!(WeatherCoordinates::new(0., 181.).is_err());
        for value in [f64::NAN, f64::INFINITY, 1e300, -274.] {
            assert!(WeatherTemperature::new(value).is_err());
        }
        assert!(WeatherLocation::new("\n", WeatherCoordinates::new(0., 0.).unwrap()).is_err());
        assert!(
            WeatherPreferences {
                enabled: true,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        let freezing = WeatherTemperature::new(0.).unwrap();
        assert_eq!(TemperatureUnit::Fahrenheit.value(freezing), 32.);
    }
    #[test]
    fn forecast_has_bounded_ordered_dates_and_checked_ranges() {
        let low = WeatherTemperature::new(3.).unwrap();
        let high = WeatherTemperature::new(8.).unwrap();
        let day = |n| {
            WeatherDay::new(
                Date::new(2026, 1, n).unwrap(),
                WeatherCondition::Clear,
                low,
                high,
            )
            .unwrap()
        };
        assert!(
            WeatherDay::new(
                Date::new(2026, 1, 1).unwrap(),
                WeatherCondition::Rain,
                high,
                low
            )
            .is_err()
        );
        assert!(
            WeatherSnapshot::new(
                low,
                WeatherCondition::Clear,
                true,
                0,
                0,
                vec![day(1), day(1), day(3)]
            )
            .is_err()
        );
        assert!(
            WeatherSnapshot::new(
                low,
                WeatherCondition::Clear,
                true,
                0,
                0,
                vec![day(1), day(2)]
            )
            .is_err()
        );
        assert_eq!(
            WeatherSnapshot::new(
                low,
                WeatherCondition::Clear,
                true,
                0,
                0,
                vec![day(1), day(2), day(3)]
            )
            .unwrap()
            .days()
            .len(),
            3
        );
    }
}
