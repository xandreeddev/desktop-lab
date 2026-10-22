//! Presentation fixtures consume the same public recipes as ordinary clients.
use lucent_api::*;
use lucent_design::{component, font, motion, radius, space};

use crate::{Lab, Message};

pub struct Story {
    pub id: &'static str,
    pub family: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub states: &'static [&'static str],
    pub tokens: &'static [&'static str],
    pub recipe: &'static str,
    pub source: Source,
}

pub struct Source {
    pub path: &'static str,
    pub symbol: &'static str,
}

pub const FAMILIES: &[&str] = &["Controls", "Fields", "Layout", "Menus", "Motion"];

pub const STORIES: &[Story] = &[
    Story {
        id: "buttons",
        source: Source {
            path: "lucent/crates/lucent-design/src/lib.rs",
            symbol: "Theme::button_enabled",
        },
        family: "Controls",
        title: "Buttons",
        description: "Shared action, choice and disabled recipes. Tab and pointer feedback come from the engine.",
        states: &["default", "hover", "focus", "disabled"],
        tokens: &[
            "component.button.radius",
            "component.button.padding_inline",
            "component.button.padding_block",
            "font.control",
            "opacity.muted",
        ],
        recipe: "theme.button_enabled(\"Apply changes\", Message::Action(\"Apply\"), lab.state != \"disabled\")",
    },
    Story {
        id: "input",
        source: Source {
            path: "lucent/crates/lucent-design/src/lib.rs",
            symbol: "Theme::text_input",
        },
        family: "Fields",
        title: "Text input",
        description: "Ordinary editing, selection, clipboard and semantic supporting text.",
        states: &["filled", "empty", "error"],
        tokens: &[
            "component.input.height",
            "component.input.padding_inline",
            "component.input.padding_block",
            "radius.control",
        ],
        recipe: "theme.text_input(&lab.text, lab.field.selection(), \"Your name\", move |edit| Message::Edit(generation, edit))",
    },
    Story {
        id: "input-group",
        source: Source {
            path: "lucent/crates/lucent-design/src/lib.rs",
            symbol: "Theme::input_group",
        },
        family: "Fields",
        title: "Input group",
        description: "One shared rounded field with leading and trailing content and a focus-within outline.",
        states: &["filled", "empty"],
        tokens: &[
            "component.input.height",
            "component.input.padding_inline",
            "space.sm",
        ],
        recipe: "theme.input_group(vec![leading, theme.text_input_content(&lab.text, lab.field.selection(), \"Search components\", move |edit| Message::Edit(generation, edit)), trailing])",
    },
    Story {
        id: "toggle",
        source: Source {
            path: "lucent/crates/lucent-design/src/lib.rs",
            symbol: "Theme::toggle_row",
        },
        family: "Controls",
        title: "Toggle",
        description: "One controlled label and switch, with a single focus and pointer target.",
        states: &["off", "on"],
        tokens: &[
            "component.switch.width",
            "component.switch.height",
            "component.button.radius",
        ],
        recipe: "theme.toggle_row(\"Notifications\", Message::Toggle, checked)",
    },
    Story {
        id: "slider",
        source: Source {
            path: "lucent/crates/lucent-design/src/lib.rs",
            symbol: "Theme::slider",
        },
        family: "Controls",
        title: "Slider",
        description: "Controlled value; pointer dragging and keyboard steps use the actual slider.",
        states: &["middle", "minimum", "maximum"],
        tokens: &[
            "component.slider.height",
            "component.slider.track_height",
            "component.slider.thumb_diameter",
        ],
        recipe: "theme.slider(SliderSpec::new(value, 0.1).unwrap(), Message::Slider)",
    },
    Story {
        id: "scroll",
        source: Source {
            path: "lucent/crates/lucent-design/src/lib.rs",
            symbol: "Theme::scroll",
        },
        family: "Layout",
        title: "Scroll",
        description: "Controlled vertical offset, wheel routing, focus reveal and a draggable thumb.",
        states: &["top", "bottom"],
        tokens: &[
            "component.scroll.line_step",
            "component.scroll.padding",
            "component.scroll.thumb_width",
        ],
        recipe: "theme.scroll(ScrollSpec::new(offset, component::scroll::LINE_STEP).unwrap(), content, Message::Scroll)",
    },
    Story {
        id: "popover",
        source: Source {
            path: "lucent/crates/lucent-design/src/lib.rs",
            symbol: "Theme::popover",
        },
        family: "Layout",
        title: "Popover",
        description: "One surface-local focus scope, outside/Escape dismissal and opener restoration.",
        states: &["open", "closed"],
        tokens: &[
            "component.popover.width",
            "component.popover.gap",
            "component.popover.inset",
            "radius.panel",
        ],
        recipe: "root.popover(Popover { id, anchor, placement, content: Box::new(theme.popover(content)), dismiss: Message::ClosePopover })",
    },
    Story {
        id: "menu",
        source: Source {
            path: "lucent/apps/lucent-menu/src/lib.rs",
            symbol: "<Menu as Component>::view",
        },
        family: "Menus",
        title: "Actual Menu",
        description: "The production lucent-menu client, embedded with isolated completion and clipboard routing.",
        states: &["populated", "empty", "disabled"],
        tokens: &[
            "component.menu.width",
            "component.menu.row_height",
            "component.menu.padding",
            "component.input.height",
        ],
        recipe: "let (menu, completion) = lucent_menu::Menu::new(request);\nmenu.view(cx).map(move |message| Message::Menu(generation, message))",
    },
    Story {
        id: "motion",
        source: Source {
            path: "lucent/crates/lucent-api/src/lib.rs",
            symbol: "Motion::target",
        },
        family: "Motion",
        title: "Motion",
        description: "A shared easing curve and controlled Motion; native replay requests compositor-paced animation only while active.",
        states: &["end", "start", "middle"],
        tokens: &[
            "motion.panel",
            "motion.decelerate",
            "radius.control",
            "space.xxl",
        ],
        recipe: "motion.target(1., effects.now, motion::PANEL, motion::DECELERATE);\nshape.at(travel * motion.value(cx.now), 0.)",
    },
];

impl Story {
    pub fn time_ms(&self, state: &str) -> u64 {
        if self.id == "motion" {
            match state {
                "start" => 0,
                "middle" => (motion::PANEL * 500.) as u64,
                _ => (motion::PANEL * 1000.) as u64,
            }
        } else {
            1000
        }
    }
}

pub fn view(lab: &Lab, cx: &ViewContext) -> Element<Message> {
    let theme = lab.theme();
    if lab.story().id == "menu" {
        let generation = lab.generation;
        return lab
            .menu
            .view(cx)
            .map(move |message| Message::Menu(generation, message));
    }
    let generation = lab.generation;
    let editor = || {
        theme
            .text_input(&lab.text, lab.field.selection(), "Your name", move |edit| {
                Message::Edit(generation, edit)
            })
            .id("lab-editor")
    };
    let body = match lab.story().id {
        "buttons" => Element::column(vec![
            theme
                .button_enabled(
                    "Apply changes",
                    Message::Action("Apply"),
                    lab.state != "disabled",
                )
                .id("sample-primary")
                .autofocus_if(lab.state == "focus"),
            theme
                .choice_button("Selected choice", Message::Action("Choice"), true)
                .id("sample-choice"),
            theme
                .button_enabled("Unavailable", Message::Action("Disabled"), false)
                .id("sample-disabled"),
        ])
        .gap(space::MD),
        "input" => Element::column(vec![
            editor(),
            theme
                .status_text(
                    if lab.error {
                        "Choose a name before continuing."
                    } else {
                        "Try café, selection, undo, or paste."
                    },
                    lab.error,
                )
                .wrap(2),
        ])
        .gap(space::SM),
        "input-group" => theme.input_group(vec![
            Element::text("Find")
                .font(font::CAPTION)
                .color(theme.on_surface),
            theme
                .text_input_content(
                    &lab.text,
                    lab.field.selection(),
                    "Search components",
                    move |edit| Message::Edit(generation, edit),
                )
                .id("lab-editor"),
            theme
                .button_enabled("Clear", Message::Clear, !lab.text.is_empty())
                .id("clear-input"),
        ]),
        "toggle" => theme
            .toggle_row("Notifications", Message::Toggle, lab.checked)
            .id("sample-toggle"),
        "slider" => Element::column(vec![
            theme
                .slider(
                    SliderSpec::new(lab.value, 0.1).expect("controlled slider"),
                    Message::Slider,
                )
                .id("sample-slider"),
            theme.status_text(format!("Value: {:.0}%", lab.value * 100.), false),
        ])
        .gap(space::MD),
        "scroll" => {
            let content = Element::column(
                (0..16)
                    .map(|index| {
                        theme
                            .button(
                                format!("Item {:02}", index + 1),
                                Message::Action("List item"),
                            )
                            .id(format!("list-{index}"))
                            .width(Length::Fill)
                            .height(Length::Fixed(component::input::HEIGHT))
                    })
                    .collect(),
            )
            .width(Length::Fill)
            .gap(space::SM);
            theme
                .scroll(
                    ScrollSpec::new(lab.offset, component::scroll::LINE_STEP)
                        .expect("controlled scroll"),
                    content,
                    Message::Scroll,
                )
                .id("sample-scroll")
                .height(Length::Fixed(component::lab::STAGE_HEIGHT / 2.))
        }
        "popover" => theme
            .button("Choose an option", Message::TogglePopover)
            .id("popover-trigger")
            .tooltip(theme.tooltip("Opens a surface-local popover", OverlaySide::Above)),
        "motion" => {
            let size = space::XXL * 2.;
            let travel = (cx.width - space::XL * 2. - size).max(0.);
            Element::column(vec![
                Element::stack(vec![
                    Element::empty()
                        .size(size, size)
                        .background(theme.primary)
                        .radius(radius::CONTROL)
                        .at(travel * lab.movement.value(cx.now), 0.),
                ])
                .width(Length::Fill)
                .height(Length::Fixed(size)),
                theme
                    .button("Replay motion", Message::Replay)
                    .id("motion-replay"),
            ])
            .gap(space::XL)
        }
        _ => unreachable!("validated story"),
    };
    let mut root = Element::column(vec![
        Element::text(lab.story().title)
            .font(font::TITLE)
            .color(theme.on_surface),
        Element::text(lab.story().description)
            .font(font::BODY)
            .color(theme.on_surface)
            .width(Length::Fill)
            .wrap(4),
        body,
    ])
    .fill()
    .padding(space::XL)
    .gap(space::LG);
    if lab.story().id == "popover" && lab.open {
        root = root.popover(Popover {
            id: "lab-popover".into(),
            anchor: "popover-trigger".into(),
            placement: AnchoredSpec {
                side: OverlaySide::Below,
                align: OverlayAlign::Start,
                gap: component::popover::GAP,
                inset: component::popover::INSET,
            },
            content: Box::new(
                theme.popover(
                    Element::column(vec![
                        Element::text("Choose an option")
                            .font(font::LABEL)
                            .color(theme.on_surface),
                        theme
                            .button("First option", Message::ChoosePopover)
                            .id("popover-first"),
                        theme
                            .button("Second option", Message::ChoosePopover)
                            .id("popover-second"),
                        theme
                            .button("Cancel", Message::ClosePopover)
                            .id("popover-cancel"),
                    ])
                    .width(Length::Fill)
                    .gap(space::SM),
                ),
            ),
            dismiss: Message::ClosePopover,
        });
    }
    theme.apply(root)
}

// Only the focus preset requests initial focus; later native focus is engine-owned.
trait InitialFocus<M> {
    fn autofocus_if(self, enabled: bool) -> Self;
}
impl<M> InitialFocus<M> for Element<M> {
    fn autofocus_if(self, enabled: bool) -> Self {
        if enabled { self.autofocus() } else { self }
    }
}
