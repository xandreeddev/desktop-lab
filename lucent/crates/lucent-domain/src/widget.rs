//! Widget interaction and finite animation timelines, independent of the platform.
pub const CARD_WIDTH: f32 = 540.0;
pub const CARD_HEIGHT: f32 = 220.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CardVisual {
    pub position: Point,
    pub scale: f32,
    pub opacity: f32,
    pub hover: f32,
    pub color: f32,
}
impl CardVisual {
    /// The actual animated card bounds, in logical output coordinates.
    pub fn bounds(self) -> [f32; 4] {
        let w = CARD_WIDTH * self.scale;
        let h = CARD_HEIGHT * self.scale;
        [
            self.position.x + (CARD_WIDTH - w) / 2.0,
            self.position.y + (CARD_HEIGHT - h) / 2.0,
            w,
            h,
        ]
    }
}

#[derive(Debug)]
struct Tween {
    from: f32,
    to: f32,
    start: f64,
    duration: f64,
}
impl Tween {
    fn fixed(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            start: 0.0,
            duration: 0.2,
        }
    }
    fn value(&self, now: f64) -> f32 {
        let progress = ((now - self.start) / self.duration).clamp(0.0, 1.0) as f32;
        self.from + (self.to - self.from) * (1.0 - (1.0 - progress).powi(3))
    }
    fn target(&mut self, value: f32, now: f64, duration: f64) {
        if self.to != value {
            self.from = self.value(now);
            self.to = value;
            self.start = now;
            self.duration = duration;
        }
    }
    fn active(&self, now: f64) -> bool {
        self.from != self.to && now < self.start + self.duration
    }
}

#[derive(Debug)]
struct Drag {
    pointer: Point,
    origin: Point,
    moved: bool,
}
#[derive(Debug, PartialEq)]
pub enum Release {
    Ignored,
    Clicked,
    Dragged,
}

#[derive(Debug)]
pub struct Widget {
    pub position: Point,
    pub clicks: u64,
    viewport: Point,
    initialized: bool,
    restored: bool,
    drag: Option<Drag>,
    reveal: Tween,
    press: Tween,
    hover: Tween,
    color: Tween,
    closing: bool,
}
impl Widget {
    pub fn new(saved: Option<Point>) -> Self {
        let saved = saved.filter(|p| p.x.is_finite() && p.y.is_finite());
        Self {
            position: saved.unwrap_or_default(),
            clicks: 0,
            viewport: Point::default(),
            initialized: false,
            restored: saved.is_some(),
            drag: None,
            reveal: Tween::fixed(0.0),
            press: Tween::fixed(0.0),
            hover: Tween::fixed(0.0),
            color: Tween::fixed(0.0),
            closing: false,
        }
    }
    pub fn configure(&mut self, width: f32, height: f32, now: f64) {
        self.viewport = Point::new(width, height);
        if !self.initialized {
            if !self.restored {
                self.position =
                    Point::new((width - CARD_WIDTH) / 2.0, (height - CARD_HEIGHT) / 2.0);
            }
            self.reveal.target(1.0, now, 0.35);
            self.initialized = true;
        }
        self.clamp();
        // A resize invalidates the old drag origin.
        self.drag = None;
        self.press.target(0.0, now, 0.18);
    }
    fn clamp(&mut self) {
        self.position.x = self
            .position
            .x
            .clamp(0.0, (self.viewport.x - CARD_WIDTH).max(0.0));
        self.position.y = self
            .position
            .y
            .clamp(0.0, (self.viewport.y - CARD_HEIGHT).max(0.0));
    }
    pub fn hover(&mut self, inside: bool, now: f64) {
        self.hover.target(if inside { 1.0 } else { 0.0 }, now, 0.18);
    }
    pub fn press(&mut self, pointer: Point, now: f64) {
        if !self.closing {
            self.drag = Some(Drag {
                pointer,
                origin: self.position,
                moved: false,
            });
            self.press.target(1.0, now, 0.12);
        }
    }
    pub fn motion(&mut self, pointer: Point) {
        if let Some(drag) = &mut self.drag {
            let dx = pointer.x - drag.pointer.x;
            let dy = pointer.y - drag.pointer.y;
            drag.moved |= dx.hypot(dy) >= 5.0;
            if drag.moved {
                self.position = Point::new(drag.origin.x + dx, drag.origin.y + dy);
                self.clamp();
            }
        }
    }
    pub fn release(&mut self, now: f64) -> Release {
        self.press.target(0.0, now, 0.22);
        let Some(drag) = self.drag.take() else {
            return Release::Ignored;
        };
        if drag.moved {
            Release::Dragged
        } else {
            self.clicks = self.clicks.saturating_add(1);
            self.color.target(
                if self.clicks.is_multiple_of(2) {
                    0.0
                } else {
                    1.0
                },
                now,
                0.3,
            );
            Release::Clicked
        }
    }
    pub fn cancel_drag(&mut self, now: f64) {
        self.drag = None;
        self.press.target(0.0, now, 0.18);
    }
    pub fn close(&mut self, now: f64) {
        self.cancel_drag(now);
        self.closing = true;
        self.reveal.target(0.0, now, 0.2);
    }
    pub fn finished(&self, now: f64) -> bool {
        self.closing && !self.reveal.active(now)
    }
    pub fn animating(&self, now: f64) -> bool {
        [&self.reveal, &self.press, &self.hover, &self.color]
            .iter()
            .any(|t| t.active(now))
    }
    pub fn visual(&self, now: f64) -> CardVisual {
        let reveal = self.reveal.value(now);
        CardVisual {
            position: self.position,
            scale: 0.92 + 0.08 * reveal - 0.018 * self.press.value(now),
            opacity: reveal,
            hover: self.hover.value(now),
            color: self.color.value(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn widget() -> Widget {
        let mut w = Widget::new(None);
        w.configure(1920.0, 1080.0, 0.0);
        w
    }
    #[test]
    fn drag_preserves_pointer_offset_and_is_not_a_click() {
        let mut w = widget();
        let origin = w.position;
        w.press(Point::new(800.0, 500.0), 1.0);
        w.motion(Point::new(1000.0, 600.0));
        assert_eq!(w.position, Point::new(origin.x + 200.0, origin.y + 100.0));
        assert_eq!(w.release(1.1), Release::Dragged);
        assert_eq!(w.clicks, 0);
    }
    #[test]
    fn pointer_jitter_is_a_click_and_does_not_move_the_card() {
        let mut w = widget();
        let origin = w.position;
        w.press(Point::new(800.0, 500.0), 1.0);
        w.motion(Point::new(802.0, 502.0));
        assert_eq!(w.position, origin);
        assert_eq!(w.release(1.1), Release::Clicked);
        assert_eq!(w.clicks, 1);
        assert_eq!(w.release(1.2), Release::Ignored);
    }
    #[test]
    fn dragging_outside_the_output_clamps_and_remains_a_drag() {
        let mut w = widget();
        w.press(Point::new(800.0, 500.0), 1.0);
        w.motion(Point::new(-1000.0, -1000.0));
        assert_eq!(w.position, Point::new(0.0, 0.0));
        w.motion(Point::new(5000.0, 5000.0));
        assert_eq!(w.position, Point::new(1380.0, 860.0));
        assert_eq!(w.release(1.2), Release::Dragged);
    }
    #[test]
    fn persisted_position_is_clamped_after_output_shrinks() {
        let mut w = Widget::new(Some(Point::new(1500.0, 900.0)));
        w.configure(1280.0, 720.0, 0.0);
        assert_eq!(w.position, Point::new(740.0, 500.0));
        w.configure(400.0, 200.0, 1.0);
        assert_eq!(w.position, Point::new(0.0, 0.0));
    }
    #[test]
    fn animations_finish_and_interrupted_tweens_are_continuous() {
        let mut w = widget();
        assert!(w.animating(0.1));
        assert!(!w.animating(0.5));
        assert_eq!(w.visual(0.5).opacity, 1.0);
        w.hover(true, 1.0);
        let halfway = w.visual(1.09).hover;
        w.hover(false, 1.09);
        assert_eq!(w.visual(1.09).hover, halfway);
        assert!(!w.animating(1.4));
        w.close(2.0);
        assert!(!w.finished(2.1));
        assert!(w.finished(2.3));
        assert_eq!(w.visual(2.3).opacity, 0.0);
    }
    #[test]
    fn bad_saved_coordinates_fall_back_to_center() {
        let mut w = Widget::new(Some(Point::new(f32::NAN, 0.0)));
        w.configure(1920.0, 1080.0, 0.0);
        assert_eq!(w.position, widget().position);
    }
}
