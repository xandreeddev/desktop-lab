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
    Edit(TextEdit),
    Noop,
    Query(String),
    Navigate(i32),
    ListNavigate(ListNavigation),
    ScrollResults(u64, ScrollEvent),
    SelectEntry(usize),
    ChooseEntry(usize),
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
    query_field: TextFieldState,
    edit_error: String,
    start: usize,
    query_revision: u64,
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
                query_field: TextFieldState::default(),
                edit_error: String::new(),
                start: 0,
                query_revision: 0,
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
        let start = list_window(
            self.selection.selected,
            self.start,
            self.selection.matches.len(),
            self.rows(cx.height),
        )
        .start;
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
            t.text_input(
                &self.query,
                self.query_field.selection(),
                if self.request.mode == Mode::Input {
                    "Type a value…"
                } else {
                    "Search actions or keys…"
                },
                Message::Edit,
            )
            .id("menu-search")
            .autofocus(),
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
                        .id(format!("menu-row-{source}"))
                        .on_hover(Message::SelectEntry(*source))
                        .on_click(Message::ChooseEntry(*source))
                        .hover_feedback(!entry.disabled)
                        .tab_stop(false)
                        .at(0., index as f32 * (token::ROW_HEIGHT + space::XS))
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
                let pitch = token::ROW_HEIGHT + space::XS;
                let revision = self.query_revision;
                let count = self.selection.matches.len();
                let visible = self.rows(cx.height);
                let viewport = Element::scroll(
                    ScrollSpec::new(start as f32 * pitch, pitch).unwrap(),
                    Element::stack(rows)
                        .width(Length::Fill)
                        .height(Length::Fixed(count as f32 * pitch - space::XS)),
                    move |event| Message::ScrollResults(revision, event),
                )
                .scroll_style(t.scroll_style())
                .width(Length::Fill)
                .height(Length::Fixed(visible as f32 * pitch - space::XS))
                .tab_stop(false)
                .focus_outline(false)
                .hover_feedback(false)
                .id(format!("menu-results-{revision}"));
                Element::stack(vec![viewport])
                    .width(Length::Fill)
                    .height(Length::Fill)
            });
        } else {
            children.push(Element::empty().height(Length::Fill));
        }
        children.push(
            Element::row(vec![
                Element::text(if !self.edit_error.is_empty() {
                    self.edit_error.clone()
                } else if self.request.mode == Mode::Input {
                    "Enter confirm · Esc cancel".into()
                } else {
                    format!(
                        "{} results · {} · Enter · Esc {}",
                        self.selection.matches.len(),
                        if width >= token::WIDTH {
                            "Arrows select · PgUp/PgDn page"
                        } else {
                            "PgUp/PgDn"
                        },
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
            .focus_outline(false)
            .hover_feedback(false);
        t.apply(Element::stack(vec![panel]).fill())
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Noop => {}
            Message::Query(query) => {
                self.query_revision = self.query_revision.wrapping_add(1);
                self.query = query.chars().take(4096).collect();
                self.query_field.reset(&self.query);
                self.edit_error.clear();
                self.selection.search(&self.query);
                self.start = 0;
            }
            Message::Edit(edit) => {
                match self.query_field.update(
                    &mut self.query,
                    edit,
                    4096,
                    ClipboardTarget::new("menu", "menu-search"),
                    effects,
                    Message::Edit,
                ) {
                    Ok(change) => {
                        self.edit_error.clear();
                        if change.content {
                            self.query_revision = self.query_revision.wrapping_add(1);
                            self.selection.search(&self.query);
                            self.start = 0;
                        }
                    }
                    Err(error) => self.edit_error = error.to_string(),
                }
            }
            Message::Navigate(delta) => self.selection.navigate(delta),
            Message::ListNavigate(navigation) => {
                if let Some(index) = navigation.target(
                    self.selection.selected,
                    self.selection.matches.len(),
                    self.rows(self.viewport.1),
                    |index| !self.selection.entries[self.selection.matches[index]].disabled,
                ) {
                    self.selection.select(index);
                }
            }
            Message::ScrollResults(revision, event) => {
                if revision == self.query_revision
                    && self.request.mode == Mode::Select
                    && event.offset.is_finite()
                    && event.offset >= 0.
                {
                    let page = self.rows(self.viewport.1);
                    let first = (event.offset / (token::ROW_HEIGHT + space::XS)).round() as usize;
                    let first = first.min(self.selection.matches.len().saturating_sub(page));
                    let relative = self
                        .selection
                        .selected
                        .saturating_sub(self.start)
                        .min(page.saturating_sub(1));
                    self.start = first;
                    self.update(
                        Message::ListNavigate(ListNavigation::Select(
                            first.saturating_add(relative),
                        )),
                        effects,
                    );
                }
            }
            Message::SelectEntry(source) | Message::ChooseEntry(source) => {
                if let Some(index) = self
                    .selection
                    .matches
                    .iter()
                    .position(|entry| *entry == source)
                    && self.selection.select(index)
                    && matches!(message, Message::ChooseEntry(_))
                {
                    self.update(Message::Accept, effects);
                }
            }
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
        self.start = list_window(
            self.selection.selected,
            self.start,
            self.selection.matches.len(),
            self.rows(self.viewport.1),
        )
        .start;
        effects.redraw("menu");
    }
}

impl Application for Menu {
    fn input_method_enabled(&self) -> bool {
        true
    }

    fn clipboard_enabled(&self) -> bool {
        true
    }
    fn name(&self) -> &'static str {
        "lucent-menu"
    }
    fn fonts(&self) -> Vec<&'static [u8]> {
        vec![include_bytes!("../../lucent-desktop/assets/LucentSans.ttf")]
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "menu",
            instance: None,
            output: None,
            layer: Layer::Overlay,
            anchor: Anchor::Fill,
            margins: Default::default(),
            redraw_group: None,
            width: 0,
            height: 0,
            exclusive_zone: -1,
            keyboard: Keyboard::Exclusive,
            visible: true,
            pointer_input: lucent_api::PointerInput::All,
        }]
    }
    fn init(&mut self, effects: &mut Effects<Message>) {
        self.reveal
            .target(1., effects.now, motion::ENTER, motion::DECELERATE);
        effects.redraw("menu");
    }
    fn event(&self, event: Event) -> Option<Message> {
        match event {
            Event::OutputsChanged(_) | Event::Scoped { .. } => None,
            Event::PointerInside { .. } | Event::PointerButton { .. } => None,
            Event::FrameSubmitted { .. } | Event::KeyboardFocus { .. } => None,
            Event::Outside { .. } => Some(Message::Cancel),
            Event::Resize { width, height, .. } => Some(Message::Resize(width, height)),
            Event::Scroll { lines, .. } if lines.is_finite() && lines != 0. => {
                Some(Message::Navigate(if lines > 0. { 1 } else { -1 }))
            }
            Event::Scroll { .. } => None,
            Event::Key { key, .. } => match key {
                Key::Escape => Some(if self.request.back {
                    Message::Back
                } else {
                    Message::Cancel
                }),
                Key::Enter => Some(Message::Accept),
                Key::Tab => Some(Message::ListNavigate(ListNavigation::Next)),
                Key::BackTab => Some(Message::ListNavigate(ListNavigation::Previous)),
                key if self.request.mode == Mode::Select
                    && !matches!(key, Key::Home | Key::End) =>
                {
                    ListNavigation::from_key(&key).map(Message::ListNavigate)
                }
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
    fn clipboard_keeps_original_menu_values_and_surfaces_transfer_errors() {
        let (mut menu, completion) = Menu::new(request());
        let mut effects = Effects::default();
        menu.update(Message::Edit(TextEdit::Paste), &mut effects);
        let request = effects.clipboard.pop().unwrap();
        assert_eq!(request.target, ClipboardTarget::new("menu", "menu-search"));
        menu.update(
            request.finish(ClipboardOutcome::Text("Action 227".into())),
            &mut effects,
        );
        assert_eq!(menu.selection.matches.len(), 1);
        menu.update(Message::Accept, &mut effects);
        assert!(
            matches!(completion.outcome(),Some(MenuOutcome::Accepted(value)) if value=="original-227")
        );
        menu.update(Message::Edit(TextEdit::Paste), &mut effects);
        let request = effects.clipboard.pop().unwrap();
        menu.update(
            request.finish(ClipboardOutcome::Error(ClipboardError::Timeout)),
            &mut effects,
        );
        assert_eq!(menu.query, "Action 227");
        assert_eq!(menu.edit_error, "Clipboard transfer timed out");
    }
    #[test]
    fn page_navigation_uses_visible_rows_skips_disabled_and_never_accepts() {
        let (mut menu, completion) = Menu::new(request());
        let page = menu.rows(menu.viewport.1);
        menu.selection.entries[page].disabled = true;
        menu.selection.entries[page + 1].disabled = true;
        let message = menu
            .event(Event::Key {
                surface: "menu",
                key: Key::PageDown,
            })
            .unwrap();
        menu.update(message, &mut Effects::default());
        assert_eq!(menu.selection.selected, page + 2);
        assert!(
            list_window(
                menu.selection.selected,
                menu.start,
                menu.selection.matches.len(),
                page
            )
            .contains(&menu.selection.selected)
        );
        assert!(completion.outcome().is_none());
        menu.update(
            Message::ListNavigate(ListNavigation::Last),
            &mut Effects::default(),
        );
        assert_eq!(menu.selection.selected, 227);
        let message = menu
            .event(Event::Key {
                surface: "menu",
                key: Key::PageUp,
            })
            .unwrap();
        menu.update(message, &mut Effects::default());
        assert_eq!(menu.selection.selected, 227 - page);
        menu.update(
            Message::Query("no such result".into()),
            &mut Effects::default(),
        );
        menu.update(
            Message::ListNavigate(ListNavigation::PageNext),
            &mut Effects::default(),
        );
        menu.update(Message::Accept, &mut Effects::default());
        assert_eq!(menu.start, 0);
        assert!(completion.outcome().is_none());
        for key in [Key::Home, Key::End] {
            assert!(
                menu.event(Event::Key {
                    surface: "menu",
                    key
                })
                .is_none(),
                "editor retains Home/End"
            );
        }
        for lines in [0., f64::NAN, f64::INFINITY] {
            assert!(
                menu.event(Event::Scroll {
                    surface: "menu",
                    lines
                })
                .is_none()
            );
        }
    }
    #[test]
    fn captured_entry_and_scroll_messages_cannot_target_a_replacement_search() {
        let (mut menu, completion) = Menu::new(request());
        let revision = menu.query_revision;
        menu.update(
            Message::ScrollResults(
                revision,
                ScrollEvent {
                    offset: 12. * (token::ROW_HEIGHT + space::XS),
                },
            ),
            &mut Effects::default(),
        );
        assert_eq!((menu.start, menu.selection.selected), (12, 12));
        menu.update(Message::Query("Action 227".into()), &mut Effects::default());
        menu.update(
            Message::ScrollResults(revision, ScrollEvent { offset: 0. }),
            &mut Effects::default(),
        );
        menu.update(Message::ChooseEntry(0), &mut Effects::default());
        assert_eq!(menu.start, 0);
        assert!(
            completion.outcome().is_none(),
            "retired source index cannot accept the new rank-zero result"
        );
        menu.update(Message::ChooseEntry(227), &mut Effects::default());
        assert!(
            matches!(completion.outcome(), Some(MenuOutcome::Accepted(value)) if value == "original-227")
        );
    }
    #[test]
    fn last_row_is_visible_and_selection_keeps_input_focus() {
        for (width, height) in [(1920., 1080.), (390., 580.), (640., 360.), (640., 288.)] {
            let (mut menu, _) = Menu::new(request());
            let fonts = menu
                .fonts()
                .iter()
                .map(|b| lucent_ui::text::Font::from_bytes(*b, Default::default()).unwrap())
                .collect();
            let layout = Layout::new(Arc::new(fonts));
            menu.init(&mut Effects::default());
            menu.update(Message::Resize(width, height), &mut Effects::default());
            menu.update(Message::Navigate(227), &mut Effects::default());
            let tree = menu.view(&ViewContext {
                surface: "menu",
                instance: None,
                width,
                height,
                now: 1.,
                density: lucent_api::Density::ONE,
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
    fn shared_editor_updates_filtering_and_returns_literal_input_after_burst_edits() {
        let (mut menu, result) = Menu::new(request());
        menu.request.mode = Mode::Input;
        menu.update(Message::Query("café".into()), &mut Effects::default());
        for edit in [
            TextEdit::Move {
                movement: TextMove::Start,
                extend: false,
            },
            TextEdit::Insert("new ".into()),
            TextEdit::Insert("value ".into()),
        ] {
            menu.update(Message::Edit(edit), &mut Effects::default());
        }
        assert_eq!(menu.query, "new value café");
        menu.update(Message::Accept, &mut Effects::default());
        assert_eq!(
            result.outcome(),
            Some(&MenuOutcome::Accepted("new value café".into()))
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
    fn unavailable_menu_row_keeps_its_paint_and_selection_when_hovered() {
        let mut r = request();
        r.entries[0].disabled = true;
        let (mut menu, completion) = Menu::new(r);
        menu.init(&mut Effects::default());
        let fonts = menu
            .fonts()
            .into_iter()
            .map(|bytes| lucent_ui::text::Font::from_bytes(bytes, Default::default()).unwrap())
            .collect();
        let layout = Layout::new(Arc::new(fonts));
        let cx = ViewContext {
            surface: "menu",
            instance: None,
            width: 640.,
            height: 580.,
            now: 1.,
            density: Density::ONE,
        };
        menu.update(
            Message::Resize(cx.width, cx.height),
            &mut Effects::default(),
        );
        let mut input = Interaction::default();
        let scene = layout.build(&menu.view(&cx), cx.width, cx.height, &input, cx.now);
        let row = scene
            .hits
            .iter()
            .find(|hit| hit.id == "menu-row-0")
            .unwrap();
        let selected = menu.selection.selected;
        let mut effects = Effects::default();
        for message in input.motion(
            &scene,
            row.rect.x + row.rect.w / 2.,
            row.rect.y + row.rect.h / 2.,
            0.,
        ) {
            menu.update(message, &mut effects);
        }
        let hovered = layout.build(&menu.view(&cx), cx.width, cx.height, &input, cx.now);
        assert_eq!(format!("{:?}", scene.paint), format!("{:?}", hovered.paint));
        assert_eq!(menu.selection.selected, selected);
        assert!(!effects.exit);
        assert!(completion.outcome().is_none());
    }
    #[test]
    fn passive_menu_padding_does_not_tint_the_panel_but_enabled_rows_still_hover() {
        for light in [false, true] {
            let mut request = request();
            request.light = light;
            let (mut menu, completion) = Menu::new(request);
            menu.init(&mut Effects::default());
            let fonts = menu
                .fonts()
                .into_iter()
                .map(|bytes| lucent_ui::text::Font::from_bytes(bytes, Default::default()).unwrap())
                .collect();
            let layout = Layout::new(Arc::new(fonts));
            let cx = ViewContext {
                surface: "menu",
                instance: None,
                width: 640.,
                height: 580.,
                now: 1.,
                density: Density::ONE,
            };
            menu.update(
                Message::Resize(cx.width, cx.height),
                &mut Effects::default(),
            );
            let mut input = Interaction::default();
            let scene = layout.build(&menu.view(&cx), cx.width, cx.height, &input, cx.now);
            let panel = scene
                .hits
                .iter()
                .find(|hit| hit.id == "menu-panel")
                .unwrap();
            assert!(
                input
                    .motion(
                        &scene,
                        panel.rect.x + token::PADDING / 2.,
                        panel.rect.y + panel.rect.h / 2.,
                        0.,
                    )
                    .is_empty()
            );
            assert_eq!(input.hovered.as_deref(), Some("menu-panel"));
            let hovered = layout.build(&menu.view(&cx), cx.width, cx.height, &input, cx.now);
            assert_eq!(format!("{:?}", scene.paint), format!("{:?}", hovered.paint));

            let row = scene
                .hits
                .iter()
                .find(|hit| hit.id == "menu-row-1")
                .unwrap();
            let mut effects = Effects::default();
            for message in input.motion(
                &scene,
                row.rect.x + row.rect.w / 2.,
                row.rect.y + row.rect.h / 2.,
                0.,
            ) {
                menu.update(message, &mut effects);
            }
            let hovered_row = layout.build(&menu.view(&cx), cx.width, cx.height, &input, cx.now);
            assert_eq!(input.hovered.as_deref(), Some("menu-row-1"));
            assert_eq!(menu.selection.selected, 1);
            assert_ne!(
                format!("{:?}", scene.paint),
                format!("{:?}", hovered_row.paint)
            );
            assert!(!effects.exit);
            assert!(completion.outcome().is_none());
        }
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
