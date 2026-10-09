//! Notification presentation; delivery and action execution are injected.
use lucent_api::*;
use lucent_design::{Theme, component::notification as token, font, radius, space};
use lucent_domain::{Notification, NotificationPort, NotificationSnapshot};
use std::sync::Arc;
#[derive(Clone)]
pub enum Message {
    Snapshot(lucent_domain::Result<NotificationSnapshot>),
    Toggle,
    Dismiss(u32),
    Invoke(u32, String),
    Dnd,
    Page(i32),
    ActionPage(u32, i32),
    Clear,
    Done(lucent_domain::Result<()>),
}
pub struct Center {
    pub snapshot: NotificationSnapshot,
    pub ready: bool,
    pub history_open: bool,
    page: usize,
    action_pages: std::collections::BTreeMap<u32, usize>,
    error: bool,
    port: Arc<dyn NotificationPort>,
}
impl Center {
    pub fn new(port: Arc<dyn NotificationPort>) -> Self {
        Self {
            snapshot: NotificationSnapshot::default(),
            ready: false,
            history_open: false,
            page: 0,
            action_pages: Default::default(),
            error: false,
            port,
        }
    }
    pub fn visible(&self) -> bool {
        self.history_open
            || self
                .snapshot
                .active
                .iter()
                .any(|n| !self.snapshot.do_not_disturb || n.critical)
    }
    fn card(&self, note: &Notification, history: bool, t: Theme) -> Element<Message> {
        let mut children = vec![
            Element::row(vec![
                Element::text(&note.app)
                    .font(font::CAPTION)
                    .color(t.primary)
                    .width(Length::Fill),
                if history {
                    Element::empty()
                } else {
                    t.button("×", Message::Dismiss(note.id))
                        .id(format!("notification-dismiss-{}", note.id))
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
                    .wrap(token::BODY_LINES as usize),
            );
        }
        if !history && !note.actions.is_empty() {
            let index = self.action_pages.get(&note.id).copied().unwrap_or(0) % note.actions.len();
            let action = &note.actions[index];
            let mut buttons = vec![
                t.button(&action.label, Message::Invoke(note.id, action.id.clone()))
                    .width(Length::Fill)
                    .id(format!("notification-action-{}-{}", note.id, action.id)),
            ];
            if note.actions.len() > 1 {
                buttons.push(
                    t.button("Next action", Message::ActionPage(note.id, 1))
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
        let mut cards = vec![];
        if self.history_open {
            cards.push(
                Element::column(vec![
                    Element::text("Notifications")
                        .font(font::TITLE)
                        .color(t.on_surface),
                    Element::row(vec![
                        t.button(
                            if self.snapshot.do_not_disturb {
                                "DND on"
                            } else {
                                "DND off"
                            },
                            Message::Dnd,
                        ),
                        t.button("Clear", Message::Clear),
                        t.button("Close", Message::Toggle),
                    ])
                    .gap(space::XS),
                    Element::text(if !self.ready {
                        "Waiting for the notification service"
                    } else if self.error {
                        "That action is no longer available"
                    } else {
                        "Recent activity"
                    })
                    .font(font::CAPTION)
                    .color(t.on_surface)
                    .wrap(2),
                ])
                .gap(space::SM)
                .padding(token::PADDING)
                .width(Length::Fill)
                .background(t.surface)
                .radius(radius::PANEL),
            );
        }
        if self.history_open {
            let notes: Vec<_> = self
                .snapshot
                .active
                .iter()
                .map(|n| (n, false))
                .chain(self.snapshot.history.iter().map(|n| (n, true)))
                .collect();
            let page = self.page.min(notes.len().saturating_sub(1));
            if let Some((note, history)) = notes.get(page) {
                cards.push(self.card(note, *history, t));
            }
            if notes.len() > 1 {
                cards.push(
                    Element::row(vec![
                        t.button("Previous", Message::Page(-1)),
                        Element::text(format!("{} / {}", page + 1, notes.len()))
                            .font(font::CAPTION)
                            .color(t.on_surface)
                            .width(Length::Fill)
                            .align(Align::Center),
                        t.button("Next", Message::Page(1)),
                    ])
                    .width(Length::Fill)
                    .align(Align::Center)
                    .padding(space::SM)
                    .gap(space::XS)
                    .background(t.surface)
                    .radius(radius::CONTROL),
                );
            }
        } else {
            let room = cx.height - lucent_design::component::panel::BAR_HEIGHT - token::MARGIN * 2.;
            let limit = (room / (token::MAX_CARD_HEIGHT + token::GAP))
                .floor()
                .max(1.) as usize;
            for note in self
                .snapshot
                .active
                .iter()
                .filter(|n| !self.snapshot.do_not_disturb || n.critical)
                .take(limit.min(token::TOAST_LIMIT as usize))
            {
                cards.push(self.card(note, false, t));
            }
        }
        let width = token::WIDTH.min(cx.width - token::MARGIN * 2.);
        Element::stack(vec![
            Element::column(cards)
                .gap(token::GAP)
                .width(Length::Fixed(width))
                .at(
                    cx.width - width - token::MARGIN,
                    lucent_design::component::panel::BAR_HEIGHT + token::MARGIN,
                ),
        ])
        .fill()
    }
}
impl Component for Center {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        self.view_with_theme(cx, Theme::new(false))
    }
    fn update(&mut self, message: Message, e: &mut Effects<Message>) {
        let port = self.port.clone();
        match message {
            Message::Snapshot(Ok(value)) => {
                self.action_pages
                    .retain(|id, _| value.active.iter().any(|n| n.id == *id));
                self.snapshot = value;
                self.ready = true;
            }
            Message::Snapshot(Err(_)) => self.ready = false,
            Message::Toggle => {
                self.history_open = !self.history_open;
                self.page = 0;
            }
            Message::Page(delta) => {
                self.page = (self.page as i32 + delta).clamp(
                    0,
                    (self.snapshot.active.len() + self.snapshot.history.len()).saturating_sub(1)
                        as i32,
                ) as usize
            }
            Message::ActionPage(id, delta) => {
                let index = self.action_pages.entry(id).or_default();
                *index = (*index as i32 + delta).max(0) as usize;
            }
            Message::Dismiss(id) => e.task(move || Message::Done(port.dismiss(id))),
            Message::Invoke(id, action) => e.task(move || Message::Done(port.invoke(id, &action))),
            Message::Dnd => {
                let enabled = !self.snapshot.do_not_disturb;
                e.task(move || Message::Done(port.set_do_not_disturb(enabled)));
            }
            Message::Clear => e.task(move || Message::Done(port.clear_history())),
            Message::Done(result) => self.error = result.is_err(),
        }
        e.redraw("notifications");
        e.redraw("bar");
    }
}
