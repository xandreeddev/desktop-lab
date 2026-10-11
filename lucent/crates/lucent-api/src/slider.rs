//! A controlled, normalized scalar input. Units and effects belong to the client.
use crate::Color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderSpec {
    value: f32,
    step: f32,
}
impl SliderSpec {
    /// Values are in 0..=1; step is finite and in 0..=1, excluding zero.
    pub fn new(value: f32, step: f32) -> Option<Self> {
        (value.is_finite()
            && (0. ..=1.).contains(&value)
            && step.is_finite()
            && step > 0.
            && step <= 1.)
            .then_some(Self { value, step })
    }
    pub fn value(self) -> f32 {
        self.value
    }
    pub fn step(self) -> f32 {
        self.step
    }
    /// Quantize pointer input; endpoints remain reachable with non-dividing steps.
    pub fn at(self, fraction: f32) -> Option<f32> {
        if !fraction.is_finite() {
            return None;
        }
        if fraction <= 0. {
            return Some(0.);
        }
        if fraction >= 1. {
            return Some(1.);
        }
        Some(
            ((f64::from(fraction) / f64::from(self.step)).round() * f64::from(self.step))
                .clamp(0., 1.) as f32,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderEvent {
    pub value: f32,
    /// True on pointer release or each keyboard adjustment.
    pub finished: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct SliderStyle {
    pub track: Color,
    pub fill: Color,
    pub thumb: Color,
    pub track_height: f32,
    pub thumb_diameter: f32,
}
impl Default for SliderStyle {
    fn default() -> Self {
        Self {
            track: Color::hex(0x444444),
            fill: Color::hex(0xffffff),
            thumb: Color::hex(0xffffff),
            track_height: 4.,
            thumb_diameter: 16.,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slider_rejects_invalid_ranges_and_keeps_endpoints() {
        for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(SliderSpec::new(value, 0.1).is_none());
        }
        for step in [f32::NAN, f32::INFINITY, -0.1, 0., 1.1] {
            assert!(SliderSpec::new(0.5, step).is_none());
        }
        let slider = SliderSpec::new(0.5, 0.3).unwrap();
        assert_eq!(slider.at(-10.), Some(0.));
        assert_eq!(slider.at(10.), Some(1.));
        assert_eq!(slider.at(0.55), Some(0.6));
        assert_eq!(slider.at(f32::NAN), None);
    }
}
