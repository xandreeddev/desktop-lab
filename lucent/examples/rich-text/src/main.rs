//! Independent consumer: inline links dispatch typed messages, never OS calls.
use lucent_api::*;
use lucent_design::{Theme, component, font, radius, space};
#[derive(Clone)]
enum Message {
    Selected(String),
    Quit,
}
#[derive(Default)]
struct Example {
    selected: String,
}
impl Component for Example {
    type Message = Message;
    fn view(&self, _: &ViewContext) -> Element<Message> {
        let theme = Theme::new(false);
        theme.apply(Element::column(vec![
            Element::text("Inline rich text").font(font::TITLE).color(theme.on_surface),
            Element::rich_text(vec![TextSpan::new("One reusable flow supports "),TextSpan::new("bold").face(1),TextSpan::new(", "),TextSpan::new("italic").face(2),TextSpan::new(" and "),TextSpan::new("both").face(3),TextSpan::new(" faces.\nA "),TextSpan::new("long link wraps naturally across several lines while remaining one keyboard focus target").link("details").color(theme.info).underline(true),TextSpan::new(". More ordinary text follows.")])
                .on_link(|id|Message::Selected(id.into())).id("body").autofocus().font(font::BODY).color(theme.on_surface).width(Length::Fill).wrap(20),
            Element::text(if self.selected.is_empty(){"Tab to the link · Enter selects · Click any wrapped fragment".into()}else{format!("Selected client action: {}",self.selected)}).font(font::CAPTION).color(theme.on_surface).width(Length::Fill).wrap(2),
            theme.button("Close",Message::Quit).id("close"),
        ]).padding(space::XXL).gap(space::MD).size(component::example::WIDTH,component::example::HEIGHT).background(theme.surface).color(theme.on_surface).radius(radius::PANEL))
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Selected(id) => {
                self.selected = id;
                effects.redraw("rich-text");
            }
            Message::Quit => effects.quit(),
        }
    }
}
impl Application for Example {
    fn name(&self) -> &'static str {
        "lucent-rich-text"
    }
    fn fonts(&self) -> Vec<&'static [u8]> {
        vec![
            include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf"),
            include_bytes!("../../../apps/lucent-desktop/assets/LucentBold.ttf"),
            include_bytes!("../../../apps/lucent-desktop/assets/LucentItalic.ttf"),
            include_bytes!("../../../apps/lucent-desktop/assets/LucentBoldItalic.ttf"),
        ]
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "rich-text",
            output: None,
            layer: Layer::Top,
            anchor: Anchor::Top,
            margins: Default::default(),
            redraw_group: None,
            width: component::example::WIDTH as u32,
            height: component::example::HEIGHT as u32,
            exclusive_zone: -1,
            keyboard: Keyboard::Exclusive,
            pointer_input: PointerInput::Content,
            visible: true,
        }]
    }
    fn event(&self, event: Event) -> Option<Message> {
        matches!(
            event,
            Event::Key {
                key: Key::Escape,
                ..
            }
        )
        .then_some(Message::Quit)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--help") {
        println!("lucent-rich-text: independent Wayland inline formatting/link example");
        return Ok(());
    }
    lucent_wayland::run(Example::default())
}
