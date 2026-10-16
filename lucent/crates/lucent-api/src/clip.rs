//! Shared geometry for paint clipping, hit testing and native input regions.
use crate::Rect;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedRect {
    pub rect: Rect,
    pub radius: f32,
}
impl RoundedRect {
    pub fn new(rect: Rect, radius: f32) -> Self {
        Self {
            rect,
            radius: radius.max(0.).min(rect.w / 2.).min(rect.h / 2.),
        }
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        self.horizontal_span(y)
            .is_some_and(|(left, right)| x >= left && x < right)
    }
    pub fn horizontal_span(self, y: f32) -> Option<(f32, f32)> {
        if y < self.rect.y || y >= self.rect.y + self.rect.h {
            return None;
        }
        let edge =
            ((y - self.rect.y - self.rect.h / 2.).abs() - (self.rect.h / 2. - self.radius)).max(0.);
        let inset = self.radius - (self.radius * self.radius - edge * edge).max(0.).sqrt();
        Some((self.rect.x + inset, self.rect.x + self.rect.w - inset))
    }
}

/// A top-left to bottom-right reveal in logical coordinates. Progress is
/// normalized before storage; a non-finite value reveals nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiagonalReveal {
    pub rect: Rect,
    progress: f32,
}
impl DiagonalReveal {
    pub fn new(rect: Rect, progress: f32) -> Self {
        Self {
            rect,
            progress: normalize_reveal(progress),
        }
    }
    pub fn progress(self) -> f32 {
        self.progress
    }
    pub fn horizontal_span(self, y: f32) -> Option<(f32, f32)> {
        if self.progress <= 0. || y < self.rect.y || y >= self.rect.y + self.rect.h {
            return None;
        }
        let boundary =
            self.rect.x + self.progress * (self.rect.w + self.rect.h) - (y - self.rect.y);
        // Unlike the outer rectangle's right edge, the diagonal includes its
        // boundary. Keep that distinction when quantizing input pixel centers.
        let right = (self.rect.x + self.rect.w).min(boundary.next_up());
        (right > self.rect.x).then_some((self.rect.x, right))
    }
}

pub(crate) fn normalize_reveal(progress: f32) -> f32 {
    if progress.is_finite() {
        progress.clamp(0., 1.)
    } else {
        0.
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipShape {
    Rounded(RoundedRect),
    Diagonal(DiagonalReveal),
}
impl ClipShape {
    pub fn horizontal_span(self, y: f32) -> Option<(f32, f32)> {
        match self {
            Self::Rounded(shape) => shape.horizontal_span(y),
            Self::Diagonal(shape) => shape.horizontal_span(y),
        }
    }
}

/// Intersection of a rectangular bound and zero or more nonrectangular shapes.
/// Descendants share the immutable ancestor chain until another clip is added.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub bounds: Rect,
    shapes: Arc<Vec<ClipShape>>,
}
impl From<Rect> for Clip {
    fn from(bounds: Rect) -> Self {
        Self {
            bounds,
            shapes: Arc::new(vec![]),
        }
    }
}
impl Clip {
    pub fn intersect(&self, rect: Rect) -> Self {
        Self {
            bounds: self.bounds.intersect(rect),
            shapes: self.shapes.clone(),
        }
    }
    pub fn with_rounded(&self, rect: Rect, radius: f32) -> Self {
        let mut clip = self.intersect(rect);
        if radius > 0. {
            Arc::make_mut(&mut clip.shapes)
                .push(ClipShape::Rounded(RoundedRect::new(rect, radius)));
        }
        clip
    }
    /// Clip an element and its descendants to a moving 45-degree cut. The
    /// reveal stays inside `rect`, with exact empty/full endpoint coverage.
    pub fn with_diagonal_reveal(&self, rect: Rect, progress: f32) -> Self {
        let shape = DiagonalReveal::new(rect, progress);
        let mut clip = self.intersect(rect);
        if shape.progress() <= 0. {
            clip.bounds.w = 0.;
            clip.bounds.h = 0.;
        } else if shape.progress() < 1. {
            Arc::make_mut(&mut clip.shapes).push(ClipShape::Diagonal(shape));
        }
        clip
    }
    pub fn shapes(&self) -> &[ClipShape] {
        &self.shapes
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.bounds.contains(x, y)
            && self
                .horizontal_span(y)
                .is_some_and(|(left, right)| x >= left && x < right)
    }
    pub fn horizontal_span(&self, y: f32) -> Option<(f32, f32)> {
        if !y.is_finite() {
            return None;
        }
        let (mut left, mut right) = RoundedRect::new(self.bounds, 0.).horizontal_span(y)?;
        for shape in self.shapes() {
            let (a, b) = shape.horizontal_span(y)?;
            left = left.max(a);
            right = right.min(b);
        }
        (right > left).then_some((left, right))
    }
    /// Wayland regions use integer logical coordinates. Include cells whose
    /// centers are inside this clip; do not shrink an extra pixel at each edge.
    pub fn integer_span(&self, row: i32) -> Option<(i32, i32)> {
        let (left, right) = self.horizontal_span(row as f32 + 0.5)?;
        let (left, right) = ((left - 0.5).ceil() as i32, (right - 0.5).ceil() as i32);
        (right > left).then_some((left, right))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clips_reject_nonfinite_pointer_positions() {
        let bounds = Rect::new(0., 0., 24., 16.);
        for clip in [
            Clip::from(bounds),
            Clip::from(bounds).with_rounded(bounds, 4.),
            Clip::from(bounds).with_diagonal_reveal(bounds, 0.5),
        ] {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                assert!(!clip.contains(4., value));
                assert!(!clip.contains(value, 4.));
                assert!(clip.horizontal_span(value).is_none());
            }
        }
    }
    #[test]
    fn diagonal_cut_preserves_rounded_ancestors_and_scanline_input() {
        let bounds = Rect::new(4., 6., 32., 24.);
        let ancestor = Clip::from(bounds).with_rounded(bounds, 8.);
        for progress in [0., 0.125, 0.5, 0.875, 1.] {
            let clip = ancestor.with_diagonal_reveal(bounds, progress);
            for row in 0..40 {
                for column in 0..44 {
                    let (x, y) = (column as f32 + 0.5, row as f32 + 0.5);
                    let expected = progress > 0.
                        && ancestor.contains(x, y)
                        && (x - bounds.x) + (y - bounds.y) <= progress * (bounds.w + bounds.h);
                    assert_eq!(clip.contains(x, y), expected, "{progress} at {x},{y}");
                    assert_eq!(
                        clip.integer_span(row)
                            .is_some_and(|(left, right)| column >= left && column < right),
                        expected,
                        "native input {progress} at {x},{y}"
                    );
                }
            }
        }
        assert_eq!(
            ancestor.shapes().len(),
            1,
            "shared ancestor stays immutable"
        );
        assert!(matches!(ancestor.shapes()[0], ClipShape::Rounded(_)));
    }

    #[test]
    fn diagonal_endpoints_normalize_without_retaining_a_mask() {
        let bounds = Rect::new(0., 0., 20., 12.);
        let original = Clip::from(bounds);
        for progress in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1., 0.] {
            let clip = original.with_diagonal_reveal(bounds, progress);
            assert!(!clip.contains(0.5, 0.5));
            assert!(clip.integer_span(0).is_none());
            assert!(clip.shapes().is_empty());
            let element = crate::Element::<()>::empty().diagonal_reveal(progress);
            assert_eq!(element.style.diagonal_reveal, Some(0.));
        }
        for progress in [1., 2., f32::MAX] {
            let clip = original.with_diagonal_reveal(bounds, progress);
            assert_eq!(clip, original);
            assert!(clip.contains(19.5, 11.5));
            let element = crate::Element::<()>::empty().diagonal_reveal(progress);
            assert_eq!(element.style.diagonal_reveal, Some(1.));
        }
    }

    #[test]
    fn nested_diagonal_cuts_intersect_and_include_the_cut_boundary() {
        let bounds = Rect::new(0., 0., 24., 16.);
        let clip = Clip::from(bounds)
            .with_diagonal_reveal(bounds, 0.75)
            .with_diagonal_reveal(Rect::new(4., 2., 16., 12.), 0.5);
        assert_eq!(clip.shapes().len(), 2);
        assert!(clip.contains(10.5, 9.5)); // Local x+y is exactly 14.
        assert!(!clip.contains(11.5, 9.5));
        assert!(!clip.contains(3.5, 2.5));
        assert_eq!(clip.integer_span(9), Some((4, 11)));
    }

    #[test]
    fn nested_rounding_agrees_between_hits_and_scanline_regions() {
        let clip = Clip::from(Rect::new(0., 0., 32., 32.))
            .with_rounded(Rect::new(0., 0., 32., 32.), 12.)
            .with_rounded(Rect::new(8., 4., 24., 24.), 8.);
        assert!(!clip.contains(8.5, 4.5));
        assert!(clip.contains(16., 16.));
        for y in 0..32 {
            for x in 0..32 {
                let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
                assert_eq!(
                    clip.contains(x, y),
                    clip.horizontal_span(y)
                        .is_some_and(|(a, b)| x >= a && x < b)
                );
                assert_eq!(
                    clip.contains(x, y),
                    clip.integer_span(y.floor() as i32)
                        .is_some_and(|(a, b)| x.floor() as i32 >= a && (x.floor() as i32) < b)
                );
            }
        }
    }
}
