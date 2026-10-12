//! A small declarative shell API. Public types contain no Wayland or GPU handles.
mod input;
mod scroll;
pub use input::InputEdit;
pub use scroll::{ScrollEvent, ScrollSpec, ScrollStyle};
mod slider;
pub use slider::{SliderEvent, SliderSpec, SliderStyle};
mod clip;
pub use clip::{Clip, RoundedRect};
mod image;
pub use image::{ImageData, ImageError, ImageLevel, reduce_premultiplied};
use std::{
    collections::BTreeSet,
    sync::{Arc, Condvar, Mutex},
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    #[default]
    Fill,
    Contain,
    Cover,
}
/// Presentation parameters in logical pixels. The renderer uses a smooth
/// distance-field falloff outside the shape, truncated at three blur lengths.
#[derive(Clone, Copy, Debug)]
pub struct Shadow {
    pub color: Color,
    pub offset: (f32, f32),
    pub blur: f32,
}
impl Shadow {
    /// Outward support of the renderer's truncated shadow falloff. Layout
    /// policies can retain this space without copying a backend constant.
    /// Call with the validated non-negative blur used for painting.
    pub fn blur_outset(blur: f32) -> f32 {
        blur * 3.
    }
}
impl Default for Shadow {
    fn default() -> Self {
        Self {
            color: Color(0., 0., 0., 0.2),
            offset: (0., 4.),
            blur: 8.,
        }
    }
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
    pub max_lines: Option<usize>,
    pub align: Align,
    pub opacity: f32,
    /// Suppresses this subtree's paint without removing layout or input hits.
    pub paint: bool,
    pub clip: bool,
    pub shadow: Option<Shadow>,
    /// Tint and strength of the hover state layer; transparent disables it.
    pub hover_color: Color,
    pub hover_transition: Transition,
    /// Multiplicative image tint; white preserves original image colors.
    pub image_tint: Color,
    /// How image pixels fit their layout box; cover crops at the box boundary.
    pub image_fit: ImageFit,
    pub focus_color: Color,
    pub focus_width: f32,
    pub focus_outline: bool,
    pub focus_within: bool,
    /// Controlled selection, independent of pointer hover and text-input focus.
    pub selected: bool,
    pub tab_stop: bool,
    pub caret_width: f32,
    pub slider: SliderStyle,
    pub scroll: ScrollStyle,
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
            max_lines: None,
            align: Align::Start,
            opacity: 1.,
            paint: true,
            clip: false,
            shadow: None,
            hover_color: Color::TRANSPARENT,
            hover_transition: Transition::default(),
            image_tint: Color::hex(0xffffff),
            image_fit: ImageFit::Fill,
            focus_color: Color::hex(0xffffff),
            focus_width: 1.,
            focus_outline: true,
            focus_within: false,
            selected: false,
            tab_stop: true,
            caret_width: 1.,
            slider: SliderStyle::default(),
            scroll: ScrollStyle::default(),
        }
    }
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
    MaskedInput { length: usize, placeholder: String },
    Slider(SliderSpec),
    Scroll(ScrollSpec),
    Empty,
}
#[derive(Clone, Copy, Debug)]
pub struct DragEvent {
    pub dx: f32,
    pub dy: f32,
    pub finished: bool,
}
pub type InputCallback<M> = Arc<dyn Fn(String) -> M + Send + Sync>;
pub type EditCallback<M> = Arc<dyn Fn(InputEdit) -> M + Send + Sync>;
pub type ScrollCallback<M> = Arc<dyn Fn(ScrollEvent) -> M + Send + Sync>;
pub type SliderCallback<M> = Arc<dyn Fn(SliderEvent) -> M + Send + Sync>;
pub type DragCallback<M> = Arc<dyn Fn(DragEvent) -> M + Send + Sync>;
#[derive(Clone)]
pub struct Element<M> {
    pub id: String,
    pub kind: Kind,
    pub style: Style,
    pub children: Vec<Element<M>>,
    pub click: Option<M>,
    pub submit: Option<M>,
    pub hover: Option<M>,
    pub input: Option<InputCallback<M>>,
    pub edit: Option<EditCallback<M>>,
    pub drag: Option<DragCallback<M>>,
    pub slider: Option<SliderCallback<M>>,
    pub scroll: Option<ScrollCallback<M>>,
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
            submit: None,
            hover: None,
            input: None,
            edit: None,
            drag: None,
            slider: None,
            scroll: None,
            autofocus: false,
        }
    }
    /// Wrap text at measured glyph advances, limiting overflow with an ellipsis.
    pub fn wrap(mut self, max_lines: usize) -> Self {
        self.style.max_lines = Some(max_lines.max(1));
        self
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
    /// Only the character count enters the element/scene. The component owns
    /// the actual value and applies emitted edit intents, including burst input
    /// before the next frame. This field supports end editing, not selection/IME.
    pub fn masked_input(
        length: usize,
        placeholder: impl Into<String>,
        on_edit: impl Fn(InputEdit) -> M + Send + Sync + 'static,
    ) -> Self {
        let mut e = Self::new(Kind::MaskedInput {
            length: length.min(4096),
            placeholder: placeholder.into(),
        });
        e.edit = Some(Arc::new(on_edit));
        e
    }
    /// Submit a focused text/masked field with Enter; pointer focus never submits.
    pub fn on_submit(mut self, message: M) -> Self {
        self.submit = Some(message);
        self
    }
    /// A controlled scalar input with pointer capture and keyboard adjustment.
    pub fn slider(
        spec: SliderSpec,
        on_change: impl Fn(SliderEvent) -> M + Send + Sync + 'static,
    ) -> Self {
        let mut e = Self::new(Kind::Slider(spec));
        e.slider = Some(Arc::new(on_change));
        e
    }
    /// A single content tree in a bounded vertical viewport. The controlled
    /// offset is clamped by layout; wheel, scrollbar and keyboard reveal emit
    /// ScrollEvent. Give the viewport a fixed/fill height and content an
    /// intrinsic/fixed height. Children keep their own stable focus identities.
    pub fn scroll(
        spec: ScrollSpec,
        content: Self,
        on_change: impl Fn(ScrollEvent) -> M + Send + Sync + 'static,
    ) -> Self {
        let mut e = Self::new(Kind::Scroll(spec));
        e.children = vec![content];
        e.scroll = Some(Arc::new(on_change));
        e.style.clip = true;
        e
    }
    pub fn scroll_style(mut self, style: ScrollStyle) -> Self {
        self.style.scroll = style;
        self
    }
    pub fn slider_style(mut self, style: SliderStyle) -> Self {
        self.style.slider = style;
        self
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
        self.style.image_fit = ImageFit::Contain;
        self
    }
    /// Fill the box without stretching, cropping equally from opposite edges.
    pub fn cover(mut self) -> Self {
        self.style.image_fit = ImageFit::Cover;
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
    /// Retain layout, focus and pointer capture while another surface presents
    /// this content. Unlike zero opacity, input-only nodes keep their hit tree.
    pub fn input_only(mut self) -> Self {
        self.style.paint = false;
        self
    }
    pub fn clip(mut self) -> Self {
        self.style.clip = true;
        self
    }
    pub fn shadow(mut self) -> Self {
        self.style.shadow = Some(Shadow::default());
        self
    }
    pub fn with_shadow(mut self, shadow: Shadow) -> Self {
        self.style.shadow = Some(shadow);
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
    /// Explicit width/height, measured from the output's upper-left corner.
    TopLeft,
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
/// Pointer coverage is independent of the painted tree and keyboard policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerInput {
    /// Empty native input region; pointer events pass to surfaces underneath.
    None,
    /// Only the tree's interactive hit regions capture input.
    Content,
    /// The entire native extent captures input, including outside-click areas.
    All,
}
impl PointerInput {
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Content => "content",
            Self::All => "all",
        }
    }
}
/// Signed logical-pixel margins from anchored output edges. Negative margins
/// are valid protocol requests; actual placement belongs to the compositor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SurfaceMargins {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceSpec {
    pub id: &'static str,
    pub layer: Layer,
    pub anchor: Anchor,
    pub margins: SurfaceMargins,
    /// A client-selected redraw target shared by related native surfaces.
    /// `Effects::redraw` matches either this group or the individual surface ID.
    pub redraw_group: Option<&'static str>,
    pub width: u32,
    pub height: u32,
    pub exclusive_zone: i32,
    pub keyboard: Keyboard,
    pub visible: bool,
    pub pointer_input: PointerInput,
}
impl SurfaceSpec {
    /// Check layer-shell invariants before making native protocol requests.
    /// This is not a renderer extent check or a memory budget.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_empty() || self.id.contains('\0') {
            return Err("A native surface ID must be nonempty and contain no NUL");
        }
        if self.redraw_group.is_some_and(|group| group.is_empty()) {
            return Err("A redraw group must be nonempty");
        }
        let horizontal = matches!(self.anchor, Anchor::Top | Anchor::Bottom | Anchor::Fill);
        let vertical = self.anchor == Anchor::Fill;
        if (self.width == 0 && !horizontal) || (self.height == 0 && !vertical) {
            return Err("An omitted surface dimension requires opposite anchors on that axis");
        }
        Ok(())
    }
    pub fn matches_redraw(&self, target: &str) -> bool {
        self.id == target || self.redraw_group == Some(target)
    }
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
    PageUp,
    PageDown,
    SelectAll,
    ClearInput,
}
#[derive(Clone, Debug)]
pub enum Event {
    /// A successful buffer submission, after view/layout/render. This is not
    /// scanout or a compositor display acknowledgement. `now` is the view's
    /// animation time; dimensions are the submitted logical buffer extent.
    FrameSubmitted {
        surface: &'static str,
        width: f32,
        height: f32,
        now: f64,
    },
    Resize {
        surface: &'static str,
        /// Logical dimensions; this event also fires when integer buffer scale changes.
        width: f32,
        height: f32,
        scale: u32,
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
    /// Translate a scheduling failure into a client message. Unhandled failures
    /// stop the runtime with an error (secure clients retain their lock protocol).
    fn runtime_failure(&self, _failure: RuntimeFailure) -> Option<Self::Message> {
        None
    }
    fn animating(&self, _surface: &str, _now: f64) -> bool {
        false
    }
}

pub type Task<M> = Box<dyn FnOnce() -> M + Send>;
/// Each named queue executes in submission order on an independent worker.
/// Queue names belong to the client; the engine knows no desktop service names.
pub struct ScheduledTask<M> {
    pub queue: &'static str,
    pub cancellation: Cancellation,
    task: Task<M>,
}
impl<M> ScheduledTask<M> {
    /// Execute after admission/cancellation checks; useful for deterministic client tests.
    pub fn run(self) -> M {
        (self.task)()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeFailure {
    pub source: String,
    pub reason: String,
}
impl std::fmt::Display for RuntimeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.source, self.reason)
    }
}
pub struct Effects<M> {
    pub now: f64,
    pub tasks: Vec<ScheduledTask<M>>,
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
        self.task_on("default", task);
    }
    pub fn task_on(&mut self, queue: &'static str, task: impl FnOnce() -> M + Send + 'static) {
        self.task_cancellable_on(queue, Cancellation::default(), task);
    }
    /// Cancellation skips queued work and checks again before result delivery.
    /// Running operations cooperate by checking this token at safe points. Once
    /// delivery starts, cancellation does not retract the result or undo effects;
    /// clients still validate request identity where results can be superseded.
    pub fn task_cancellable_on(
        &mut self,
        queue: &'static str,
        cancellation: Cancellation,
        task: impl FnOnce() -> M + Send + 'static,
    ) {
        self.tasks.push(ScheduledTask {
            queue,
            cancellation,
            task: Box::new(task),
        });
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
            self.task_cancellable_on(task.queue, task.cancellation.clone(), move || {
                map(task.run())
            });
        }
    }
    pub fn quit(&mut self) {
        self.exit = true;
    }
}
#[derive(Clone, Default)]
pub struct Cancellation(Arc<(Mutex<bool>, Condvar)>);
impl Cancellation {
    pub fn cancel(&self) {
        *self.0.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.0.1.notify_all();
    }
    pub fn cancelled(&self) -> bool {
        *self.0.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn sleep(&self, duration: Duration) {
        let state = self.0.0.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self
            .0
            .1
            .wait_timeout_while(state, duration, |cancelled| !*cancelled);
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
    /// Runtime subscription emitters apply bounded backpressure on this producer
    /// thread. Do not hold a lock needed by the UI while sending. Cancellation
    /// releases a waiting sender and discards obsolete subscription messages.
    pub fn send(&self, message: M) {
        (self.0)(message)
    }
}
pub struct Subscription<M> {
    /// Stable identity of the captured inputs; change it to replace a stream.
    pub id: String,
    pub run: Box<dyn FnOnce(Emitter<M>, Cancellation) + Send>,
}
impl<M: Send + 'static> Subscription<M> {
    pub fn stream(
        id: impl Into<String>,
        run: impl FnOnce(Emitter<M>, Cancellation) + Send + 'static,
    ) -> Self {
        Self {
            id: id.into(),
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
        // The inverse-curve search is approximate. Preserve the exact starting
        // value so retargeting cannot introduce a size-dependent discontinuity.
        if now <= self.start {
            return self.from;
        }
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
    fn native_surface_requests_validate_anchor_dimensions_and_redraw_targets() {
        let mut spec = SurfaceSpec {
            id: "card",
            layer: Layer::Bottom,
            anchor: Anchor::TopLeft,
            margins: SurfaceMargins {
                top: 16,
                left: -8,
                ..Default::default()
            },
            redraw_group: Some("cards"),
            width: 320,
            height: 240,
            exclusive_zone: -1,
            keyboard: Keyboard::OnDemand,
            visible: true,
            pointer_input: PointerInput::Content,
        };
        assert!(spec.validate().is_ok());
        assert!(spec.matches_redraw("card"));
        assert!(spec.matches_redraw("cards"));
        assert!(!spec.matches_redraw("other"));
        spec.width = 0;
        assert!(spec.validate().is_err());
        spec.anchor = Anchor::Top;
        assert!(spec.validate().is_ok());
        spec.height = 0;
        assert!(spec.validate().is_err());
        spec.anchor = Anchor::Fill;
        assert!(spec.validate().is_ok());
        spec.id = "";
        assert!(spec.validate().is_err());
        spec.id = "bad\0id";
        assert!(spec.validate().is_err());
        spec.id = "card";
        spec.redraw_group = Some("");
        assert!(spec.validate().is_err());
    }
    #[test]
    fn interrupted_motion_is_continuous_and_finishes() {
        let mut m = Motion::fixed(0.);
        m.target(1., 0., 0.5, [0.38, 1.21, 0.22, 1.]);
        assert_eq!(m.value(-1.), 0.);
        assert_eq!(m.value(0.), 0.);
        let v = m.value(0.2);
        m.target(0., 0.2, 0.3, [0.05, 0.7, 0.1, 1.]);
        assert_eq!(m.value(0.2), v);
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
    deferred: bool,
}
impl FrameDemand {
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    pub fn ready(&mut self) {
        self.pending = false;
    }
    /// No buffer was submitted. Preserve damage but wait for a runtime retry timer.
    pub fn defer(&mut self) {
        self.pending = false;
        self.dirty = true;
        self.deferred = true;
    }
    pub fn retry_ready(&mut self) {
        self.deferred = false;
    }
    pub fn begin(&mut self) -> bool {
        if !self.dirty || self.pending || self.deferred {
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
        let edit = self.edit.map(|callback| {
            let map = mapper.clone();
            Arc::new(move |event| map(callback(event))) as EditCallback<N>
        });
        let drag = self.drag.map(|drag| {
            let map = mapper.clone();
            Arc::new(move |event| map(drag(event))) as DragCallback<N>
        });
        let slider = self.slider.map(|callback| {
            let map = mapper.clone();
            Arc::new(move |event| map(callback(event))) as SliderCallback<N>
        });
        let scroll = self.scroll.map(|f| {
            let map = mapper.clone();
            Arc::new(move |event| map(f(event))) as ScrollCallback<N>
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
            submit: self.submit.map(|m| mapper(m)),
            hover: self.hover.map(|m| mapper(m)),
            input,
            edit,
            drag,
            slider,
            scroll,
            autofocus: self.autofocus,
        }
    }
}

#[cfg(test)]
mod scheduling_tests {
    use super::*;
    #[test]
    fn skipped_frames_wait_for_retry_and_preserve_damage() {
        let mut frames = FrameDemand::default();
        frames.invalidate();
        assert!(frames.begin());
        frames.defer();
        frames.invalidate();
        frames.ready(); // A late compositor callback must not bypass backoff.
        assert!(!frames.begin());
        frames.retry_ready();
        assert!(frames.begin());
        assert!(!frames.begin());
        frames.ready();
        assert!(!frames.begin());
    }
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
                child.task_on("settings", || 42);
            },
            |n| format!("child:{n}"),
        );
        assert!(parent.redraw.contains("panel"));
        let task = parent.tasks.pop().unwrap();
        assert_eq!(task.queue, "settings");
        assert_eq!(task.run(), "child:42");
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
