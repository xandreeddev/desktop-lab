//! Invariant-bearing values. Screen bounds, grid policy and OS time stay outside.
use crate::{DomainError, Result};
use serde::{Deserialize, Serialize};

/// A finite logical position. Negative coordinates are valid; the client/use case
/// decides which output and widget bounds apply. The persisted shape stays {x,y}.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PlacementRecord", into = "PlacementRecord")]
pub struct Placement {
    x: f32,
    y: f32,
}
impl Placement {
    pub fn new(x: f32, y: f32) -> Result<Self> {
        if !x.is_finite() || !y.is_finite() {
            return Err(DomainError::Invalid(
                "Widget coordinates must be finite".into(),
            ));
        }
        Ok(Self { x, y })
    }
    pub fn x(self) -> f32 {
        self.x
    }
    pub fn y(self) -> f32 {
        self.y
    }
}
#[derive(Serialize, Deserialize)]
struct PlacementRecord {
    x: f32,
    y: f32,
}
impl TryFrom<PlacementRecord> for Placement {
    type Error = DomainError;
    fn try_from(value: PlacementRecord) -> Result<Self> {
        Self::new(value.x, value.y)
    }
}
impl From<Placement> for PlacementRecord {
    fn from(value: Placement) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

/// A saved widget extent with finite, positive dimensions. Minimum sizes,
/// output limits and snapping are supplied by the client and use cases.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "WidgetSizeRecord", into = "WidgetSizeRecord")]
pub struct WidgetSize {
    width: f32,
    height: f32,
}
impl WidgetSize {
    pub fn new(width: f32, height: f32) -> Result<Self> {
        if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
            return Err(DomainError::Invalid(
                "Widget dimensions must be finite and positive".into(),
            ));
        }
        Ok(Self { width, height })
    }
    pub fn width(self) -> f32 {
        self.width
    }
    pub fn height(self) -> f32 {
        self.height
    }
    pub fn tuple(self) -> (f32, f32) {
        (self.width, self.height)
    }
}
#[derive(Serialize, Deserialize)]
struct WidgetSizeRecord {
    width: f32,
    height: f32,
}
impl TryFrom<WidgetSizeRecord> for WidgetSize {
    type Error = DomainError;
    fn try_from(value: WidgetSizeRecord) -> Result<Self> {
        Self::new(value.width, value.height)
    }
}
impl From<WidgetSize> for WidgetSizeRecord {
    fn from(value: WidgetSize) -> Self {
        Self {
            width: value.width,
            height: value.height,
        }
    }
}

/// A valid date in the proleptic Gregorian calendar (including year zero).
/// Weekdays are derived, so they cannot disagree with the date. The full i32 year
/// range is supported; intermediate arithmetic uses i64 to avoid overflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    year: i32,
    month: u32,
    day: u32,
}
impl Date {
    pub fn new(year: i32, month: u32, day: u32) -> Result<Self> {
        if day == 0 || day > Self::days_in_month(year, month) {
            return Err(DomainError::Invalid("Invalid Gregorian date".into()));
        }
        Ok(Self { year, month, day })
    }
    pub fn year(self) -> i32 {
        self.year
    }
    pub fn month(self) -> u32 {
        self.month
    }
    pub fn day(self) -> u32 {
        self.day
    }
    /// Returns zero for a month outside 1..=12.
    pub fn days_in_month(year: i32, month: u32) -> u32 {
        match month {
            4 | 6 | 9 | 11 => 30,
            2 if Self::leap_year(year) => 29,
            2 => 28,
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            _ => 0,
        }
    }
    fn leap_year(year: i32) -> bool {
        year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
    }
    /// Monday = 0. Count complete years/months since 0001-01-01 (a Monday).
    /// Euclidean division extends the leap-year count consistently before year 1.
    pub fn weekday(self) -> u32 {
        let prior = i64::from(self.year) - 1;
        let years =
            prior * 365 + prior.div_euclid(4) - prior.div_euclid(100) + prior.div_euclid(400);
        let preceding =
            [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334][self.month as usize - 1];
        let leap = i64::from(self.month > 2 && Self::leap_year(self.year));
        (years + preceding + leap + i64::from(self.day) - 1).rem_euclid(7) as u32
    }
    pub fn first_weekday(self) -> u32 {
        Self { day: 1, ..self }.weekday()
    }
    /// First day of a relative month, independent of presentation/navigation limits.
    /// Constant-time arithmetic also handles negative offsets and year boundaries.
    pub fn month_start(self, offset: i32) -> Result<Self> {
        let total = i64::from(self.year) * 12 + i64::from(self.month) - 1 + i64::from(offset);
        let year = i32::try_from(total.div_euclid(12)).map_err(|_| {
            DomainError::Invalid("Calendar month exceeds the supported year range".into())
        })?;
        Self::new(year, total.rem_euclid(12) as u32 + 1, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn widget_size_requires_positive_finite_dimensions_including_deserialization() {
        use serde::de::{
            IntoDeserializer,
            value::{Error, MapDeserializer},
        };
        for (width, height) in [(0., 1.), (-1., 1.), (1., f32::NAN), (f32::INFINITY, 1.)] {
            assert!(WidgetSize::new(width, height).is_err());
            let record = [("width", width), ("height", height)]
                .into_iter()
                .map(|(key, value)| (key.into_deserializer(), value.into_deserializer()));
            assert!(WidgetSize::deserialize(MapDeserializer::<_, Error>::new(record)).is_err());
        }
        assert_eq!(WidgetSize::new(320., 240.).unwrap().tuple(), (320., 240.));
    }
    #[test]
    fn placement_construction_and_deserialization_require_finite_coordinates() {
        use serde::de::{
            IntoDeserializer,
            value::{Error, MapDeserializer},
        };
        for (x, y) in [(f32::NAN, 1.), (1., f32::INFINITY), (f32::NEG_INFINITY, 0.)] {
            assert!(Placement::new(x, y).is_err());
            let record = [("x", x), ("y", y)]
                .into_iter()
                .map(|(key, value)| (key.into_deserializer(), value.into_deserializer()));
            assert!(Placement::deserialize(MapDeserializer::<_, Error>::new(record)).is_err());
        }
        let point = Placement::new(-50., f32::MAX).unwrap();
        assert_eq!((point.x(), point.y()), (-50., f32::MAX));
    }
    #[test]
    fn dates_validate_leap_days_and_derive_weekdays() {
        for (year, month, day) in [
            (2024, 0, 1),
            (2024, 13, 1),
            (2024, 1, 0),
            (2024, 4, 31),
            (1900, 2, 29),
            (2100, 2, 29),
        ] {
            assert!(Date::new(year, month, day).is_err());
        }
        for (year, month, day, weekday) in [
            (1, 1, 1, 0),
            (0, 1, 1, 5),
            (1970, 1, 1, 3),
            (2000, 2, 29, 1),
            (2024, 2, 29, 3),
            (2026, 1, 15, 3),
        ] {
            let date = Date::new(year, month, day).unwrap();
            assert_eq!(date.weekday(), weekday);
            assert_eq!(
                date.first_weekday(),
                Date::new(year, month, 1).unwrap().weekday()
            );
        }
    }
    #[test]
    fn relative_months_cross_year_zero_and_handle_extremes_without_overflow() {
        let date = Date::new(2024, 3, 31).unwrap();
        assert_eq!(
            date.month_start(-1).unwrap(),
            Date::new(2024, 2, 1).unwrap()
        );
        assert_eq!(
            date.month_start(10).unwrap(),
            Date::new(2025, 1, 1).unwrap()
        );
        assert_eq!(
            Date::new(0, 1, 1).unwrap().month_start(-1).unwrap(),
            Date::new(-1, 12, 1).unwrap()
        );
        assert!(Date::new(i32::MIN, 1, 1).unwrap().month_start(-1).is_err());
        assert!(Date::new(i32::MAX, 12, 31).unwrap().month_start(1).is_err());
        for year in [i32::MIN, -400, 0, 400, i32::MAX] {
            assert!(Date::new(year, 1, 1).unwrap().weekday() < 7);
        }
        assert!(date.month_start(i32::MIN).is_ok());
        assert!(date.month_start(i32::MAX).is_ok());
    }
}
