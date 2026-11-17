//! Independent ordinary focus consumer; no desktop, domain or service imports.
use lucent_api::*;
use lucent_design::{Theme, component::example, font, radius, space};

const SURFACE: &str = "editor";
const FIELDS: [&str; 2] = ["first", "second"];

#[derive(Clone)]
enum Message {
    Press(PointerPress),
    Focus(u64, KeyboardFocusOutcome),
    Native(NativeSurfaceToken, KeyboardFocusChange),
    Edit(usize, TextEdit),
    Release,
    Quit,
}

struct Request {
    id: u64,
    cancellation: Cancellation,
    native: Option<NativeSurfaceToken>,
}
impl Drop for Request {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

struct Example {
    values: [String; 2],
    fields: [TextFieldState; 2],
    next_request: u64,
    request: Option<Request>,
    status: &'static str,
}
impl Default for Example {
    fn default() -> Self {
        Self {
            values: Default::default(),
            fields: Default::default(),
            next_request: 0,
            request: None,
            status: "Click a field to request keyboard focus",
        }
    }
}

impl Example {
    fn release(&mut self) {
        self.request = None;
        self.status = "Keyboard released; text and selection are retained";
    }
}

impl Component for Example {
    type Message = Message;

    fn view(&self, _: &ViewContext) -> Element<Message> {
        let theme = Theme::new(false);
        let fields: Vec<_> = FIELDS
            .iter()
            .enumerate()
            .map(|(index, id)| {
                theme
                    .text_input(
                        self.values[index].clone(),
                        self.fields[index].selection(),
                        if index == 0 {
                            "First field"
                        } else {
                            "Second field"
                        },
                        move |edit| Message::Edit(index, edit),
                    )
                    .id(*id)
                    .width(Length::Fill)
            })
            .collect();
        theme.apply(
            Element::column(vec![
                Element::text("Ordinary keyboard focus").font(font::TITLE),
                Element::text(
                    "This bottom-layer client starts passive. Click, type, Tab or Escape.",
                )
                .font(font::CAPTION)
                .width(Length::Fill)
                .wrap(2),
                Element::column(fields).gap(space::SM).width(Length::Fill),
                Element::text(self.status)
                    .font(font::CAPTION)
                    .width(Length::Fill)
                    .wrap(2),
                Element::row(vec![
                    theme
                        .button("Release keyboard", Message::Release)
                        .id("release"),
                    theme.button("Close", Message::Quit).id("close"),
                ])
                .gap(space::SM),
            ])
            .padding(space::XXL)
            .gap(space::MD)
            .size(example::WIDTH, example::HEIGHT)
            .background(theme.surface)
            .color(theme.on_surface)
            .radius(radius::PANEL)
            .on_click(Message::Release)
            .tab_stop(false)
            .on_pointer_press(Message::Press)
            .id("focus-panel"),
        )
    }

    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Press(press) if press.button == MouseButton::Primary => {
                self.release();
                if press.keyboard_focusable {
                    if let Some(id) = self.next_request.checked_add(1) {
                        self.next_request = id;
                        let cancellation = Cancellation::default();
                        effects.request_keyboard_focus(
                            SurfaceAddress::new(SURFACE),
                            cancellation.clone(),
                            move |outcome| Message::Focus(id, outcome),
                        );
                        self.request = Some(Request {
                            id,
                            cancellation,
                            native: None,
                        });
                        self.status = "Waiting for the compositor";
                    } else {
                        self.status = "The focus request limit was reached";
                    }
                }
            }
            Message::Press(_) => {}
            Message::Focus(id, outcome) => {
                let Some(request) = self.request.as_mut().filter(|request| request.id == id) else {
                    return;
                };
                match outcome {
                    KeyboardFocusOutcome::Focused(native) => {
                        request.native = Some(native);
                        self.status = "Keyboard focused; Tab moves between fields";
                    }
                    outcome => {
                        self.request = None;
                        self.status = match outcome {
                            KeyboardFocusOutcome::Unavailable => {
                                "Focus requests are unavailable on this compositor"
                            }
                            KeyboardFocusOutcome::Timeout => {
                                "The compositor did not grant focus in time"
                            }
                            KeyboardFocusOutcome::NotRetained => {
                                "The compositor did not retain focus"
                            }
                            _ => "The focus request ended; click a field to try again",
                        };
                    }
                }
            }
            Message::Native(native, change) => {
                if change != KeyboardFocusChange::Entered
                    && self
                        .request
                        .as_ref()
                        .is_some_and(|request| request.native.as_ref() == Some(&native))
                {
                    self.release();
                }
            }
            Message::Edit(index, edit) => {
                if self.fields[index]
                    .update(
                        &mut self.values[index],
                        edit,
                        4096,
                        ClipboardTarget::new(SURFACE, FIELDS[index]),
                        effects,
                        move |edit| Message::Edit(index, edit),
                    )
                    .is_err()
                {
                    self.status = "This field has a 4 KiB edit limit";
                }
            }
            Message::Release => self.release(),
            Message::Quit => effects.quit(),
        }
        effects.redraw(SURFACE);
    }
}

impl Application for Example {
    fn name(&self) -> &'static str {
        "lucent-keyboard-focus"
    }
    fn keyboard_focus_requests_enabled(&self) -> bool {
        true
    }
    fn clipboard_enabled(&self) -> bool {
        true
    }
    fn input_method_enabled(&self) -> bool {
        true
    }
    fn fonts(&self) -> Vec<&'static [u8]> {
        vec![include_bytes!(
            "../../../apps/lucent-desktop/assets/LucentSans.ttf"
        )]
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: SURFACE,
            instance: None,
            output: None,
            layer: Layer::Bottom,
            anchor: Anchor::Center,
            margins: Default::default(),
            redraw_group: None,
            width: example::WIDTH as u32,
            height: example::HEIGHT as u32,
            exclusive_zone: -1,
            keyboard: if self.request.is_some() {
                Keyboard::OnDemand
            } else {
                Keyboard::None
            },
            visible: true,
            pointer_input: PointerInput::Content,
        }]
    }
    fn event(&self, event: Event) -> Option<Message> {
        match event {
            Event::KeyboardFocus {
                surface: SURFACE,
                native,
                change,
            } => Some(Message::Native(native, change)),
            Event::Key {
                surface: SURFACE,
                key: Key::Escape,
                ..
            } => Some(Message::Release),
            _ => None,
        }
    }
    fn command(&self, command: &str) -> Result<Option<Message>, String> {
        match command {
            "quit" => Ok(Some(Message::Quit)),
            _ => Err("Unknown command".into()),
        }
    }
    fn inspect(&self) -> String {
        serde_json::json!({
            "values": self.values,
            "requested": self.request.is_some(),
            "retained": self.request.as_ref().is_some_and(|request| request.native.is_some()),
        })
        .to_string()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!(
            "keyboard-focus: click to focus ordinary fields on a bottom layer; Escape releases focus"
        );
        return Ok(());
    }
    if !args.is_empty() {
        return Err("Unknown argument; use --help".into());
    }
    lucent_wayland::run(Example::default())
}
