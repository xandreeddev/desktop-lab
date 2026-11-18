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
    Fit,
    Short,
    Toggle,
    Quit,
}
#[derive(Default)]
struct Example {
    offset: f32,
    selected: Option<usize>,
    compact: bool,
    bounded: bool,
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
    fn natural_limit(&self) -> f32 {
        // This consumer chooses space for three complete rows. The framework
        // measures shorter content and caps taller content without estimating
        // text height in the client.
        token::ROW_HEIGHT * 3. + space::SM * 2. + scroll::PADDING * 2.
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
            let viewport = theme
                .scroll(
                    ScrollSpec::new(self.offset, scroll::LINE_STEP).expect("controlled offset"),
                    content,
                    Message::Scroll,
                )
                .id("items");
            if self.bounded {
                viewport
                    .height(Length::Shrink)
                    .max_height(self.natural_limit())
            } else {
                viewport
            }
        };
        theme.apply(
            Element::column(vec![
                Element::text("A list, owned by its client").font(font::TITLE),
                Element::text("Wheel · drag the thumb · Tab · Page Down").font(font::LABEL),
                viewport,
                Element::row(vec![
                    theme.button("Resize", Message::Compact).id("resize"),
                    theme.button("Fewer", Message::Short).id("fewer"),
                    theme
                        .button(if self.bounded { "Fill" } else { "Fit" }, Message::Fit)
                        .selected(self.bounded)
                        .id("fit"),
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
            Message::Fit => {
                self.bounded = !self.bounded;
                self.offset = 0.;
            }
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
    fn fonts(&self) -> Vec<&'static [u8]> {
        vec![include_bytes!(
            "../../../apps/lucent-desktop/assets/LucentSans.ttf"
        )]
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "list",
            instance: None,
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
            "{{\"offset\":{},\"selected\":{},\"count\":{},\"compact\":{},\"bounded\":{},\"hidden\":{}}}",
            self.offset,
            self.selected.map_or("null".into(), |i| i.to_string()),
            self.count(),
            self.compact,
            self.bounded,
            self.hidden
        )
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    lucent_wayland::run(Example::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_ui::{Interaction, Layout};
    use std::sync::Arc;

    fn context() -> ViewContext {
        ViewContext {
            surface: "list",
            instance: None,
            width: token::WIDTH,
            height: token::COMPACT_HEIGHT,
            now: 0.,
            density: Density::ONE,
        }
    }
    fn activate(app: &mut Example, layout: &Layout, input: &mut Interaction<Message>, id: &str) {
        let cx = context();
        let scene = layout.build(&app.view(&cx), cx.width, cx.height, input, cx.now);
        input.synchronize(&scene);
        input.focus = Some(id.into());
        let messages = input.key(&scene, &Key::Enter);
        assert_eq!(messages.len(), 1, "the visible {id} control must activate");
        for message in messages {
            app.update(message, &mut Effects::default());
        }
    }

    #[test]
    fn independent_client_fits_short_data_and_scrolls_to_click_the_last_long_row() {
        let mut app = Example::default();
        let fonts = app
            .fonts()
            .into_iter()
            .map(|bytes| lucent_ui::text::Font::from_bytes(bytes, Default::default()).unwrap())
            .collect();
        let layout = Layout::new(Arc::new(fonts));
        let mut input = Interaction::default();
        let cx = context();
        activate(&mut app, &layout, &mut input, "fit");
        let scene = layout.build(&app.view(&cx), cx.width, cx.height, &input, cx.now);
        input.synchronize(&scene);
        let list = scene.hits.iter().find(|hit| hit.id == "items").unwrap();
        assert_eq!(list.rect.h, app.natural_limit());
        assert!(list.scroll.as_ref().unwrap().maximum > 0.);
        let long_footer = scene
            .hits
            .iter()
            .find(|hit| hit.id == "fewer")
            .unwrap()
            .rect
            .y;
        input.focus = Some("items".into());
        for message in input.key(&scene, &Key::End) {
            app.update(message, &mut Effects::default());
        }
        let scene = layout.build(&app.view(&cx), cx.width, cx.height, &input, cx.now);
        input.synchronize(&scene);
        let last = scene.hits.iter().find(|hit| hit.id == "item-31").unwrap();
        assert_eq!(last.rect.intersect(last.clip.bounds).h, last.rect.h);
        let (x, y) = (
            last.rect.x + last.rect.w / 2.,
            last.rect.y + last.rect.h / 2.,
        );
        input.press(&scene, x, y);
        let click = input
            .release(x, y)
            .expect("last row is clickable after End");
        app.update(click, &mut Effects::default());
        assert_eq!(app.selected, Some(31));

        activate(&mut app, &layout, &mut input, "fewer");
        let scene = layout.build(&app.view(&cx), cx.width, cx.height, &input, cx.now);
        let list = scene.hits.iter().find(|hit| hit.id == "items").unwrap();
        assert_eq!(
            list.rect.h,
            token::ROW_HEIGHT * 2. + space::SM + scroll::PADDING * 2.
        );
        assert!(list.rect.h < app.natural_limit());
        assert_eq!(list.scroll.as_ref().unwrap().maximum, 0.);
        assert_eq!(list.scroll.as_ref().unwrap().offset, 0.);
        assert!(
            scene
                .hits
                .iter()
                .find(|hit| hit.id == "fewer")
                .unwrap()
                .rect
                .y
                < long_footer
        );
        assert_eq!(app.selected, None);
    }
}
