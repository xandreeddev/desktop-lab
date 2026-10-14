//! Token-based notification presentation; all state and adapters stay in the child.
use super::{Center, Message};
use lucent_api::*;
use lucent_design::{Theme, component::notification as token, font, radius, space};
use lucent_domain::{Notification, NotificationHistoryStatus};
impl Center {
    fn card(
        &self,
        note: &Notification,
        history: bool,
        detailed: bool,
        t: Theme,
    ) -> Element<Message> {
        let mut children = vec![
            Element::row(vec![
                Element::text(&note.app)
                    .font(font::CAPTION)
                    .color(t.primary)
                    .width(Length::Fill),
                if history {
                    Element::empty()
                } else {
                    note.target()
                        .map(|target| {
                            t.button("×", Message::Dismiss(target)).id(format!(
                                "notification-dismiss-{}-{}",
                                note.id, note.revision
                            ))
                        })
                        .unwrap_or_else(Element::empty)
                },
            ])
            .width(Length::Fill)
            .align(Align::Center),
            Element::text(&note.summary)
                .font(font::TITLE)
                .color(if note.critical { t.error } else { t.on_surface })
                .width(Length::Fill)
                .wrap(token::TITLE_LINES as usize),
        ];
        if !note.body.is_empty() {
            children.push(
                Element::text(&note.body)
                    .font(font::BODY)
                    .color(t.on_surface)
                    .width(Length::Fill)
                    .wrap(if detailed {
                        token::HISTORY_BODY_LINES
                    } else {
                        token::BODY_LINES
                    } as usize),
            );
        }
        if !history
            && !note.actions.is_empty()
            && let Some(target) = note.target()
        {
            let index = self.action_pages.get(&target).copied().unwrap_or(0) % note.actions.len();
            let action = &note.actions[index];
            let mut buttons = vec![
                t.button(&action.label, Message::Invoke(target, action.id.clone()))
                    .width(Length::Fill)
                    .id(format!(
                        "notification-action-{}-{}-{index}",
                        note.id, note.revision
                    )),
            ];
            if note.actions.len() > 1 {
                buttons.push(
                    t.button("Next action", Message::ActionPage(target, 1))
                        .id(format!("notification-next-action-{}", note.id)),
                );
            }
            children.push(Element::row(buttons).width(Length::Fill).gap(space::XS));
        }
        Element::column(children)
            .gap(token::GAP)
            .padding(token::PADDING)
            .width(Length::Fill)
            .radius(radius::PANEL)
            .background(t.surface)
            .shadow()
            .clip()
    }
    pub fn view_with_theme(&self, cx: &ViewContext, t: Theme) -> Element<Message> {
        let width = token::WIDTH.min((cx.width - token::MARGIN * 2.).max(0.));
        let top = lucent_design::component::panel::BAR_HEIGHT + token::MARGIN;
        let panel = if self.history_open || self.closing {
            let mut controls = vec![
                t.button(
                    if self.snapshot.do_not_disturb {
                        "DND on"
                    } else {
                        "DND off"
                    },
                    Message::Dnd,
                )
                .width(Length::Fill)
                .id("notification-dnd"),
                t.button("Clear", Message::Clear)
                    .width(Length::Fill)
                    .id("notification-clear"),
            ];
            if matches!(
                self.snapshot.history_status,
                NotificationHistoryStatus::ReadFailed | NotificationHistoryStatus::WriteFailed
            ) {
                controls.push(
                    t.button("Retry", Message::RetryHistory)
                        .width(Length::Fill)
                        .id("notification-history-retry"),
                );
            }
            let status = if !self.ready {
                "Notification service unavailable"
            } else {
                match self.snapshot.history_status {
                    NotificationHistoryStatus::ReadFailed => {
                        "History could not be read; file preserved"
                    }
                    NotificationHistoryStatus::WriteFailed => "History could not be saved",
                    _ if self.error => "That action is no longer available",
                    _ => "Recent activity",
                }
            };
            let header = Element::column(vec![
                Element::row(vec![
                    Element::text("Notifications")
                        .color(t.on_surface)
                        .font(font::TITLE)
                        .width(Length::Fill)
                        .wrap(1),
                    t.button("×", Message::Toggle)
                        .id("notification-history-close"),
                ])
                .gap(space::SM)
                .align(Align::Center)
                .width(Length::Fill),
                Element::row(controls).gap(space::XS).width(Length::Fill),
                Element::text(status)
                    .color(
                        if self.error
                            || matches!(
                                self.snapshot.history_status,
                                NotificationHistoryStatus::ReadFailed
                                    | NotificationHistoryStatus::WriteFailed
                            )
                        {
                            t.error
                        } else {
                            t.on_surface
                        },
                    )
                    .font(font::CAPTION)
                    .wrap(2)
                    .width(Length::Fill),
            ])
            .gap(space::SM)
            .width(Length::Fill);
            let groups = self.groups();
            let mut rows = vec![];
            for group in &groups {
                let selected = self.selected_group.as_ref() == Some(&group.key);
                let live = group
                    .entries
                    .iter()
                    .filter(|(_, history)| !*history)
                    .count();
                let label = format!(
                    "{} · {}{}",
                    if group.label.is_empty() {
                        "Application"
                    } else {
                        group.label
                    },
                    group.entries.len(),
                    if live > 0 {
                        format!(" · {live} active")
                    } else {
                        String::new()
                    }
                );
                rows.push(
                    t.button(label, Message::Group(group.key.clone()))
                        .id(format!("notification-group-{}", group.key))
                        .width(Length::Fill)
                        .height(Length::Fixed(token::GROUP_HEIGHT))
                        .selected(selected)
                        .wrap(1),
                );
                if selected {
                    let page = self.page.min(group.entries.len().saturating_sub(1));
                    if let Some((note, history)) = group.entries.get(page) {
                        rows.push(self.card(note, *history, true, t));
                    }
                    if group.entries.len() > 1 {
                        rows.push(
                            Element::row(vec![
                                t.button("Previous", Message::Page(-1))
                                    .id("notification-previous"),
                                Element::text(format!("{} / {}", page + 1, group.entries.len()))
                                    .color(t.on_surface)
                                    .width(Length::Fill)
                                    .font(font::CAPTION)
                                    .align(Align::Center),
                                t.button("Next", Message::Page(1)).id("notification-next"),
                            ])
                            .gap(space::XS)
                            .align(Align::Center)
                            .width(Length::Fill),
                        );
                    }
                }
            }
            if rows.is_empty() {
                rows.push(
                    Element::text("No notifications yet")
                        .color(t.on_surface)
                        .font(font::BODY)
                        .wrap(2)
                        .width(Length::Fill),
                );
            }
            let content = Element::column(rows).gap(token::GAP).width(Length::Fill);
            let scroll = t
                .scroll(
                    ScrollSpec::new(self.scroll, lucent_design::component::scroll::LINE_STEP)
                        .expect("controlled offset"),
                    content,
                    Message::Scroll,
                )
                .id("notification-groups")
                .width(Length::Fill)
                .height(Length::Fill);
            Element::column(vec![header, scroll])
                .gap(token::GAP)
                .padding(token::PADDING)
                .size(
                    width,
                    token::HISTORY_HEIGHT.min((cx.height - top - token::MARGIN).max(0.)),
                )
                .radius(radius::PANEL)
                .background(t.surface)
                .shadow()
                .clip()
        } else {
            let room = cx.height - top - token::MARGIN;
            let limit = (room / (token::MAX_CARD_HEIGHT + token::GAP))
                .floor()
                .max(1.) as usize;
            let cards = self
                .snapshot
                .active
                .iter()
                .filter(|n| !self.snapshot.do_not_disturb || n.critical)
                .take(limit.min(token::TOAST_LIMIT as usize))
                .map(|note| self.card(note, false, false, t))
                .collect();
            Element::column(cards)
                .gap(token::GAP)
                .width(Length::Fixed(width))
        };
        let reveal = if self.history_open || self.closing {
            self.reveal.value(cx.now)
        } else {
            1.
        };
        let offset = if self.history_open || self.closing {
            token::HISTORY_OFFSET * (1. - reveal)
        } else {
            0.
        };
        t.apply(
            Element::stack(vec![
                panel
                    .opacity(reveal)
                    .at(cx.width - width - token::MARGIN, top - offset),
            ])
            .fill(),
        )
    }
}
