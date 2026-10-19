//! Surface-local presentation geometry. Native output positions and application
//! identities do not belong in this contract.
use crate::{Element, Rect, Transition};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlaySide {
    Above,
    Below,
    Left,
    Right,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayAlign {
    Start,
    Center,
    End,
}

/// The caller supplies visual policy. The engine can flip the preferred side
/// and clamp within this surface's viewport; it never invents global coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchoredSpec {
    pub side: OverlaySide,
    pub align: OverlayAlign,
    pub gap: f32,
    pub inset: f32,
}
impl AnchoredSpec {
    pub fn place(self, anchor: Rect, size: (f32, f32), viewport: Rect) -> Option<Rect> {
        if ![
            anchor.x, anchor.y, anchor.w, anchor.h, viewport.x, viewport.y, viewport.w, viewport.h,
            size.0, size.1, self.gap, self.inset,
        ]
        .iter()
        .all(|v| v.is_finite())
            || anchor.w <= 0.
            || anchor.h <= 0.
            || size.0 <= 0.
            || size.1 <= 0.
            || self.gap < 0.
            || self.inset < 0.
        {
            return None;
        }
        if ![
            anchor.x + anchor.w,
            anchor.y + anchor.h,
            viewport.x + viewport.w,
            viewport.y + viewport.h,
        ]
        .iter()
        .all(|v| v.is_finite())
        {
            return None;
        }
        let bounds = Rect::new(
            viewport.x + self.inset,
            viewport.y + self.inset,
            viewport.w - 2. * self.inset,
            viewport.h - 2. * self.inset,
        );
        if ![
            bounds.x,
            bounds.y,
            bounds.w,
            bounds.h,
            bounds.x + bounds.w,
            bounds.y + bounds.h,
        ]
        .iter()
        .all(|v| v.is_finite())
            || bounds.w <= 0.
            || bounds.h <= 0.
            || anchor.intersect(viewport).w <= 0.
            || anchor.intersect(viewport).h <= 0.
        {
            return None;
        }
        let (w, h) = (size.0.min(bounds.w), size.1.min(bounds.h));
        let opposite = match self.side {
            OverlaySide::Above => OverlaySide::Below,
            OverlaySide::Below => OverlaySide::Above,
            OverlaySide::Left => OverlaySide::Right,
            OverlaySide::Right => OverlaySide::Left,
        };
        let available = |side| match side {
            OverlaySide::Above => anchor.y - bounds.y - self.gap,
            OverlaySide::Below => bounds.y + bounds.h - anchor.y - anchor.h - self.gap,
            OverlaySide::Left => anchor.x - bounds.x - self.gap,
            OverlaySide::Right => bounds.x + bounds.w - anchor.x - anchor.w - self.gap,
        };
        let needed = if matches!(self.side, OverlaySide::Above | OverlaySide::Below) {
            h
        } else {
            w
        };
        let side = if available(self.side) < needed && available(opposite) > available(self.side) {
            opposite
        } else {
            self.side
        };
        let align = |origin: f32, extent: f32, length: f32| {
            origin
                + match self.align {
                    OverlayAlign::Start => 0.,
                    OverlayAlign::Center => (extent - length) / 2.,
                    OverlayAlign::End => extent - length,
                }
        };
        let (x, y) = match side {
            OverlaySide::Above => (align(anchor.x, anchor.w, w), anchor.y - self.gap - h),
            OverlaySide::Below => (align(anchor.x, anchor.w, w), anchor.y + anchor.h + self.gap),
            OverlaySide::Left => (anchor.x - self.gap - w, align(anchor.y, anchor.h, h)),
            OverlaySide::Right => (anchor.x + anchor.w + self.gap, align(anchor.y, anchor.h, h)),
        };
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let result = Rect::new(
            x.clamp(bounds.x, bounds.x + bounds.w - w),
            y.clamp(bounds.y, bounds.y + bounds.h - h),
            w,
            h,
        );
        [
            result.x,
            result.y,
            result.w,
            result.h,
            result.x + result.w,
            result.y + result.h,
        ]
        .iter()
        .all(|v| v.is_finite())
        .then_some(result)
    }
}

/// Passive paint attached to an element's exact current geometry. Its content
/// cannot receive input, become a focus target or expand native input regions.
#[derive(Clone)]
pub struct Tooltip<M> {
    pub content: Box<Element<M>>,
    pub placement: AnchoredSpec,
    pub max_width: f32,
    pub delay: f64,
    pub transition: Transition,
}
/// One interactive focus scope in an existing surface. A client controls whether
/// it is present and handles its dismissal; no native popup/session role is implied.
#[derive(Clone)]
pub struct Popover<M> {
    pub id: String,
    pub anchor: String,
    pub content: Box<Element<M>>,
    pub placement: AnchoredSpec,
    pub dismiss: M,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec(side: OverlaySide) -> AnchoredSpec {
        AnchoredSpec {
            side,
            align: OverlayAlign::Center,
            gap: 4.,
            inset: 8.,
        }
    }
    #[test]
    fn anchor_geometry_flips_then_clamps_without_global_coordinates() {
        let viewport = Rect::new(0., 0., 200., 120.);
        assert_eq!(
            spec(OverlaySide::Above).place(Rect::new(80., 4., 24., 24.), (60., 30.), viewport),
            Some(Rect::new(62., 32., 60., 30.))
        );
        assert_eq!(
            spec(OverlaySide::Below).place(Rect::new(180., 80., 16., 16.), (90., 40.), viewport),
            Some(Rect::new(102., 36., 90., 40.))
        );
        assert_eq!(
            spec(OverlaySide::Right).place(Rect::new(180., 40., 16., 16.), (60., 30.), viewport),
            Some(Rect::new(116., 33., 60., 30.))
        );
        assert_eq!(
            spec(OverlaySide::Below).place(Rect::new(80., 40., 20., 20.), (400., 300.), viewport),
            Some(Rect::new(8., 8., 184., 104.))
        );
    }
    #[test]
    fn invalid_or_absent_anchor_geometry_has_no_overlay() {
        let viewport = Rect::new(0., 0., 100., 100.);
        for anchor in [
            Rect::new(f32::NAN, 0., 20., 20.),
            Rect::new(0., 0., 0., 20.),
            Rect::new(200., 0., 20., 20.),
        ] {
            assert!(
                spec(OverlaySide::Below)
                    .place(anchor, (20., 20.), viewport)
                    .is_none()
            );
        }
        assert!(
            AnchoredSpec {
                gap: f32::INFINITY,
                ..spec(OverlaySide::Below)
            }
            .place(Rect::new(20., 20., 20., 20.), (20., 20.), viewport)
            .is_none()
        );
        assert!(
            spec(OverlaySide::Right)
                .place(
                    Rect::new(f32::MAX, 20., 20., 20.),
                    (20., 20.),
                    Rect::new(f32::MAX, 0., f32::MAX, 100.)
                )
                .is_none()
        );
        assert!(
            AnchoredSpec {
                inset: f32::MAX,
                ..spec(OverlaySide::Below)
            }
            .place(Rect::new(20., 20., 20., 20.), (20., 20.), viewport)
            .is_none()
        );
    }
    #[test]
    fn child_mapping_preserves_overlay_messages_and_policy() {
        let child = Element::empty()
            .id("anchor")
            .tooltip(Tooltip {
                content: Box::new(Element::empty().on_click(1)),
                placement: spec(OverlaySide::Above),
                max_width: 100.,
                delay: 0.5,
                transition: Transition::default(),
            })
            .popover(Popover {
                id: "choices".into(),
                anchor: "anchor".into(),
                placement: spec(OverlaySide::Below),
                content: Box::new(Element::empty().on_click(2)),
                dismiss: 3,
            });
        let parent = child.map(|message| format!("child:{message}"));
        let tooltip = parent.tooltip.as_ref().unwrap();
        assert_eq!(tooltip.content.click.as_deref(), Some("child:1"));
        assert_eq!(tooltip.placement.side, OverlaySide::Above);
        assert_eq!(tooltip.delay, 0.5);
        let popover = &parent.popovers[0];
        assert_eq!(popover.content.click.as_deref(), Some("child:2"));
        assert_eq!(popover.dismiss, "child:3");
        assert_eq!(popover.anchor, "anchor");
    }
}
