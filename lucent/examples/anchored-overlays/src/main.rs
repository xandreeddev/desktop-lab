//! An independent framework client: no desktop/service/domain imports or IO in views.
use lucent_api::*;
use lucent_design::{Theme, component, font, motion, radius, space};
#[derive(Clone)]
enum Message {
    Toggle,
    Close,
    Choose(&'static str),
    Quit,
}
#[derive(Default)]
struct Example {
    open: bool,
    selected: Option<&'static str>,
    reveal: Option<Motion>,
}
impl Component for Example {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        let theme = Theme::new(false);
        let mut root = Element::column(vec![
            Element::text("Anchored overlays").font(font::TITLE),
            Element::text(
                "Hover or Tab for a label. Open the menu, then use Tab, Shift+Tab or Escape.",
            )
            .font(font::BODY)
            .width(Length::Fill)
            .wrap(3),
            theme
                .button("Choose a value", Message::Toggle)
                .id("trigger")
                .autofocus()
                .tooltip(theme.tooltip("This label takes no input or focus", OverlaySide::Below)),
            Element::text(self.selected.unwrap_or("No selection yet")).font(font::BODY),
            theme.button("Close example", Message::Quit).id("quit"),
        ])
        .padding(space::XXL)
        .gap(space::MD)
        .fill()
        .background(theme.surface)
        .color(theme.on_surface)
        .radius(radius::PANEL);
        if self.open {
            let content = theme
                .popover(
                    Element::column(vec![
                        theme
                            .button("First value", Message::Choose("First value"))
                            .id("first"),
                        theme
                            .button("Second value", Message::Choose("Second value"))
                            .id("second"),
                        theme.button("Cancel", Message::Close).id("cancel"),
                    ])
                    .gap(space::SM)
                    .width(Length::Fill),
                )
                .opacity(self.reveal.map_or(1., |motion| motion.value(cx.now)));
            root = root.popover(Popover {
                id: "choices".into(),
                anchor: "trigger".into(),
                placement: AnchoredSpec {
                    side: OverlaySide::Below,
                    align: OverlayAlign::Start,
                    gap: component::popover::GAP,
                    inset: component::popover::INSET,
                },
                content: Box::new(content),
                dismiss: Message::Close,
            });
        }
        theme.apply(root)
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Toggle => {
                self.open = !self.open;
                if self.open {
                    let mut reveal = Motion::fixed(0.);
                    reveal.target(1., effects.now, motion::ENTER, motion::DECELERATE);
                    self.reveal = Some(reveal);
                }
            }
            Message::Close => self.open = false,
            Message::Choose(value) => {
                self.selected = Some(value);
                self.open = false;
            }
            Message::Quit => effects.quit(),
        }
        effects.redraw("example");
    }
}
impl Application for Example {
    fn name(&self) -> &'static str {
        "anchored-overlays"
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "example",
            instance: None,
            output: None,
            layer: Layer::Overlay,
            anchor: Anchor::Center,
            margins: Default::default(),
            redraw_group: None,
            width: component::example::WIDTH as u32,
            height: component::example::HEIGHT as u32,
            exclusive_zone: 0,
            keyboard: Keyboard::OnDemand,
            visible: true,
            pointer_input: PointerInput::Content,
        }]
    }
    fn event(&self, event: Event) -> Option<Message> {
        // The runtime offers Escape to an open popover before this global route.
        matches!(
            event,
            Event::Key {
                key: Key::Escape,
                ..
            }
        )
        .then_some(Message::Quit)
    }
    fn animating(&self, _: &str, now: f64) -> bool {
        self.open && self.reveal.is_some_and(|motion| motion.active(now))
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--help") {
        println!("anchored-overlays: passive tooltips and one actionable surface-local popover");
        return Ok(());
    }
    lucent_wayland::run(Example::default())
}
