//! An independent client composing a child component and lifting its typed effects.
use lucent_api::*;
#[derive(Clone)]
enum CounterMessage {
    Increment,
}
#[derive(Default)]
struct Counter {
    count: u32,
    motion: Option<Motion>,
}
impl Component for Counter {
    type Message = CounterMessage;
    fn view(&self, cx: &ViewContext) -> Element<CounterMessage> {
        let width = self.motion.map(|m| m.value(cx.now)).unwrap_or(60.);
        Element::column(vec![
            Element::text(format!("{} clicks · Vulkan · frame callbacks", self.count)),
            Element::empty()
                .size(width, 6.)
                .background(Color::hex(0x73dcee))
                .radius(3.),
            Element::button("Count", CounterMessage::Increment)
                .id("count")
                .background(Color::hex(0x1b6a7d)),
        ])
        .gap(16.)
    }
    fn update(&mut self, _: CounterMessage, effects: &mut Effects<CounterMessage>) {
        self.count += 1;
        self.motion.get_or_insert(Motion::fixed(60.)).target(
            60. + (self.count % 5) as f32 * 50.,
            effects.now,
            0.35,
            [0.38, 1.21, 0.22, 1.],
        );
        effects.redraw("example");
    }
}
#[derive(Clone)]
enum Message {
    Counter(CounterMessage),
    Quit,
}
#[derive(Default)]
struct Example {
    counter: Counter,
}
impl Component for Example {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        Element::column(vec![
            Element::text("Hello, native Wayland").font(23.),
            self.counter.view(cx).map(Message::Counter),
            Element::button("Close", Message::Quit)
                .id("close")
                .background(Color::hex(0x1b6a7d)),
        ])
        .padding(24.)
        .gap(12.)
        .size(420., 270.)
        .background(Color::hex(0x2c5184))
        .radius(28.)
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Counter(message) => effects.delegate(
                |child| self.counter.update(message, child),
                Message::Counter,
            ),
            Message::Quit => effects.quit(),
        }
    }
}
impl Application for Example {
    fn name(&self) -> &'static str {
        "hello-layer"
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "example",
            layer: Layer::Overlay,
            anchor: Anchor::Center,
            width: 420,
            height: 270,
            exclusive_zone: 0,
            keyboard: Keyboard::OnDemand,
            visible: true,
            capture_all: false,
        }]
    }
    fn animating(&self, _: &str, now: f64) -> bool {
        self.counter.motion.is_some_and(|m| m.active(now))
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    lucent_wayland::run(Example::default())
}
