//! A native picker client. It returns a value; callers retain action ownership.
//! The component only knows typed entries, selection policy, and the framework API.
use lucent_api::*;
use lucent_design::{
    Theme,
    component::{input, menu as token},
    font, motion, opacity, radius, space,
};
use lucent_domain::{MenuEntry, MenuOutcome};
use lucent_usecases::menu::Selection;
use serde::Deserialize;
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Select,
    Input,
}
#[derive(Deserialize)]
pub struct Request {
    pub prompt: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub entries: Vec<MenuEntry>,
    pub width: Option<f32>,
    pub max_height: Option<f32>,
    #[serde(default)]
    pub light: bool,
    #[serde(default)]
    pub palette: Option<lucent_domain::ThemePalette>,
    #[serde(default)]
    pub back: bool,
}
#[derive(Clone)]
pub enum Message {
    Noop,
    Query(String),
    Navigate(i32),
    Select(usize),
    Choose(usize),
    Accept,
    Cancel,
    Back,
    Resize(f32, f32),
}
/// Read the result after the runtime exits and destroys its keyboard surface.
#[derive(Clone, Default)]
pub struct Completion(Arc<OnceLock<MenuOutcome>>);
impl Completion {
    pub fn outcome(&self) -> Option<&MenuOutcome> {
        self.0.get()
    }
}
pub struct Menu {
    pub request: Request,
    pub selection: Selection,
    pub query: String,
    start: usize,
    viewport: (f32, f32),
    reveal: Motion,
    completion: Completion,
}
impl Menu {
    pub fn new(mut request: Request) -> (Self, Completion) {
        let completion = Completion::default();
        let selection = Selection::new(std::mem::take(&mut request.entries));
        (
            Self {
                request,
                selection,
                query: String::new(),
                start: 0,
                viewport: (1920., 1080.),
                reveal: Motion::fixed(0.),
                completion: completion.clone(),
            },
            completion,
        )
    }
    fn height(&self, viewport: f32) -> f32 {
        let preferred = if self.request.mode == Mode::Input {
            token::INPUT_HEIGHT
        } else {
            self.request.max_height.unwrap_or(token::HEIGHT)
        };
        preferred
            .max(if self.request.mode == Mode::Input {
                token::INPUT_HEIGHT
            } else {
                self.chrome_height() + token::ROW_HEIGHT
            })
            .min((viewport - space::XXL * 2.).max(0.))
    }
    fn chrome_height(&self) -> f32 {
        token::HEADER_HEIGHT
            + input::HEIGHT
            + token::FOOTER_HEIGHT
            + token::PADDING * 2.
            + space::MD * 3.
    }
    fn rows(&self, viewport: f32) -> usize {
        ((self.height(viewport) - self.chrome_height() + space::XS)
            / (token::ROW_HEIGHT + space::XS))
            .floor()
            .max(1.) as usize
    }
    fn finish(&self, outcome: MenuOutcome, effects: &mut Effects<Message>) {
        let _ = self.completion.0.set(outcome);
        effects.quit();
    }
}
impl Component for Menu {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        let t = self
            .request
            .palette
            .as_ref()
            .filter(|p| p.validate().is_ok())
            .map(Theme::from_palette)
            .unwrap_or_else(|| Theme::new(self.request.light));
        let width = self
            .request
            .width
            .unwrap_or(token::WIDTH)
            .max(token::MIN_WIDTH)
            .min((cx.width - space::XXL * 2.).max(0.));
        let height = self.height(cx.height);
        let start = self.selection.start(self.start, self.rows(cx.height));
        let mut header = vec![
            Element::text(&self.request.prompt)
                .font(font::TITLE)
                .color(t.on_surface)
                .width(Length::Fill)
                .wrap(1),
            t.button("Close", Message::Cancel)
                .id("menu-close")
                .tab_stop(false),
        ];
        if self.request.back {
            header.insert(
                0,
                t.button("Back", Message::Back)
                    .id("menu-back")
                    .tab_stop(false),
            );
        }
        let mut children = vec![
            Element::row(header)
                .gap(if self.request.back { space::SM } else { 0. })
                .width(Length::Fill)
                .height(Length::Fixed(token::HEADER_HEIGHT))
                .align(Align::Center),
            Element::input(
                &self.query,
                if self.request.mode == Mode::Input {
                    "Type a value…"
                } else {
                    "Search actions or keys…"
                },
                Message::Query,
            )
            .id("menu-search")
            .autofocus()
            .width(Length::Fill)
            .height(Length::Fixed(input::HEIGHT))
            .padding_xy(input::PADDING_INLINE, input::PADDING_BLOCK)
            .font(font::BODY)
            .color(t.on_surface)
            .background(t.surface_container)
            .radius(radius::CONTROL),
        ];
        if self.request.mode == Mode::Select {
            let rows: Vec<_> = self
                .selection
                .matches
                .iter()
                .enumerate()
                .skip(start)
                .take(self.rows(cx.height))
                .map(|(index, source)| {
                    let entry = &self.selection.entries[*source];
                    let selected = index == self.selection.selected && !entry.disabled;
                    let mut labels = vec![
                        Element::text(&entry.label)
                            .font(font::BODY)
                            .color(t.on_surface)
                            .width(Length::Fill)
                            .wrap(1),
                    ];
                    if !entry.detail.is_empty() {
                        labels.push(
                            Element::text(&entry.detail)
                                .font(font::CAPTION)
                                .color(t.on_surface.alpha(opacity::SUPPORTING))
                                .width(Length::Fill)
                                .wrap(1),
                        );
                    }
                    Element::column(labels)
                        .gap(space::XXS)
                        .width(Length::Fill)
                        .height(Length::Fixed(token::ROW_HEIGHT))
                        .padding_xy(token::ROW_PADDING, space::SM)
                        .radius(radius::CONTROL)
                        .opacity(if entry.disabled {
                            opacity::DISABLED
                        } else {
                            1.
                        })
                        .selected(selected)
                        .background(if selected {
                            t.primary.alpha(opacity::HOVER)
                        } else {
                            Color::TRANSPARENT
                        })
                        .id(format!("menu-row-{index}"))
                        .on_hover(Message::Select(index))
                        .on_click(Message::Choose(index))
                        .tab_stop(false)
                })
                .collect();
            children.push(if rows.is_empty() {
                Element::text("No matching shortcuts or options")
                    .font(font::BODY)
                    .color(t.on_surface)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(space::LG)
                    .wrap(2)
            } else {
                Element::column(rows)
                    .gap(space::XS)
                    .width(Length::Fill)
                    .height(Length::Fill)
            });
        } else {
            children.push(Element::empty().height(Length::Fill));
        }
        children.push(
            Element::row(vec![
                Element::text(if self.request.mode == Mode::Input {
                    "Enter confirm · Esc cancel".into()
                } else {
                    format!(
                        "{} results · Arrows select · Enter run · Esc {}",
                        self.selection.matches.len(),
                        if self.request.back { "back" } else { "close" }
                    )
                })
                .font(font::CAPTION)
                .color(t.on_surface)
                .width(Length::Fill)
                .wrap(1),
            ])
            .width(Length::Fill)
            .height(Length::Fixed(token::FOOTER_HEIGHT)),
        );
        let reveal = self.reveal.value(cx.now);
        let panel = Element::column(children)
            .gap(space::MD)
            .padding(token::PADDING)
            .size(width, height)
            .at(
                (cx.width - width) / 2.,
                (cx.height - height) / 2. + space::LG * (1. - reveal),
            )
            .radius(radius::PANEL)
            .background(t.surface)
            .shadow()
            .opacity(reveal)
            .id("menu-panel")
            .on_click(Message::Noop)
            .tab_stop(false)
            .focus_outline(false);
        t.apply(Element::stack(vec![panel]).fill())
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Noop => {}
            Message::Query(query) => {
                self.query = query.chars().take(4096).collect();
                self.selection.search(&self.query);
                self.start = 0;
            }
            Message::Navigate(delta) => self.selection.navigate(delta),
            Message::Select(index) => {
                self.selection.select(index);
            }
            Message::Choose(index) => {
                if self.selection.select(index) {
                    self.update(Message::Accept, effects);
                }
            }
            Message::Accept => {
                let value = if self.request.mode == Mode::Input {
                    Some(self.query.clone()).filter(|v| !v.is_empty())
                } else {
                    self.selection.choose()
                };
                if let Some(value) = value {
                    self.finish(MenuOutcome::Accepted(value), effects);
                }
            }
            Message::Cancel => self.finish(MenuOutcome::Cancelled, effects),
            Message::Back => self.finish(MenuOutcome::Parent, effects),
            Message::Resize(w, h) => self.viewport = (w, h),
        }
        self.start = self.selection.start(self.start, self.rows(self.viewport.1));
        effects.redraw("menu");
    }
}

impl Application for Menu {
    fn name(&self) -> &'static str {
        "lucent-menu"
    }
    fn fonts(&self) -> Vec<&'static [u8]> {
        vec![include_bytes!("../../lucent-desktop/assets/LucentSans.ttf")]
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "menu",
            layer: Layer::Overlay,
            anchor: Anchor::Fill,
            width: 0,
            height: 0,
            exclusive_zone: -1,
            keyboard: Keyboard::Exclusive,
            visible: true,
            capture_all: true,
        }]
    }
    fn init(&mut self, effects: &mut Effects<Message>) {
        self.reveal
            .target(1., effects.now, motion::ENTER, motion::DECELERATE);
        effects.redraw("menu");
    }
    fn event(&self, event: Event) -> Option<Message> {
        match event {
            Event::Outside { .. } => Some(Message::Cancel),
            Event::Resize { width, height, .. } => Some(Message::Resize(width, height)),
            Event::Scroll { lines, .. } => Some(Message::Navigate(if lines > 0. { 1 } else { -1 })),
            Event::Key { key, .. } => match key {
                Key::Escape => Some(if self.request.back {
                    Message::Back
                } else {
                    Message::Cancel
                }),
                Key::Enter => Some(Message::Accept),
                Key::Down | Key::Tab => Some(Message::Navigate(1)),
                Key::Up | Key::BackTab => Some(Message::Navigate(-1)),
                _ => None,
            },
        }
    }
    fn command(&self, command: &str) -> Result<Option<Message>, String> {
        if command == "close" {
            Ok(Some(Message::Cancel))
        } else {
            Err("Use close or inspect".into())
        }
    }
    fn inspect(&self) -> String {
        // Never expose caller values or input text through diagnostic IPC.
        serde_json::json!({"prompt": self.request.prompt, "entries": self.selection.entries.len(),
            "results": self.selection.matches.len(), "selected": self.selection.selected, "start": self.start}).to_string()
    }
    fn animating(&self, _: &str, now: f64) -> bool {
        self.reveal.active(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_ui::{Interaction, Layout};
    fn request() -> Request {
        Request {
            prompt: "Keybindings".into(),
            mode: Mode::Select,
            entries: (0..228)
                .map(|i| MenuEntry {
                    label: format!("Action {i}"),
                    detail: format!("Super + {i}"),
                    value: format!("original-{i}"),
                    disabled: false,
                })
                .collect(),
            width: Some(800.),
            max_height: Some(500.),
            light: false,
            palette: None,
            back: false,
        }
    }
    #[test]
    fn last_row_is_visible_and_selection_keeps_input_focus() {
        for (width, height) in [(1920., 1080.), (390., 580.), (640., 360.), (640., 288.)] {
            let (mut menu, _) = Menu::new(request());
            let fonts = menu
                .fonts()
                .iter()
                .map(|b| fontdue::Font::from_bytes(*b, Default::default()).unwrap())
                .collect();
            let layout = Layout::new(Arc::new(fonts));
            menu.init(&mut Effects::default());
            menu.update(Message::Resize(width, height), &mut Effects::default());
            menu.update(Message::Navigate(227), &mut Effects::default());
            let tree = menu.view(&ViewContext {
                surface: "menu",
                width,
                height,
                now: 1.,
            });
            let mut input = Interaction::default();
            let scene = layout.build(&tree, width, height, &input, 1.);
            input.synchronize(&scene);
            assert_eq!(input.focus.as_deref(), Some("menu-search"));
            assert!(input.focus_visible);
            let panel = scene
                .hits
                .iter()
                .find(|h| h.id == "menu-panel")
                .unwrap()
                .rect;
            let rows: Vec<_> = scene
                .hits
                .iter()
                .filter(|h| h.id.starts_with("menu-row-"))
                .collect();
            assert!(rows.iter().any(|h| h.id == "menu-row-227"));
            for row in rows {
                assert!(row.rect.y >= panel.y + token::PADDING);
                assert!(
                    row.rect.y + row.rect.h
                        <= panel.y + panel.h - token::PADDING - token::FOOTER_HEIGHT
                );
                assert!(row.rect.x >= panel.x + token::PADDING);
                assert!(row.rect.x + row.rect.w <= panel.x + panel.w - token::PADDING);
            }
        }
    }
    #[test]
    fn selection_returns_original_value_only_after_explicit_accept() {
        let (mut menu, result) = Menu::new(request());
        let mut e = Effects::default();
        menu.update(Message::Query("action 227".into()), &mut e);
        assert_eq!(result.outcome(), None);
        menu.update(Message::Accept, &mut e);
        assert_eq!(
            result.outcome(),
            Some(&MenuOutcome::Accepted("original-227".into()))
        );
        assert!(e.exit);
        assert!(
            e.tasks.is_empty(),
            "Picker must never execute caller commands"
        );
    }
    #[test]
    fn empty_results_do_not_run_an_action_and_escape_cancels() {
        let (mut menu, result) = Menu::new(request());
        let mut e = Effects::default();
        menu.update(Message::Query("missing".into()), &mut e);
        menu.update(Message::Accept, &mut e);
        assert!(!e.exit);
        menu.update(Message::Cancel, &mut e);
        assert_eq!(result.outcome(), Some(&MenuOutcome::Cancelled));
        assert!(e.exit);
    }
    #[test]
    fn input_returns_literal_text_without_disclosing_it_in_inspect() {
        let mut r = request();
        r.mode = Mode::Input;
        let (mut menu, result) = Menu::new(r);
        let value = "literal $(command)";
        menu.update(Message::Query(value.into()), &mut Effects::default());
        assert!(!menu.inspect().contains(value));
        menu.update(Message::Accept, &mut Effects::default());
        assert_eq!(result.outcome(), Some(&MenuOutcome::Accepted(value.into())));
    }
    #[test]
    fn disabled_rows_cannot_activate_and_keyboard_skips_them() {
        let mut r = request();
        r.entries[0].disabled = true;
        r.entries[0].label = "Unavailable fixture".into();
        r.entries[2].disabled = true;
        let (mut menu, result) = Menu::new(r);
        let mut effects = Effects::default();
        assert_eq!(menu.selection.selected, 1);
        menu.update(Message::Choose(0), &mut effects);
        assert!(!effects.exit);
        assert!(result.outcome().is_none());
        menu.update(Message::Navigate(1), &mut effects);
        assert_eq!(menu.selection.selected, 3);
        menu.update(Message::Query("Unavailable fixture".into()), &mut effects);
        menu.update(Message::Accept, &mut effects);
        assert!(!effects.exit);
    }
    #[test]
    fn escape_in_a_submenu_returns_to_parent_without_executing_a_value() {
        let mut r = request();
        r.back = true;
        let (mut menu, result) = Menu::new(r);
        let message = menu
            .event(Event::Key {
                surface: "menu",
                key: Key::Escape,
            })
            .unwrap();
        menu.update(message, &mut Effects::default());
        assert_eq!(result.outcome(), Some(&MenuOutcome::Parent));
    }
}
