//! An independent client composing a child component and lifting its typed effects.
use lucent_api::*;
use lucent_design::{Theme, component::example, font, motion, radius, space};
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
                .background(Theme::new(false).primary)
                .radius(radius::INDICATOR),
            Theme::new(false)
                .button("Count", CounterMessage::Increment)
                .id("count")
                .background(Theme::new(false).surface_container),
        ])
        .gap(space::LG)
    }
    fn update(&mut self, _: CounterMessage, effects: &mut Effects<CounterMessage>) {
        self.count += 1;
        self.motion.get_or_insert(Motion::fixed(60.)).target(
            60. + (self.count % 5) as f32 * 50.,
            effects.now,
            motion::SELECTION,
            motion::SPATIAL,
        );
        effects.redraw("example");
    }
}
#[derive(Clone)]
enum Message {
    Counter(CounterMessage),
    Level(SliderEvent),
    Outputs(Vec<DisplayOutput>),
    Quit,
}
#[derive(Default)]
struct Example {
    counter: Counter,
    level: f32,
    output: Option<OutputName>,
    outputs: Vec<DisplayOutput>,
}
impl Component for Example {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        let theme = Theme::new(false);
        theme.apply(
            Element::column(vec![
                Element::text("Hello, native Wayland").font(font::TITLE),
                Element::text(format!(
                    "Output target: {} · {} available",
                    self.output
                        .as_ref()
                        .map_or("compositor choice", OutputName::as_str),
                    self.outputs.len()
                ))
                .font(font::CAPTION)
                .wrap(2),
                self.counter.view(cx).map(Message::Counter),
                Element::text(format!("Demo level: {:.0}%", self.level * 100.)).font(font::BODY),
                theme
                    .slider(
                        SliderSpec::new(self.level, 0.05).expect("controlled normalized value"),
                        Message::Level,
                    )
                    .id("demo-level"),
                Theme::new(false)
                    .button("Close", Message::Quit)
                    .id("close")
                    .background(Theme::new(false).surface_container),
            ])
            .padding(space::XXL)
            .gap(space::MD)
            .size(example::WIDTH, example::HEIGHT)
            .background(Theme::new(false).surface)
            .radius(radius::PANEL),
        )
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Counter(message) => effects.delegate(
                |child| self.counter.update(message, child),
                Message::Counter,
            ),
            Message::Level(event) => {
                self.level = event.value;
                effects.redraw("example");
            }
            Message::Outputs(outputs) => {
                self.outputs = outputs;
                effects.redraw("example");
            }
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
            instance: None,
            output: self.output.clone(),
            layer: Layer::Overlay,
            anchor: Anchor::Center,
            margins: Default::default(),
            redraw_group: None,
            width: example::WIDTH as u32,
            height: example::HEIGHT as u32,
            exclusive_zone: 0,
            keyboard: Keyboard::OnDemand,
            visible: true,
            pointer_input: lucent_api::PointerInput::Content,
        }]
    }
    fn event(&self, event: Event) -> Option<Message> {
        if let Event::OutputsChanged(outputs) = event {
            Some(Message::Outputs(outputs))
        } else {
            None
        }
    }
    fn animating(&self, _: &str, now: f64) -> bool {
        self.counter.motion.is_some_and(|m| m.active(now))
    }
    fn inspect(&self) -> String {
        format!(
            "{{\"level\":{},\"count\":{}}}",
            self.level, self.counter.count
        )
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = Example::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => {
                println!(
                    "hello-layer [--output NAME]: a named target waits when disconnected; omitted target lets the compositor choose"
                );
                return Ok(());
            }
            "--output" => {
                app.output = Some(OutputName::new(
                    args.next().ok_or("--output requires a name")?,
                )?)
            }
            _ => return Err("Unknown argument; use --help".into()),
        }
    }
    lucent_wayland::run(app)
}
