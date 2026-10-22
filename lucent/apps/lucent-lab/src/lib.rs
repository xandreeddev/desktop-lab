//! A standalone consumer of production presentation APIs, without desktop adapters.
use std::collections::VecDeque;

use lucent_api::*;
use lucent_design::{Theme, component::lab as token, font, motion, palettes, radius, space};
use lucent_domain::{MenuEntry, MenuOutcome};

pub mod render;
pub mod stories;

pub const SURFACE: &str = "lab";
pub const FONT_BYTES: &[&[u8]] = &[
    include_bytes!("../../lucent-desktop/assets/LucentSans.ttf"),
    include_bytes!("../../lucent-desktop/assets/LucentDisplay.ttf"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewport {
    Regular,
    Narrow,
}
impl Viewport {
    pub const ALL: [Self; 2] = [Self::Regular, Self::Narrow];
    pub fn id(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Narrow => "narrow",
        }
    }
    pub fn width(self) -> f32 {
        match self {
            Self::Regular => token::STAGE_WIDTH,
            Self::Narrow => token::NARROW_WIDTH,
        }
    }
    pub fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.id() == value)
            .ok_or_else(|| "Viewport must be regular or narrow".into())
    }
}

#[derive(Clone)]
pub enum Message {
    Story(usize),
    Palette(usize),
    State(usize),
    Viewport(Viewport),
    Reset,
    Edit(u64, TextEdit),
    Clear,
    Toggle,
    Slider(SliderEvent),
    Scroll(ScrollEvent),
    TogglePopover,
    ClosePopover,
    ChoosePopover,
    Replay,
    Action(&'static str),
    MenuKeys,
    Menu(u64, lucent_menu::Message),
    Quit,
}

pub struct Lab {
    story_index: usize,
    palette_index: usize,
    pub(crate) state: &'static str,
    pub viewport: Viewport,
    pub(crate) generation: u64,
    pub(crate) text: String,
    pub(crate) field: TextFieldState,
    pub(crate) error: bool,
    pub(crate) checked: bool,
    pub(crate) value: f32,
    pub(crate) offset: f32,
    pub(crate) open: bool,
    pub(crate) movement: Motion,
    replaying: bool,
    pub(crate) menu: lucent_menu::Menu,
    completion: lucent_menu::Completion,
    menu_finished: bool,
    menu_keys: bool,
    log: VecDeque<&'static str>,
}

fn menu(palette: usize, state: &str) -> (lucent_menu::Menu, lucent_menu::Completion) {
    let entries = if state == "empty" {
        vec![]
    } else {
        [
            "Open project",
            "Recent files",
            "Preferences",
            "Quit application",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, label)| MenuEntry {
            label: label.into(),
            detail: if index == 0 {
                "A controlled fixture; no application is launched".into()
            } else {
                String::new()
            },
            value: format!("fixture-{index}"),
            disabled: state == "disabled" && index % 2 == 0,
        })
        .collect()
    };
    lucent_menu::Menu::new(lucent_menu::Request {
        prompt: "Choose an action".into(),
        mode: lucent_menu::Mode::Select,
        entries,
        width: None,
        max_height: None,
        light: false,
        palette: Some(palettes()[palette].clone()),
        back: false,
    })
}

impl Lab {
    pub fn new(
        story: &str,
        palette: &str,
        state: Option<&str>,
        viewport: Viewport,
    ) -> Result<Self, String> {
        let story_index = stories::STORIES
            .iter()
            .position(|item| item.id == story)
            .ok_or("Unknown story")?;
        let palette_index = palettes()
            .iter()
            .position(|item| item.id == palette)
            .ok_or("Unknown palette")?;
        let story = &stories::STORIES[story_index];
        let state = story
            .states
            .iter()
            .copied()
            .find(|item| Some(*item) == state)
            .or_else(|| state.is_none().then_some(story.states[0]))
            .ok_or("Unknown state for story")?;
        let (menu, completion) = menu(palette_index, state);
        let mut lab = Self {
            story_index,
            palette_index,
            state,
            viewport,
            generation: 0,
            text: String::new(),
            field: TextFieldState::default(),
            error: false,
            checked: false,
            value: 0.5,
            offset: 0.,
            open: false,
            movement: Motion::fixed(0.),
            replaying: false,
            menu,
            completion,
            menu_finished: false,
            menu_keys: false,
            log: VecDeque::new(),
        };
        lab.reset(0.);
        Ok(lab)
    }
    pub fn story(&self) -> &'static stories::Story {
        &stories::STORIES[self.story_index]
    }
    pub fn palette_id(&self) -> &str {
        &palettes()[self.palette_index].id
    }
    pub fn state_id(&self) -> &str {
        self.state
    }
    pub fn theme(&self) -> Theme {
        Theme::from_palette(&palettes()[self.palette_index])
    }
    pub fn preview_context(&self, now: f64, density: Density) -> ViewContext {
        ViewContext {
            surface: SURFACE,
            instance: None,
            width: self.viewport.width(),
            height: token::STAGE_HEIGHT,
            density,
            now,
        }
    }
    pub fn story_view(&self, cx: &ViewContext) -> Element<Message> {
        self.theme().apply(
            stories::view(self, cx)
                .size(cx.width, cx.height)
                .background(self.theme().surface)
                .radius(radius::PANEL),
        )
    }
    fn record(&mut self, text: &'static str) {
        if self.log.len() == 3 {
            self.log.pop_front();
        }
        self.log.push_back(text);
    }
    fn reset(&mut self, now: f64) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("fixture generation exhausted");
        self.field.cancel();
        self.text = if self.state == "empty" || self.state == "error" {
            String::new()
        } else {
            "Lucent components · café".into()
        };
        self.field = TextFieldState::default();
        self.field.reset(&self.text);
        self.error = self.state == "error";
        self.checked = self.state == "on";
        self.value = match self.state {
            "minimum" => 0.,
            "maximum" => 1.,
            _ => 0.5,
        };
        self.offset = if self.state == "bottom" {
            component_scroll_end()
        } else {
            0.
        };
        self.open = self.story().id == "popover" && self.state == "open";
        let mut sample = Motion::fixed(0.);
        sample.target(1., 0., motion::PANEL, motion::DECELERATE);
        self.movement =
            Motion::fixed(sample.value(self.story().time_ms(self.state) as f64 / 1000.));
        self.replaying = false;
        (self.menu, self.completion) = menu(self.palette_index, self.state);
        self.menu_finished = false;
        self.menu_keys = false;
        let mut effects = Effects {
            now,
            ..Default::default()
        };
        self.menu.init(&mut effects);
        self.menu.update(
            lucent_menu::Message::Resize(self.viewport.width(), token::STAGE_HEIGHT),
            &mut effects,
        );
    }
    fn menu_update(&mut self, message: lucent_menu::Message, effects: &mut Effects<Message>) {
        let generation = self.generation;
        effects.delegate(
            |child| {
                self.menu.update(message, child);
                child.exit = false;
                child.redraw.remove("menu");
                child.redraw(SURFACE);
                for request in &mut child.clipboard {
                    request.target.surface = SURFACE;
                }
            },
            move |message| Message::Menu(generation, message),
        );
        if !self.menu_finished
            && let Some(outcome) = self.completion.outcome()
        {
            let event = match outcome {
                MenuOutcome::Accepted(_) => "Menu completed (fixture only)",
                MenuOutcome::Cancelled => "Menu cancelled",
                MenuOutcome::Parent => "Menu returned",
            };
            self.menu_finished = true;
            self.menu_keys = false;
            self.record(event);
        }
    }
}

fn component_scroll_end() -> f32 {
    // A deliberately overlong controlled request: Layout clamps to the actual
    // bounded viewport. No extra scroll implementation is supplied by the lab.
    16. * (lucent_design::component::input::HEIGHT + space::SM)
}

impl Component for Lab {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        let theme = self.theme();
        let mut toolbar_actions = vec![
            theme
                .choice_button(
                    "Regular",
                    Message::Viewport(Viewport::Regular),
                    self.viewport == Viewport::Regular,
                )
                .id("viewport-regular"),
            theme
                .choice_button(
                    "Narrow",
                    Message::Viewport(Viewport::Narrow),
                    self.viewport == Viewport::Narrow,
                )
                .id("viewport-narrow"),
            theme.button("Reset", Message::Reset).id("lab-reset"),
            theme.button("Close lab", Message::Quit).id("lab-close"),
        ];
        toolbar_actions.extend(
            self.story()
                .states
                .iter()
                .enumerate()
                .map(|(index, state)| {
                    theme
                        .choice_button(*state, Message::State(index), *state == self.state)
                        .id(format!("preset-{state}"))
                }),
        );
        let toolbar = Element::column(vec![
            Element::row(
                palettes()
                    .iter()
                    .enumerate()
                    .map(|(index, palette)| {
                        theme
                            .choice_button(
                                &palette.name,
                                Message::Palette(index),
                                index == self.palette_index,
                            )
                            .id(format!("palette-{index}"))
                    })
                    .collect(),
            )
            .gap(space::SM),
            Element::row(toolbar_actions).gap(space::SM),
        ])
        .height(Length::Fixed(token::TOOLBAR_HEIGHT))
        .gap(space::XS);
        let mut navigation = Vec::new();
        for family in stories::FAMILIES {
            navigation.push(
                Element::text(*family)
                    .font(font::CAPTION)
                    .color(theme.on_surface),
            );
            navigation.extend(
                stories::STORIES
                    .iter()
                    .enumerate()
                    .filter(|(_, story)| story.family == *family)
                    .map(|(index, story)| {
                        theme
                            .choice_button(
                                story.title,
                                Message::Story(index),
                                index == self.story_index,
                            )
                            .id(format!("story-{}", story.id))
                            .width(Length::Fill)
                    }),
            );
        }
        let navigation = Element::column(navigation)
            .gap(space::XS)
            .width(Length::Fixed(token::NAV_WIDTH));
        let preview = self.story_view(&self.preview_context(cx.now, cx.density));
        // The footer holds a bounded log and the explicit Menu keyboard session.
        let log = Element::column(
            self.log
                .iter()
                .map(|line| {
                    Element::text(*line)
                        .font(font::CAPTION)
                        .color(theme.on_surface)
                })
                .collect(),
        )
        .gap(space::XXS)
        .width(Length::Fill);
        let mut footer = vec![log];
        if self.story().id == "menu" {
            footer.insert(
                0,
                if self.menu_keys {
                    Element::text("Menu keyboard active")
                        .font(font::CONTROL)
                        .color(theme.on_surface)
                } else {
                    theme
                        .button("Try menu keyboard", Message::MenuKeys)
                        .id("menu-keyboard-start")
                },
            );
            footer.insert(
                1,
                Element::text("Esc returns to Lab controls")
                    .font(font::CAPTION)
                    .color(theme.on_surface)
                    .wrap(2),
            );
        }
        let footer = Element::row(footer)
            .gap(space::MD)
            .align(Align::Center)
            .height(Length::Fixed(token::STATUS_HEIGHT));
        theme.apply(
            Element::column(vec![
                toolbar,
                Element::row(vec![navigation, preview])
                    .height(Length::Fixed(token::STAGE_HEIGHT))
                    .gap(space::MD),
                footer,
            ])
            .fill()
            .padding(space::MD)
            .gap(space::SM)
            .background(theme.surface_container)
            .radius(radius::PANEL),
        )
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Story(index) if index < stories::STORIES.len() => {
                self.story_index = index;
                self.state = self.story().states[0];
                self.reset(effects.now);
                self.record("Story changed");
            }
            Message::Palette(index) if index < palettes().len() => {
                self.palette_index = index;
                self.reset(effects.now);
                self.record("Palette changed");
            }
            Message::State(index) if index < self.story().states.len() => {
                self.state = self.story().states[index];
                self.reset(effects.now);
                self.record("Preset reset");
            }
            Message::Viewport(viewport) => {
                self.viewport = viewport;
                self.reset(effects.now);
                self.record("Viewport changed");
            }
            Message::Reset => {
                self.reset(effects.now);
                self.record("Fixture reset");
            }
            Message::Edit(generation, edit)
                if generation == self.generation
                    && matches!(self.story().id, "input" | "input-group") =>
            {
                self.error = self
                    .field
                    .update(
                        &mut self.text,
                        edit,
                        4096,
                        ClipboardTarget::new(SURFACE, "lab-editor"),
                        effects,
                        move |edit| Message::Edit(generation, edit),
                    )
                    .is_err();
                self.record("Text edit or selection");
            }
            Message::Clear => {
                self.text.clear();
                self.field.reset(&self.text);
                self.record("Input cleared");
            }
            Message::Toggle => {
                self.checked = !self.checked;
                self.record("Toggle changed");
            }
            Message::Slider(event) => {
                self.value = event.value;
                self.record("Slider changed");
            }
            Message::Scroll(event) => {
                self.offset = event.offset;
                self.record("Scroll offset changed");
            }
            Message::TogglePopover => {
                self.open = !self.open;
                self.record("Popover toggled");
            }
            Message::ClosePopover => {
                self.open = false;
                self.record("Popover dismissed");
            }
            Message::ChoosePopover => {
                self.open = false;
                self.record("Popover choice");
            }
            Message::Replay => {
                self.movement = Motion::fixed(0.);
                self.movement
                    .target(1., effects.now, motion::PANEL, motion::DECELERATE);
                self.replaying = true;
                self.record("Motion replay");
            }
            Message::Action(label) => self.record(label),
            Message::MenuKeys if self.story().id == "menu" => {
                if self.menu_finished {
                    self.reset(effects.now);
                }
                self.menu_keys = true;
                self.record("Menu keyboard session");
            }
            Message::Menu(generation, message)
                if generation == self.generation && self.story().id == "menu" =>
            {
                self.menu_update(message, effects)
            }
            Message::Quit => effects.quit(),
            _ => return,
        }
        effects.redraw(SURFACE);
    }
}

impl Application for Lab {
    fn name(&self) -> &'static str {
        "lucent-lab"
    }
    fn fonts(&self) -> Vec<&'static [u8]> {
        FONT_BYTES.to_vec()
    }
    fn clipboard_enabled(&self) -> bool {
        true
    }
    fn input_method_enabled(&self) -> bool {
        true
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: SURFACE,
            instance: None,
            output: None,
            layer: Layer::Overlay,
            anchor: Anchor::Center,
            margins: Default::default(),
            redraw_group: None,
            width: token::WIDTH as u32,
            height: token::HEIGHT as u32,
            exclusive_zone: -1,
            keyboard: Keyboard::OnDemand,
            visible: true,
            pointer_input: PointerInput::Content,
        }]
    }
    fn init(&mut self, effects: &mut Effects<Message>) {
        self.reset(effects.now);
        effects.redraw(SURFACE);
    }
    fn event(&self, event: Event) -> Option<Message> {
        if self.story().id == "menu" && self.menu_keys && matches!(&event, Event::Key { .. }) {
            self.menu
                .event(event)
                .map(|message| Message::Menu(self.generation, message))
        } else {
            None
        }
    }
    fn animating(&self, _: &str, now: f64) -> bool {
        (self.story().id == "menu" && self.menu.animating("menu", now))
            || (self.story().id == "motion" && self.replaying && self.movement.active(now))
    }
    fn command(&self, command: &str) -> Result<Option<Message>, String> {
        if command == "quit" {
            Ok(Some(Message::Quit))
        } else {
            Err("Use inspect or quit".into())
        }
    }
    fn inspect(&self) -> String {
        serde_json::json!({"story":self.story().id,"palette":self.palette_id(),"state":self.state,"viewport":self.viewport.id(),"menu_keyboard_session":self.menu_keys,"events":self.log,"scope":"controlled component fixtures"}).to_string()
    }
}

#[cfg(test)]
mod tests;
