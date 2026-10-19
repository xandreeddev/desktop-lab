//! Independent controlled multiline client. No desktop/domain/services imports.
use lucent_api::*;
use lucent_design::{
    Theme,
    component::{scroll, scroll_example as token},
    font, radius, space,
};
#[derive(Clone)]
enum Message {
    Edit(TextEdit),
    Scroll(ScrollEvent),
    Clear,
    Quit,
}
struct Example {
    value: String,
    field: TextFieldState,
    offset: f32,
    error: Option<String>,
}
impl Default for Example {
    fn default() -> Self {
        Self { value: "An ordinary multiline field\n\nClick, type, select and wrap.\nThe component owns this String; the engine owns geometry.\n".into(),
            field: TextFieldState::multiline(), offset: 0., error: None }
    }
}
impl Component for Example {
    type Message = Message;
    fn view(&self, _: &ViewContext) -> Element<Message> {
        let theme = Theme::new(false);
        theme.apply(
            Element::column(vec![
                Element::text("An editor, owned by its client").font(font::TITLE),
                Element::text("Enter · Shift+arrows · Ctrl+C/X/V/Z · Page Down").font(font::LABEL),
                theme
                    .text_area(
                        &self.value,
                        self.field.selection(),
                        "Write something…",
                        ScrollSpec::new(self.offset, scroll::LINE_STEP).expect("controlled offset"),
                        Message::Edit,
                        Message::Scroll,
                    )
                    .id("body")
                    .autofocus(),
                Element::text(
                    self.error
                        .as_deref()
                        .unwrap_or("No persistence or OS service in this example"),
                )
                .font(font::LABEL),
                Element::row(vec![
                    theme.button("Clear", Message::Clear).id("clear"),
                    theme.button("Close", Message::Quit).id("close"),
                ])
                .gap(space::SM),
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
            Message::Scroll(event) => self.offset = event.offset,
            Message::Edit(edit) => {
                self.error = self
                    .field
                    .update(
                        &mut self.value,
                        edit,
                        8192,
                        ClipboardTarget::new("editor", "body"),
                        effects,
                        Message::Edit,
                    )
                    .err()
                    .map(|error| error.to_string())
            }
            Message::Clear => {
                self.value.clear();
                self.field.reset(&self.value);
                self.offset = 0.;
                self.error = None;
            }
            Message::Quit => {
                self.field.cancel();
                effects.quit();
            }
        }
        effects.redraw("editor");
    }
}
impl Application for Example {
    fn name(&self) -> &'static str {
        "text-area"
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "editor",
            instance: None,
            output: None,
            layer: Layer::Overlay,
            anchor: Anchor::Center,
            margins: Default::default(),
            redraw_group: None,
            width: token::WIDTH as u32,
            height: token::HEIGHT as u32,
            exclusive_zone: 0,
            keyboard: Keyboard::OnDemand,
            visible: true,
            pointer_input: PointerInput::Content,
        }]
    }
    fn inspect(&self) -> String {
        format!(
            "{{\"bytes\":{},\"offset\":{},\"error\":{}}}",
            self.value.len(),
            self.offset,
            self.error.is_some()
        )
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    lucent_wayland::run(Example::default())
}
