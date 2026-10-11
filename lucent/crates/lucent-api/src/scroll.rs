//! Controlled vertical scrolling. Layout owns viewport/content measurements.
use crate::Color;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollSpec {
    offset: f32,
    line_step: f32,
}
impl ScrollSpec {
    pub fn new(offset: f32, line_step: f32) -> Option<Self> {
        (offset.is_finite() && offset >= 0. && line_step.is_finite() && line_step > 0.)
            .then_some(Self { offset, line_step })
    }
    pub fn offset(self) -> f32 {
        self.offset
    }
    pub fn line_step(self) -> f32 {
        self.line_step
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollEvent {
    pub offset: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct ScrollStyle {
    pub thumb: Color,
    pub width: f32,
    /// Reserved inline space, including the thumb; content never paints under it.
    pub gutter: f32,
    pub minimum_thumb: f32,
}
impl Default for ScrollStyle {
    fn default() -> Self {
        Self {
            thumb: Color::hex(0x888888),
            width: 4.,
            gutter: 12.,
            minimum_thumb: 24.,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scroll_rejects_nonfinite_offsets_and_invalid_steps() {
        for value in [f32::NAN, f32::INFINITY, -1.] {
            assert!(ScrollSpec::new(value, 40.).is_none());
        }
        for value in [f32::NAN, f32::INFINITY, -1., 0.] {
            assert!(ScrollSpec::new(0., value).is_none());
        }
        assert_eq!(ScrollSpec::new(12., 40.).unwrap().offset(), 12.);
    }
}
