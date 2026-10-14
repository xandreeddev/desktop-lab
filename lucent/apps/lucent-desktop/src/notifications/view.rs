//! Token-based notification presentation; all state and adapters stay in the child.
use super::{Center, Message};
use lucent_api::*;
use lucent_design::{Theme, component::notification as token, font, radius, space};
use lucent_domain::{Notification, NotificationHistoryStatus};
use lucent_usecases::notifications::body::{BodyBlock, BodyStyle};
impl Center {
    fn image(&self, key: &str, alt: &str, t: Theme, now: f64) -> Element<Message> {
        let image = if let Some(image) = self.artwork.image(key) {
            let mut element = Element::image(image.clone());
            element.style.image_fit = ImageFit::Contain;
            element.opacity(self.artwork.opacity(key, now))
        } else {
            Element::text(format!(
                "{alt} · {}",
                if self.artwork.loading() {
                    "loading image…"
                } else {
                    "image unavailable"
                }
            ))
            .font(font::CAPTION)
            .color(t.on_surface)
            .wrap(3)
            .padding(space::SM)
            .background(t.surface_container)
        };
        image
            .size(token::IMAGE_WIDTH, token::IMAGE_HEIGHT)
            .radius(radius::CONTROL)
            .clip()
    }
    fn card(
        &self,
        note: &Notification,
        history: bool,
        detailed: bool,
        t: Theme,
        now: f64,
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
        if let Some(target) = note.target()
            && note.image.is_some()
        {
            children.push(self.image(
                &super::artwork::key(target, "image"),
                "Notification image",
                t,
                now,
            ));
        }
        if !note.body.is_empty()
            && let Some((target, body)) = note
                .target()
                .and_then(|target| self.bodies.get(&target).map(|body| (target, body)))
        {
            for (index, block) in body.blocks.iter().enumerate() {
                let element = match block {
                    BodyBlock::Text(spans)
                        if spans.iter().all(|span| span.style == BodyStyle::default()) =>
                    {
                        Element::text(
                            spans
                                .iter()
                                .map(|span| span.text.as_str())
                                .collect::<String>(),
                        )
                    }
                    BodyBlock::Text(spans) => Element::rich_text(
                        spans
                            .iter()
                            .map(|span| {
                                let mut run = TextSpan::new(&span.text)
                                    .face(match (span.style.bold, span.style.italic) {
                                        (true, true) => 4,
                                        (true, false) => 2,
                                        (false, true) => 3,
                                        _ => 0,
                                    })
                                    .underline(span.style.underline || span.style.link.is_some());
                                if let Some(link) = &span.style.link {
                                    run = run.color(t.info).link(link.value());
                                }
                                run
                            })
                            .collect(),
                    )
                    .on_link(move |key| Message::OpenLink(target, key.into())),
                    BodyBlock::Image { alt, .. } => {
                        children.push(
                            Element::column(vec![
                                self.image(
                                    &super::artwork::key(target, &format!("body-{index}")),
                                    alt,
                                    t,
                                    now,
                                ),
                                Element::text(alt)
                                    .font(font::CAPTION)
                                    .color(t.on_surface)
                                    .width(Length::Fill)
                                    .wrap(2),
                            ])
                            .gap(space::XS)
                            .width(Length::Fill),
                        );
                        continue;
                    }
                };
                children.push(
                    element
                        .id(format!(
                            "notification-body-{}-{}-{index}",
                            note.id, note.revision
                        ))
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
        }
        if note
            .target()
            .is_some_and(|target| self.artwork.failed_for(target))
        {
            children.push(t.button("Retry images", Message::RetryArtwork).id(format!(
                "notification-images-retry-{}-{}",
                note.id, note.revision
            )));
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
                    NotificationHistoryStatus::Loading => "Loading saved history…",
                    NotificationHistoryStatus::Saving => "Saving history…",
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
                        rows.push(self.card(note, *history, true, t, cx.now));
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
            let room = (cx.height - top - token::MARGIN).max(0.);
            let rich = self
                .snapshot
                .active
                .iter()
                .any(|note| self.has_images(note));
            let card_height = if rich {
                token::MAX_CARD_HEIGHT + token::IMAGE_HEIGHT
            } else {
                token::MAX_CARD_HEIGHT
            };
            let limit = (room / (card_height + token::GAP)).floor().max(1.) as usize;
            let cards = self
                .snapshot
                .active
                .iter()
                .filter(|n| !self.snapshot.do_not_disturb || n.critical)
                .take(limit.min(token::TOAST_LIMIT as usize))
                .map(|note| {
                    let card = self.card(note, false, false, t, cx.now);
                    if self.has_images(note)
                        && let Some(target) = note.target()
                    {
                        t.scroll(
                            ScrollSpec::new(
                                self.toast_scroll.get(&target).copied().unwrap_or(0.),
                                lucent_design::component::scroll::LINE_STEP,
                            )
                            .expect("controlled toast offset"),
                            card,
                            move |event| Message::ToastScroll(target, event),
                        )
                        .id(format!(
                            "notification-toast-scroll-{}-{}",
                            note.id, note.revision
                        ))
                        .padding(token::IMAGE_VIEWPORT_PADDING)
                        .width(Length::Fill)
                        .height(Length::Fixed(card_height.min(room)))
                        .radius(radius::PANEL)
                    } else {
                        card
                    }
                })
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
    fn has_images(&self, note: &Notification) -> bool {
        note.image.is_some()
            || note
                .target()
                .and_then(|target| self.bodies.get(&target))
                .is_some_and(|body| {
                    body.blocks
                        .iter()
                        .any(|block| matches!(block, BodyBlock::Image { .. }))
                })
    }
}
