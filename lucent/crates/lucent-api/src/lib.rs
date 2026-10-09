//! A small declarative shell API. Public types contain no Wayland or GPU handles.
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            x,
            y,
            w: w.max(0.),
            h: h.max(0.),
        }
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Self::new(
            x,
            y,
            (self.x + self.w).min(other.x + other.w) - x,
            (self.y + self.h).min(other.y + other.h) - y,
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color(pub f32, pub f32, pub f32, pub f32);
impl Color {
    pub const TRANSPARENT: Self = Self(0., 0., 0., 0.);
    pub const fn hex(rgb: u32) -> Self {
        Self(
            ((rgb >> 16) & 255) as f32 / 255.,
            ((rgb >> 8) & 255) as f32 / 255.,
            (rgb & 255) as f32 / 255.,
            1.,
        )
    }
    pub fn alpha(self, a: f32) -> Self {
        Self(self.0, self.1, self.2, a)
    }
    pub fn mix(self, b: Self, t: f32) -> Self {
        Self(
            self.0 + (b.0 - self.0) * t,
            self.1 + (b.1 - self.1) * t,
            self.2 + (b.2 - self.2) * t,
            self.3 + (b.3 - self.3) * t,
        )
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Length {
    Fixed(f32),
    Fill,
    #[default]
    Shrink,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}
/// Client-defined timing for an interaction transition.
#[derive(Clone, Copy, Debug)]
pub struct Transition {
    pub duration: f64,
    pub curve: [f32; 4],
}
impl Default for Transition {
    fn default() -> Self {
        Self {
            duration: 0.15,
            curve: [0.2, 0., 0., 1.],
        }
    }
}
/// Symmetric content insets in logical pixels, independent on each axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Padding {
    pub horizontal: f32,
    pub vertical: f32,
}
#[derive(Clone, Debug)]
pub struct Style {
    pub width: Length,
    pub height: Length,
    pub padding: Padding,
    pub gap: f32,
    pub position: Option<(f32, f32)>,
    pub background: Color,
    pub foreground: Color,
    pub radius: f32,
    pub font_size: f32,
    pub font_face: usize,
    pub align: Align,
    pub opacity: f32,
    pub clip: bool,
    pub shadow: bool,
    pub shadow_color: Color,
    pub shadow_offset: f32,
    /// Tint and strength of the hover state layer; transparent disables it.
    pub hover_color: Color,
    pub hover_transition: Transition,
    /// Multiplicative image tint; white preserves original image colors.
    pub image_tint: Color,
    /// Preserve image aspect ratio within its layout box.
    pub image_contain: bool,
    pub focus_color: Color,
    pub focus_width: f32,
    pub focus_outline: bool,
    pub focus_within: bool,
    /// Controlled selection, independent of pointer hover and text-input focus.
    pub selected: bool,
    pub tab_stop: bool,
    pub caret_width: f32,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            width: Length::Shrink,
            height: Length::Shrink,
            padding: Padding::default(),
            gap: 0.,
            position: None,
            background: Color::TRANSPARENT,
            foreground: Color::hex(0xffffff),
            radius: 0.,
            font_size: 14.,
            font_face: 0,
            align: Align::Start,
            opacity: 1.,
            clip: false,
            shadow: false,
            shadow_color: Color(0., 0., 0., 0.2),
            shadow_offset: 4.,
            hover_color: Color::TRANSPARENT,
            hover_transition: Transition::default(),
            image_tint: Color::hex(0xffffff),
            image_contain: false,
            focus_color: Color::hex(0xffffff),
            focus_width: 1.,
            focus_outline: true,
            focus_within: false,
            selected: false,
            tab_stop: true,
            caret_width: 1.,
        }
    }
}
#[derive(Clone, Debug)]
pub struct ImageData {
    pub key: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Clone, Debug)]
pub enum Kind {
    Row,
    Column,
    Stack,
    Grid(usize),
    Text(String),
    Image(Arc<ImageData>),
    Input { value: String, placeholder: String },
    Empty,
}
#[derive(Clone, Copy, Debug)]
pub struct DragEvent {
    pub dx: f32,
    pub dy: f32,
    pub finished: bool,
}
pub type InputCallback<M> = Arc<dyn Fn(String) -> M + Send + Sync>;
pub type DragCallback<M> = Arc<dyn Fn(DragEvent) -> M + Send + Sync>;
#[derive(Clone)]
pub struct Element<M> {
    pub id: String,
    pub kind: Kind,
    pub style: Style,
    pub children: Vec<Element<M>>,
    pub click: Option<M>,
    pub hover: Option<M>,
    pub input: Option<InputCallback<M>>,
    pub drag: Option<DragCallback<M>>,
    pub autofocus: bool,
}
impl<M> Element<M> {
    pub fn new(kind: Kind) -> Self {
        Self {
            id: String::new(),
            kind,
            style: Style::default(),
            children: vec![],
            click: None,
            hover: None,
            input: None,
            drag: None,
            autofocus: false,
        }
    }
    pub fn text(text: impl Into<String>) -> Self {
        Self::new(Kind::Text(text.into()))
    }
    pub fn image(image: Arc<ImageData>) -> Self {
        Self::new(Kind::Image(image))
    }
    pub fn row(children: Vec<Self>) -> Self {
        let mut e = Self::new(Kind::Row);
        e.children = children;
        e
    }
    pub fn column(children: Vec<Self>) -> Self {
        let mut e = Self::new(Kind::Column);
        e.children = children;
        e
    }
    pub fn stack(children: Vec<Self>) -> Self {
        let mut e = Self::new(Kind::Stack);
        e.children = children;
        e
    }
    pub fn grid(columns: usize, children: Vec<Self>) -> Self {
        let mut e = Self::new(Kind::Grid(columns.max(1)));
        e.children = children;
        e
    }
    pub fn empty() -> Self {
        Self::new(Kind::Empty)
    }
    pub fn button(label: impl Into<String>, message: M) -> Self {
        Self::text(label).on_click(message)
    }
    pub fn input(
        value: impl Into<String>,
        placeholder: impl Into<String>,
        on_change: impl Fn(String) -> M + Send + Sync + 'static,
    ) -> Self {
        let mut e = Self::new(Kind::Input {
            value: value.into(),
            placeholder: placeholder.into(),
        });
        e.input = Some(Arc::new(on_change));
        e
    }
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }
    pub fn width(mut self, v: Length) -> Self {
        self.style.width = v;
        self
    }
    pub fn height(mut self, v: Length) -> Self {
        self.style.height = v;
        self
    }
    pub fn size(self, w: f32, h: f32) -> Self {
        self.width(Length::Fixed(w)).height(Length::Fixed(h))
    }
    pub fn fill(self) -> Self {
        self.width(Length::Fill).height(Length::Fill)
    }
    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.style.position = Some((x, y));
        self
    }
    pub fn padding(self, v: f32) -> Self {
        self.padding_xy(v, v)
    }
    /// Inset content horizontally and vertically without shrinking the hit target.
    pub fn padding_xy(mut self, horizontal: f32, vertical: f32) -> Self {
        self.style.padding = Padding {
            horizontal: horizontal.max(0.),
            vertical: vertical.max(0.),
        };
        self
    }
    pub fn gap(mut self, v: f32) -> Self {
        self.style.gap = v;
        self
    }
    pub fn background(mut self, v: Color) -> Self {
        self.style.background = v;
        self
    }
    pub fn contain(mut self) -> Self {
        self.style.image_contain = true;
        self
    }
    pub fn selected(mut self, value: bool) -> Self {
        self.style.selected = value;
        self
    }
    /// Decorate a composite field when one of its descendants has keyboard focus.
    pub fn focus_within(mut self) -> Self {
        self.style.focus_within = true;
        self
    }
    pub fn focus_outline(mut self, enabled: bool) -> Self {
        self.style.focus_outline = enabled;
        self
    }
    pub fn tab_stop(mut self, enabled: bool) -> Self {
        self.style.tab_stop = enabled;
        self
    }
    pub fn tint(mut self, v: Color) -> Self {
        self.style.image_tint = v;
        self
    }
    pub fn color(mut self, v: Color) -> Self {
        self.style.foreground = v;
        self
    }
    pub fn radius(mut self, v: f32) -> Self {
        self.style.radius = v;
        self
    }
    pub fn font(mut self, v: f32) -> Self {
        self.style.font_size = v;
        self
    }
    /// Index into the application's registered fonts (zero is the default face).
    pub fn font_face(mut self, index: usize) -> Self {
        self.style.font_face = index;
        self
    }
    pub fn align(mut self, v: Align) -> Self {
        self.style.align = v;
        self
    }
    pub fn opacity(mut self, v: f32) -> Self {
        self.style.opacity = v;
        self
    }
    pub fn clip(mut self) -> Self {
        self.style.clip = true;
        self
    }
    pub fn shadow(mut self) -> Self {
        self.style.shadow = true;
        self
    }
    pub fn on_click(mut self, msg: M) -> Self {
        self.click = Some(msg);
        self
    }
    pub fn on_hover(mut self, msg: M) -> Self {
        self.hover = Some(msg);
        self
    }
    pub fn on_drag(mut self, f: impl Fn(DragEvent) -> M + Send + Sync + 'static) -> Self {
        self.drag = Some(Arc::new(f));
        self
    }
    pub fn autofocus(mut self) -> Self {
        self.autofocus = true;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Background,
    Bottom,
    Top,
    Overlay,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Top,
    Bottom,
    Fill,
    Center,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyboard {
    None,
    OnDemand,
    Exclusive,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceSpec {
    pub id: &'static str,
    pub layer: Layer,
    pub anchor: Anchor,
    pub width: u32,
    pub height: u32,
    pub exclusive_zone: i32,
    pub keyboard: Keyboard,
    pub visible: bool,
    pub capture_all: bool,
}
#[derive(Clone, Debug)]
pub struct ViewContext {
    pub surface: &'static str,
    pub width: f32,
    pub height: f32,
    pub now: f64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Text(String),
    Backspace,
    Delete,
    Enter,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Tab,
    BackTab,
    Home,
    End,
    SelectAll,
}
#[derive(Clone, Debug)]
pub enum Event {
    Resize {
        surface: &'static str,
        width: f32,
        height: f32,
    },
    Key {
        surface: &'static str,
        key: Key,
    },
    Scroll {
        surface: &'static str,
        lines: f64,
    },
    Outside {
        surface: &'static str,
    },
}

/// Components own their state; the runtime delivers typed messages on one UI thread.
pub trait Component: 'static {
    type Message: Clone + Send + 'static;
    fn view(&self, cx: &ViewContext) -> Element<Self::Message>;
    fn update(&mut self, message: Self::Message, effects: &mut Effects<Self::Message>);
}
/// An application supplies surfaces and subscriptions in addition to its component tree.
pub trait Application: Component {
    /// Register font bytes once. Elements select a face by index; an empty list uses Fontconfig.
    fn fonts(&self) -> Vec<&'static [u8]> {
        vec![]
    }
    fn name(&self) -> &'static str;
    fn surfaces(&self) -> Vec<SurfaceSpec>;
    fn init(&mut self, _effects: &mut Effects<Self::Message>) {}
    fn subscriptions(&self) -> Vec<Subscription<Self::Message>> {
        vec![]
    }
    fn event(&self, _event: Event) -> Option<Self::Message> {
        None
    }
    fn command(&self, _command: &str) -> Result<Option<Self::Message>, String> {
        Err("Unknown command".into())
    }
    fn inspect(&self) -> String {
        "{}".into()
    }
    fn animating(&self, _surface: &str, _now: f64) -> bool {
        false
    }
}

pub type Task<M> = Box<dyn FnOnce() -> M + Send>;
pub struct Effects<M> {
    pub now: f64,
    pub tasks: Vec<Task<M>>,
    pub redraw: BTreeSet<&'static str>,
    pub exit: bool,
}
impl<M> Default for Effects<M> {
    fn default() -> Self {
        Self {
            now: 0.,
            tasks: vec![],
            redraw: BTreeSet::new(),
            exit: false,
        }
    }
}
impl<M> Effects<M> {
    pub fn task(&mut self, task: impl FnOnce() -> M + Send + 'static) {
        self.tasks.push(Box::new(task));
    }
    pub fn redraw(&mut self, surface: &'static str) {
        self.redraw.insert(surface);
    }
    /// Run a child update and lift its async results into the parent's message type.
    pub fn delegate<C: Send + 'static>(
        &mut self,
        update: impl FnOnce(&mut Effects<C>),
        map: impl Fn(C) -> M + Send + Sync + 'static,
    ) where
        M: 'static,
    {
        let mut child = Effects {
            now: self.now,
            ..Default::default()
        };
        update(&mut child);
        self.redraw.extend(child.redraw);
        self.exit |= child.exit;
        let map = Arc::new(map);
        for task in child.tasks {
            let map = map.clone();
            self.task(move || map(task()));
        }
    }
    pub fn quit(&mut self) {
        self.exit = true;
    }
}
#[derive(Clone)]
pub struct Cancellation(Arc<AtomicBool>);
impl Default for Cancellation {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
}
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    pub fn sleep(&self, duration: Duration) {
        let start = std::time::Instant::now();
        while !self.cancelled() && start.elapsed() < duration {
            std::thread::sleep(
                (duration - start.elapsed().min(duration)).min(Duration::from_millis(100)),
            );
        }
    }
}
pub struct Emitter<M>(Arc<dyn Fn(M) + Send + Sync>);
impl<M> Clone for Emitter<M> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<M> Emitter<M> {
    pub fn new(f: impl Fn(M) + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }
    pub fn send(&self, message: M) {
        (self.0)(message)
    }
}
pub struct Subscription<M> {
    pub id: &'static str,
    pub run: Box<dyn FnOnce(Emitter<M>, Cancellation) + Send>,
}
impl<M: Send + 'static> Subscription<M> {
    pub fn stream(
        id: &'static str,
        run: impl FnOnce(Emitter<M>, Cancellation) + Send + 'static,
    ) -> Self {
        Self {
            id,
            run: Box::new(run),
        }
    }
    pub fn every(
        id: &'static str,
        interval: Duration,
        make: impl Fn() -> M + Send + 'static,
    ) -> Self {
        Self::stream(id, move |out, cancel| {
            while !cancel.cancelled() {
                out.send(make());
                cancel.sleep(interval);
            }
        })
    }
}

/// Interruptible cubic Bézier animation. X is time; Y may overshoot for spatial motion.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    from: f32,
    to: f32,
    start: f64,
    duration: f64,
    curve: [f32; 4],
}
impl Motion {
    pub fn fixed(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            start: 0.,
            duration: 0.3,
            curve: [0.05, 0.7, 0.1, 1.],
        }
    }
    pub fn value(self, now: f64) -> f32 {
        if now >= self.start + self.duration {
            return self.to;
        }
        let x = ((now - self.start) / self.duration).clamp(0., 1.) as f32;
        let bez = |t: f32, a: f32, b: f32| {
            3. * (1. - t).powi(2) * t * a + 3. * (1. - t) * t * t * b + t * t * t
        };
        let (mut lo, mut hi) = (0., 1.);
        for _ in 0..14 {
            let m = (lo + hi) * 0.5;
            if bez(m, self.curve[0], self.curve[2]) < x {
                lo = m
            } else {
                hi = m
            }
        }
        self.from + (self.to - self.from) * bez((lo + hi) * 0.5, self.curve[1], self.curve[3])
    }
    pub fn target(&mut self, value: f32, now: f64, duration: f64, curve: [f32; 4]) {
        if self.to != value {
            self.from = self.value(now);
            self.to = value;
            self.start = now;
            self.duration = duration.max(0.001);
            self.curve = curve;
        }
    }
    pub fn active(self, now: f64) -> bool {
        self.from != self.to && now < self.start + self.duration
    }
    pub fn target_value(self) -> f32 {
        self.to
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_motion_is_continuous_and_finishes() {
        let mut m = Motion::fixed(0.);
        m.target(1., 0., 0.5, [0.38, 1.21, 0.22, 1.]);
        let v = m.value(0.2);
        m.target(0., 0.2, 0.3, [0.05, 0.7, 0.1, 1.]);
        assert!((m.value(0.2) - v).abs() < 0.001);
        assert!(!m.active(1.));
        assert_eq!(m.value(1.), 0.);
    }
    #[test]
    fn intersection_never_has_negative_extent() {
        assert_eq!(
            Rect::new(0., 0., 10., 10.)
                .intersect(Rect::new(20., 20., 3., 3.))
                .w,
            0.
        );
    }
}

/// Coalesces invalidation while waiting for a compositor frame callback.
#[derive(Default)]
pub struct FrameDemand {
    dirty: bool,
    pending: bool,
}
impl FrameDemand {
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    pub fn ready(&mut self) {
        self.pending = false;
    }
    pub fn begin(&mut self) -> bool {
        if !self.dirty || self.pending {
            return false;
        }
        self.dirty = false;
        self.pending = true;
        true
    }
}

/// Compile-time widget registration; the runtime has no built-in desktop widget list.
pub struct WidgetDescriptor {
    pub id: &'static str,
    pub title: &'static str,
    pub size: (f32, f32),
}
pub type WidgetView<C, M> = fn(&C, &ViewContext) -> Element<M>;
pub struct WidgetRegistry<C, M> {
    entries: Vec<(WidgetDescriptor, WidgetView<C, M>)>,
}
impl<C, M> Default for WidgetRegistry<C, M> {
    fn default() -> Self {
        Self { entries: vec![] }
    }
}
impl<C, M> WidgetRegistry<C, M> {
    pub fn register(
        &mut self,
        descriptor: WidgetDescriptor,
        view: WidgetView<C, M>,
    ) -> Result<(), String> {
        if self.entries.iter().any(|(d, _)| d.id == descriptor.id) {
            return Err(format!("Duplicate widget ID: {}", descriptor.id));
        }
        self.entries.push((descriptor, view));
        Ok(())
    }
    pub fn descriptors(&self) -> impl Iterator<Item = &WidgetDescriptor> {
        self.entries.iter().map(|e| &e.0)
    }
    pub fn view(&self, id: &str, client: &C, cx: &ViewContext) -> Option<Element<M>> {
        self.entries
            .iter()
            .find(|(d, _)| d.id == id)
            .map(|(_, view)| view(client, cx))
    }
}
impl<M: Clone + Send + Sync + 'static> Element<M> {
    /// Lift a child's messages into its parent's message type, including input/drag callbacks.
    pub fn map<N: Clone + Send + Sync + 'static>(
        self,
        mapper: impl Fn(M) -> N + Send + Sync + 'static,
    ) -> Element<N> {
        self.map_arc(Arc::new(mapper))
    }
    fn map_arc<N: Clone + Send + Sync + 'static>(
        self,
        mapper: Arc<dyn Fn(M) -> N + Send + Sync>,
    ) -> Element<N> {
        let input = self.input.map(|input| {
            let map = mapper.clone();
            Arc::new(move |text| map(input(text))) as InputCallback<N>
        });
        let drag = self.drag.map(|drag| {
            let map = mapper.clone();
            Arc::new(move |event| map(drag(event))) as DragCallback<N>
        });
        Element {
            id: self.id,
            kind: self.kind,
            style: self.style,
            children: self
                .children
                .into_iter()
                .map(|c| c.map_arc(mapper.clone()))
                .collect(),
            click: self.click.map(|m| mapper(m)),
            hover: self.hover.map(|m| mapper(m)),
            input,
            drag,
            autofocus: self.autofocus,
        }
    }
}

#[cfg(test)]
mod scheduling_tests {
    use super::*;
    #[test]
    fn idle_never_requests_frames() {
        let mut frames = FrameDemand::default();
        assert!(!frames.begin());
        frames.invalidate();
        assert!(frames.begin());
        frames.ready();
        assert!(!frames.begin());
    }
    #[test]
    fn input_during_pending_frame_is_not_lost() {
        let mut frames = FrameDemand::default();
        frames.invalidate();
        assert!(frames.begin());
        frames.invalidate();
        frames.invalidate();
        assert!(!frames.begin());
        frames.ready();
        assert!(frames.begin());
        frames.ready();
        assert!(!frames.begin());
    }
}

#[cfg(test)]
mod composition_tests {
    use super::*;
    #[test]
    fn message_mapping_preserves_nested_input_and_drag() {
        let child = Element::row(vec![
            Element::input("", "", |s| s.len())
                .on_drag(|d| d.dx as usize)
                .on_click(7),
        ]);
        let parent = child.map(|value| format!("child:{value}"));
        let e = &parent.children[0];
        assert_eq!(e.click.as_deref(), Some("child:7"));
        assert_eq!(e.input.as_ref().unwrap()("abc".into()), "child:3");
        assert_eq!(
            e.drag.as_ref().unwrap()(DragEvent {
                dx: 4.,
                dy: 0.,
                finished: true
            }),
            "child:4"
        );
    }
    #[test]
    fn child_effects_preserve_time_redraw_and_typed_async_results() {
        let mut parent = Effects::<String> {
            now: 12.,
            ..Default::default()
        };
        parent.delegate(
            |child| {
                assert_eq!(child.now, 12.);
                child.redraw("panel");
                child.task(|| 42);
            },
            |n| format!("child:{n}"),
        );
        assert!(parent.redraw.contains("panel"));
        assert_eq!(parent.tasks.pop().unwrap()(), "child:42");
    }
    #[test]
    fn registry_rejects_duplicate_stable_ids() {
        let mut registry: WidgetRegistry<(), ()> = WidgetRegistry::default();
        let descriptor = || WidgetDescriptor {
            id: "clock",
            title: "Clock",
            size: (200., 100.),
        };
        registry
            .register(descriptor(), |_, _| Element::text("one"))
            .unwrap();
        assert!(
            registry
                .register(descriptor(), |_, _| Element::text("two"))
                .is_err()
        );
        let cx = ViewContext {
            surface: "test",
            width: 500.,
            height: 500.,
            now: 0.,
        };
        assert!(matches!(registry.view("clock",&(),&cx).unwrap().kind,Kind::Text(s) if s=="one"));
        assert!(registry.view("missing", &(), &cx).is_none());
    }
}
