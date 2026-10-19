//! Independent consumer of ordinary international text and runtime composition.
use lucent_api::*;
use lucent_design::{
    Theme,
    component::{input, scroll, scroll_example as token},
    font, radius, space,
};
#[derive(Clone)]
enum Message {
    Single(TextEdit),
    Body(TextEdit),
    Scroll(ScrollEvent),
    Secret(InputEdit),
    Quit,
}
struct Example {
    single: String,
    body: String,
    single_field: TextFieldState,
    body_field: TextFieldState,
    offset: f32,
    error: bool,
}
impl Default for Example {
    fn default() -> Self {
        let single = "Lucent · مرحبا · שלום · नमस्ते · 日本語".to_string();
        let body="English and العربية on one line.\nעברית with English 123.\nहिन्दी क्षि — 日本語 中文 한국어\nUse your compositor input method here.".to_string();
        let mut single_field = TextFieldState::default();
        single_field.reset(&single);
        let mut body_field = TextFieldState::multiline();
        body_field.reset(&body);
        Self {
            single,
            body,
            single_field,
            body_field,
            offset: 0.,
            error: false,
        }
    }
}
impl Component for Example {
    type Message = Message;
    fn view(&self, _: &ViewContext) -> Element<Message> {
        let theme = Theme::new(false);
        theme.apply(
            Element::column(vec![
                Element::text("International text").font(font::TITLE),
                Element::text(
                    "Shared shaping · visual arrows · selection · input-method composition",
                )
                .font(font::LABEL)
                .wrap(2),
                theme
                    .text_input(
                        &self.single,
                        self.single_field.selection(),
                        "Single line",
                        Message::Single,
                    )
                    .id("single")
                    .autofocus(),
                theme
                    .text_area(
                        &self.body,
                        self.body_field.selection(),
                        "Multiline",
                        ScrollSpec::new(self.offset, scroll::LINE_STEP).unwrap(),
                        Message::Body,
                        Message::Scroll,
                    )
                    .id("body"),
                Element::masked_input(0, "IME excluded: count-only control", Message::Secret)
                    .id("masked")
                    .height(Length::Fixed(input::HEIGHT)),
                Element::text(if self.error {
                    "Edit rejected by the client byte limit"
                } else {
                    "The masked probe retains no value. It never grants IME authority."
                })
                .font(font::LABEL)
                .wrap(2),
                theme.button("Close", Message::Quit).id("close"),
            ])
            .fill()
            .padding(space::XL)
            .gap(space::MD)
            .background(theme.surface)
            .radius(radius::PANEL),
        )
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Single(edit) => {
                self.error = self
                    .single_field
                    .update(
                        &mut self.single,
                        edit,
                        8192,
                        ClipboardTarget::new("international", "single"),
                        effects,
                        Message::Single,
                    )
                    .is_err()
            }
            Message::Body(edit) => {
                self.error = self
                    .body_field
                    .update(
                        &mut self.body,
                        edit,
                        8192,
                        ClipboardTarget::new("international", "body"),
                        effects,
                        Message::Body,
                    )
                    .is_err()
            }
            Message::Scroll(event) => self.offset = event.offset,
            Message::Secret(edit) => drop(edit),
            Message::Quit => effects.quit(),
        }
        effects.redraw("international");
    }
}
impl Application for Example {
    fn name(&self) -> &'static str {
        "lucent-international-text"
    }
    fn clipboard_enabled(&self) -> bool {
        true
    }
    fn fonts(&self) -> Vec<&'static [u8]> {
        vec![include_bytes!(
            "../../../apps/lucent-desktop/assets/LucentSans.ttf"
        )]
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "international",
            instance: None,
            output: None,
            layer: Layer::Overlay,
            anchor: Anchor::Top,
            margins: Default::default(),
            redraw_group: None,
            width: token::WIDTH as u32,
            height: token::HEIGHT as u32,
            exclusive_zone: -1,
            keyboard: Keyboard::OnDemand,
            visible: true,
            pointer_input: PointerInput::Full,
        }]
    }
    fn inspect(&self) -> String {
        serde_json::json!({"single":self.single,"body":self.body,"single_caret":self.single_field.selection().caret(),"body_caret":self.body_field.selection().caret(),"error":self.error}).to_string()
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    lucent_wayland::run(Example::default())
}
