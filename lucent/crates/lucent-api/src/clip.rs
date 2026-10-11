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

/// Intersection of a rectangular bound and zero or more rounded rectangles.
/// Descendants share the immutable ancestor chain until a new round clip is added.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub bounds: Rect,
    rounded: Arc<Vec<RoundedRect>>,
}
impl From<Rect> for Clip {
    fn from(bounds: Rect) -> Self {
        Self {
            bounds,
            rounded: Arc::new(vec![]),
        }
    }
}
impl Clip {
    pub fn intersect(&self, rect: Rect) -> Self {
        Self {
            bounds: self.bounds.intersect(rect),
            rounded: self.rounded.clone(),
        }
    }
    pub fn with_rounded(&self, rect: Rect, radius: f32) -> Self {
        let mut clip = self.intersect(rect);
        if radius > 0. {
            Arc::make_mut(&mut clip.rounded).push(RoundedRect::new(rect, radius));
        }
        clip
    }
    pub fn shapes(&self) -> &[RoundedRect] {
        &self.rounded
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.bounds.contains(x, y) && self.rounded.iter().all(|shape| shape.contains(x, y))
    }
    pub fn horizontal_span(&self, y: f32) -> Option<(f32, f32)> {
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
