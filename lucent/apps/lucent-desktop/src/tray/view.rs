use super::*;
use lucent_design::{
    Theme,
    component::{scroll, tray as token},
    font, icon, radius, space,
};
fn text(value: impl Into<String>, theme: Theme) -> Element<Message> {
    Element::text(value)
        .font(font::BODY)
        .color(theme.on_surface)
        .width(Length::Fill)
        .wrap(token::TITLE_LINES as usize)
}
fn button(
    label: impl Into<String>,
    id: impl Into<String>,
    message: Message,
    enabled: bool,
    theme: Theme,
) -> Element<Message> {
    theme
        .button(label, if enabled { message } else { Message::Ignore })
        .id(id)
        .width(Length::Fill)
        .tab_stop(enabled)
        .wrap(token::TITLE_LINES as usize)
        .opacity(if enabled {
            1.
        } else {
            lucent_design::opacity::MUTED
        })
}
fn label(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
        } else {
            out.push(ch);
        }
    }
    out
}
impl Tray {
    fn image(&self, item: TrayItemId, role: &str, size: f32) -> Element<Message> {
        self.images
            .get(&format!("tray:{}:{role}", item.value()))
            .cloned()
            .map(Element::image)
            .unwrap_or_else(Element::empty)
            .contain()
            .size(size, size)
    }
    fn content(&self, theme: Theme) -> Vec<Element<Message>> {
        let mut rows = vec![];
        if !self.ready {
            return vec![text(&self.status, theme)];
        }
        if let Some(menu) = &self.menu {
            rows.push(button("Back", "tray-back", Message::Back, true, theme));
            let parent = self.parents.last().copied().unwrap_or(0);
            let entries =
                lucent_usecases::tray::menu_entry(&menu.root, parent).map(|e| &e.children);
            for entry in entries.into_iter().flatten().filter(|e| e.visible) {
                if entry.separator {
                    rows.push(
                        Element::empty()
                            .width(Length::Fill)
                            .height(Length::Fixed(token::SEPARATOR_HEIGHT))
                            .background(theme.on_surface.alpha(lucent_design::opacity::QUIET)),
                    );
                    continue;
                }
                let enabled = entry.enabled && self.active.is_none();
                let mut content = vec![];
                match entry.check {
                    TrayCheck::None => content.push(Element::empty().size(
                        lucent_design::component::selection_indicator::SIZE,
                        lucent_design::component::selection_indicator::SIZE,
                    )),
                    TrayCheck::Check(checked) => {
                        content.push(theme.selection_indicator(checked, false))
                    }
                    TrayCheck::Radio(checked) => {
                        content.push(theme.selection_indicator(checked, true))
                    }
                }
                content.push(
                    text(
                        format!(
                            "{}{}",
                            label(&entry.label),
                            if entry.is_submenu() { " ›" } else { "" }
                        ),
                        theme,
                    )
                    .font(font::CONTROL),
                );
                rows.push(
                    Element::row(content)
                        .width(Length::Fill)
                        .align(Align::Center)
                        .gap(space::SM)
                        .padding_xy(
                            lucent_design::component::button::PADDING_INLINE,
                            lucent_design::component::button::PADDING_BLOCK,
                        )
                        .radius(lucent_design::component::button::RADIUS)
                        .background(theme.surface_container)
                        .on_click(if enabled {
                            Message::Invoke(entry.id)
                        } else {
                            Message::Ignore
                        })
                        .id(format!("tray-menu-{}", entry.id))
                        .tab_stop(enabled)
                        .opacity(if enabled {
                            1.
                        } else {
                            lucent_design::opacity::MUTED
                        }),
                );
            }
        } else {
            for item in self
                .snapshot
                .items
                .iter()
                .filter(|i| i.status != TrayStatus::Passive)
            {
                let mut actions = vec![
                    button(
                        "Open",
                        format!("tray-activate-{}", item.id.value()),
                        Message::Act(item.id, TrayAction::Activate { x: 0, y: 0 }),
                        self.active.is_none(),
                        theme,
                    ),
                    button(
                        "Secondary action",
                        format!("tray-secondary-{}", item.id.value()),
                        Message::Act(item.id, TrayAction::SecondaryActivate { x: 0, y: 0 }),
                        self.active.is_none(),
                        theme,
                    ),
                ];
                if item.has_menu {
                    actions.push(button(
                        "Menu",
                        format!("tray-menu-open-{}", item.id.value()),
                        Message::Menu(item.id, 0),
                        self.active.is_none(),
                        theme,
                    ));
                }
                let title = if item.title.is_empty() {
                    &item.application
                } else {
                    &item.title
                };
                let mut card = vec![
                    Element::row(vec![
                        Element::stack(vec![
                            self.image(item.id, "base", icon::CONTROL),
                            self.image(item.id, "overlay", icon::TINY)
                                .at(icon::CONTROL - icon::TINY, icon::CONTROL - icon::TINY),
                        ])
                        .size(icon::CONTROL, icon::CONTROL),
                        text(title, theme).font(font::TITLE),
                    ])
                    .gap(space::SM),
                    text(&item.tooltip, theme).font(font::CAPTION),
                ];
                if item.status == TrayStatus::NeedsAttention {
                    card.push(text("Needs attention", theme).color(theme.primary));
                }
                card.extend(actions);
                rows.push(
                    Element::column(card)
                        .width(Length::Fill)
                        .padding(token::CARD_PADDING)
                        .gap(token::CARD_GAP)
                        .background(theme.surface_container)
                        .radius(radius::PANEL)
                        .clip(),
                );
            }
            if rows.is_empty() {
                rows.push(text("No active tray applications", theme));
            }
        }
        if self.active.is_some() {
            rows.push(text("Waiting for the application…", theme));
        }
        if !self.error.is_empty() {
            rows.push(text(&self.error, theme).color(theme.error));
        }
        rows
    }
    pub fn view(&self, cx: &ViewContext, theme: Theme) -> Element<Message> {
        if !self.mounted {
            return Element::empty();
        }
        let rect = crate::shell_layout::tray_panel((cx.width, cx.height));
        let progress = self.motion.value(cx.now);
        let panel = Element::column(vec![
            Element::row(vec![
                text("Applications tray", theme).font(font::TITLE),
                button("Close", "tray-close", Message::Close, true, theme).width(Length::Shrink),
            ])
            .height(Length::Fixed(token::HEADER_HEIGHT))
            .align(Align::Center),
            theme
                .scroll(
                    ScrollSpec::new(self.scroll, scroll::LINE_STEP).expect("tray offset"),
                    Element::column(self.content(theme))
                        .width(Length::Fill)
                        .gap(space::MD),
                    Message::Scroll,
                )
                .id("tray-scroll"),
        ])
        .size(rect.w, rect.h)
        .padding(space::XL)
        .gap(space::MD)
        .background(theme.surface)
        .radius(radius::PANEL)
        .shadow()
        .clip()
        .at(rect.x, rect.y - (1. - progress) * token::SLIDE_DISTANCE)
        .opacity(progress)
        .on_click(Message::Ignore)
        .tab_stop(false)
        .id("tray-panel");
        Element::stack(vec![
            Element::empty()
                .fill()
                .on_click(Message::Close)
                .tab_stop(false)
                .id("tray-backdrop"),
            panel,
        ])
        .fill()
    }
}
