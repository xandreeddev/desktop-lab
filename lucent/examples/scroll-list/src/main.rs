//! A controlled list client. No desktop root, OS services or rendering internals.
use lucent_api::*;
use lucent_design::{
    Theme,
    component::{scroll, scroll_example as token},
    font, radius, space,
};

#[derive(Clone)]
enum Message {
    Scroll(ScrollEvent),
    Select(usize),
    Compact,
    Short,
    Toggle,
    Quit,
}
#[derive(Default)]
struct Example {
    offset: f32,
    selected: Option<usize>,
    compact: bool,
    short: bool,
    hidden: bool,
}
impl Example {
    fn height(&self) -> f32 {
        if self.compact {
            token::COMPACT_HEIGHT
        } else {
            token::HEIGHT
        }
    }
    fn count(&self) -> usize {
        if self.short { 2 } else { 32 }
    }
}
impl Component for Example {
    type Message = Message;
    fn view(&self, _: &ViewContext) -> Element<Message> {
        let theme = Theme::new(false);
        let content = Element::column(
            (0..self.count())
                .map(|index| {
                    theme
                        .button(format!("Item {:02}", index + 1), Message::Select(index))
                        .width(Length::Fill)
                        .height(Length::Fixed(token::ROW_HEIGHT))
                        .background(if self.selected == Some(index) {
                            theme.primary
                        } else {
                            theme.surface
                        })
                        .color(if self.selected == Some(index) {
                            theme.on_primary
                        } else {
                            theme.on_surface
                        })
                        .id(format!("item-{index}"))
                })
                .collect(),
        )
        .width(Length::Fill)
        .gap(space::SM);
        let viewport = if self.hidden {
            Element::empty().fill()
        } else {
            theme
                .scroll(
                    ScrollSpec::new(self.offset, scroll::LINE_STEP).expect("controlled offset"),
                    content,
                    Message::Scroll,
                )
                .id("items")
        };
        theme.apply(
            Element::column(vec![
                Element::text("A list, owned by its client").font(font::TITLE),
                Element::text("Wheel · drag the thumb · Tab · Page Down").font(font::LABEL),
                viewport,
                Element::row(vec![
                    theme.button("Resize", Message::Compact).id("resize"),
                    theme.button("Fewer", Message::Short).id("fewer"),
                    theme.button("Hide", Message::Toggle).id("hide"),
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
            Message::Select(index) => self.selected = Some(index),
            Message::Compact => self.compact = !self.compact,
            Message::Short => {
                self.short = !self.short;
                self.offset = 0.;
                self.selected = None;
            }
            Message::Toggle => self.hidden = !self.hidden,
            Message::Quit => effects.quit(),
        }
        effects.redraw("list");
    }
}
impl Application for Example {
    fn name(&self) -> &'static str {
        "scroll-list"
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "list",
            output: None,
            layer: Layer::Overlay,
            anchor: Anchor::Center,
            margins: Default::default(),
            redraw_group: None,
            width: token::WIDTH as u32,
            height: self.height() as u32,
            exclusive_zone: 0,
            keyboard: Keyboard::OnDemand,
            visible: true,
            pointer_input: lucent_api::PointerInput::Content,
        }]
    }
    fn inspect(&self) -> String {
        format!(
            "{{\"offset\":{},\"selected\":{},\"count\":{},\"compact\":{},\"hidden\":{}}}",
            self.offset,
            self.selected.map_or("null".into(), |i| i.to_string()),
            self.count(),
            self.compact,
            self.hidden
        )
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    lucent_wayland::run(Example::default())
}
