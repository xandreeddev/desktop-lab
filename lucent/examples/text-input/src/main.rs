//! Independent declarative editing client; no desktop-client/service dependency.
use lucent_api::*;
use lucent_design::{Theme, component, font, radius, space};
#[derive(Clone)]
enum Message {
    Edit(TextEdit),
    Quit,
}
struct Example {
    value: String,
    field: TextFieldState,
    error: bool,
}
impl Default for Example {
    fn default() -> Self {
        let value = "Edit a word or café".to_string();
        let mut field = TextFieldState::default();
        field.reset(&value);
        Self {
            value,
            field,
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
                Element::text("Text input").font(font::TITLE),
                Element::text("Arrows move · Shift selects · Ctrl moves by word")
                    .font(font::CAPTION)
                    .wrap(2),
                Element::text_input(
                    &self.value,
                    self.field.selection(),
                    "Type a value",
                    Message::Edit,
                )
                .id("editor")
                .autofocus()
                .width(Length::Fill)
                .height(Length::Fixed(component::input::HEIGHT))
                .padding_xy(
                    component::input::PADDING_INLINE,
                    component::input::PADDING_BLOCK,
                )
                .font(font::BODY)
                .background(theme.surface_container)
                .radius(radius::CONTROL),
                Element::text(if self.error {
                    "This field has a 4 KiB edit limit"
                } else {
                    "Click and drag to select · Backspace/Delete remove graphemes"
                })
                .font(font::CAPTION)
                .wrap(2),
                theme.button("Close", Message::Quit).id("close"),
            ])
            .padding(space::XXL)
            .gap(space::MD)
            .size(component::example::WIDTH, component::example::HEIGHT)
            .background(theme.surface)
            .color(theme.on_surface)
            .radius(radius::PANEL),
        )
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Edit(edit) => {
                self.error = self
                    .field
                    .update(
                        &mut self.value,
                        edit,
                        4096,
                        ClipboardTarget::new("text-input", "editor"),
                        effects,
                        Message::Edit,
                    )
                    .is_err();
                effects.redraw("text-input");
            }
            Message::Quit => effects.quit(),
        }
    }
}
impl Application for Example {
    fn input_method_enabled(&self) -> bool {
        true
    }

    fn clipboard_enabled(&self) -> bool {
        true
    }
    fn name(&self) -> &'static str {
        "lucent-text-input"
    }
    fn fonts(&self) -> Vec<&'static [u8]> {
        vec![include_bytes!(
            "../../../apps/lucent-desktop/assets/LucentSans.ttf"
        )]
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "text-input",
            instance: None,
            output: None,
            layer: Layer::Top,
            anchor: Anchor::Top,
            margins: Default::default(),
            redraw_group: None,
            width: component::example::WIDTH as u32,
            height: component::example::HEIGHT as u32,
            exclusive_zone: -1,
            keyboard: Keyboard::OnDemand,
            visible: true,
            pointer_input: PointerInput::Content,
        }]
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    lucent_wayland::run(Example::default())
}
