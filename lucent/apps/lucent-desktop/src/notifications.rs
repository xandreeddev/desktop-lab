//! Notification client state; delivery/history and actions use injected adapters.
mod artwork;
mod grouping;
#[cfg(test)]
mod tests;
mod view;
use lucent_api::*;
use lucent_design::{Theme, motion};
use lucent_domain::{
    ExternalUri, Notification, NotificationPort, NotificationSnapshot, NotificationTarget,
};
use lucent_usecases::notifications::body::{BodyBlock, NotificationBody, notification_body};
use std::sync::Arc;
#[derive(Clone)]
pub enum Message {
    Ignore,
    Snapshot(lucent_domain::Result<NotificationSnapshot>),
    Toggle,
    Hide,
    Settled,
    Show,
    DismissLast,
    DismissAll,
    InvokeLast,
    Dismiss(NotificationTarget),
    DismissId(u32),
    Invoke(NotificationTarget, String),
    OpenLink(NotificationTarget, String),
    LinkPrepared(u64, ActivationOutcome),
    LinkDone(u64, lucent_domain::Result<()>),
    Artwork(u64, crate::ports::Images),
    RetryArtwork,
    Dnd,
    Page(i32),
    ActionPage(NotificationTarget, i32),
    Clear,
    RetryHistory,
    Group(String),
    Scroll(ScrollEvent),
    ToastScroll(NotificationTarget, ScrollEvent),
    Done(lucent_domain::Result<()>),
}
struct LinkOpening {
    request: u64,
    target: NotificationTarget,
    selected: ExternalUri,
    waiting_for_token: bool,
    cancellation: Cancellation,
}
impl Drop for LinkOpening {
    fn drop(&mut self) {
        // Retirement can skip work still waiting on the controls queue. An
        // already-running external application operation cannot be undone.
        self.cancellation.cancel();
    }
}
pub struct Center {
    pub snapshot: NotificationSnapshot,
    pub ready: bool,
    history_open: bool,
    reveal: Motion,
    closing: bool,
    page: usize,
    selected_group: Option<String>,
    scroll: f32,
    action_pages: std::collections::BTreeMap<NotificationTarget, usize>,
    error: bool,
    adapter: Arc<dyn NotificationPort>,
    uri: Arc<dyn lucent_domain::UriPort>,
    bodies: std::collections::BTreeMap<NotificationTarget, NotificationBody>,
    artwork: artwork::Artwork,
    toast_scroll: std::collections::BTreeMap<NotificationTarget, f32>,
    link_opening: Option<LinkOpening>,
    next_link_request: u64,
}
impl Center {
    pub fn new(
        adapter: Arc<dyn NotificationPort>,
        uri: Arc<dyn lucent_domain::UriPort>,
        assets: Arc<dyn crate::ports::AssetPort>,
    ) -> Self {
        Self {
            snapshot: NotificationSnapshot::default(),
            ready: false,
            history_open: false,
            reveal: Motion::fixed(0.),
            closing: false,
            page: 0,
            selected_group: None,
            scroll: 0.,
            action_pages: Default::default(),
            error: false,
            adapter,
            uri,
            bodies: Default::default(),
            artwork: artwork::Artwork::new(assets),
            toast_scroll: Default::default(),
            link_opening: None,
            next_link_request: 0,
        }
    }
    pub fn history_open(&self) -> bool {
        self.history_open
    }
    pub fn history_closing(&self) -> bool {
        self.closing
    }
    pub fn artwork_inspection(&self) -> serde_json::Value {
        self.artwork.inspect()
    }
    pub fn animating(&self, now: f64) -> bool {
        self.reveal.active(now) || (self.visible() && self.artwork.animating(now))
    }
    pub fn settled(&self, now: f64) -> bool {
        self.closing && !self.reveal.active(now)
    }
    fn set_open(&mut self, open: bool, now: f64) {
        self.history_open = open;
        self.closing = !open && self.reveal.value(now) > 0.;
        self.reveal.target(
            if open { 1. } else { 0. },
            now,
            if open { motion::ENTER } else { motion::EXIT },
            motion::DECELERATE,
        );
        if open {
            self.page = 0;
            self.scroll = 0.;
        }
    }
    pub fn visible(&self) -> bool {
        self.history_open
            || self.closing
            || self
                .snapshot
                .active
                .iter()
                .any(|n| !self.snapshot.do_not_disturb || n.critical)
    }
    fn selected_link(&self, target: NotificationTarget, key: &str) -> Option<ExternalUri> {
        let note = self
            .snapshot
            .active
            .iter()
            .chain(&self.snapshot.history)
            .find(|note| note.target() == Some(target))?;
        // The prepared view cache is not opening authority. Validate against
        // the current content again at the input and asynchronous completion.
        notification_body(&note.body)
            .blocks
            .iter()
            .find_map(|block| match block {
                BodyBlock::Text(spans) => spans
                    .iter()
                    .filter_map(|span| span.style.link.as_ref())
                    .find(|link| link.value() == key)
                    .cloned(),
                BodyBlock::Image { .. } => None,
            })
    }
}
impl Component for Center {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        self.view_with_theme(cx, Theme::new(false))
    }
    fn update(&mut self, message: Message, e: &mut Effects<Message>) {
        let adapter = self.adapter.clone();
        match message {
            Message::Ignore => (),
            Message::Artwork(revision, images) => self.artwork.complete(revision, images, e),
            Message::RetryArtwork => self.refresh_artwork(true, e),
            Message::Snapshot(Ok(value)) => {
                self.toast_scroll.retain(|target, _| {
                    value
                        .active
                        .iter()
                        .any(|note| note.target() == Some(*target))
                });
                self.bodies.retain(|target, _| {
                    value
                        .active
                        .iter()
                        .chain(&value.history)
                        .any(|note| note.target() == Some(*target))
                });
                for note in value.active.iter().chain(&value.history) {
                    if let Some(target) = note.target() {
                        self.bodies
                            .entry(target)
                            .or_insert_with(|| notification_body(&note.body));
                    }
                }
                self.action_pages
                    .retain(|target, _| value.active.iter().any(|n| n.target() == Some(*target)));
                self.snapshot = value;
                if self.link_opening.as_ref().is_some_and(|opening| {
                    self.selected_link(opening.target, opening.selected.value())
                        .as_ref()
                        != Some(&opening.selected)
                }) {
                    self.link_opening = None;
                }
                if self
                    .selected_group
                    .as_ref()
                    .is_some_and(|key| !self.groups().iter().any(|g| &g.key == key))
                {
                    self.selected_group = None;
                    self.page = 0;
                }
                self.ready = true;
            }
            Message::Snapshot(Err(_)) => self.ready = false,
            Message::Show => self.set_open(true, e.now),
            Message::Hide => self.set_open(false, e.now),
            Message::Settled => {
                if !self.history_open && self.settled(e.now) {
                    self.closing = false;
                }
            }
            Message::DismissLast => {
                if let Some(note) = self.snapshot.active.first()
                    && let Some(target) = note.target()
                {
                    self.update(Message::Dismiss(target), e);
                }
            }
            Message::DismissAll => {
                for target in self
                    .snapshot
                    .active
                    .iter()
                    .filter_map(Notification::target)
                    .collect::<Vec<_>>()
                {
                    self.update(Message::Dismiss(target), e);
                }
            }
            Message::InvokeLast => {
                if let Some(note) = self.snapshot.active.first()
                    && let Some(target) = note.target()
                    && let Some(action) = note.actions.first()
                {
                    self.update(Message::Invoke(target, action.id.clone()), e);
                }
            }
            Message::Toggle => self.set_open(!self.history_open, e.now),
            Message::Group(key) => {
                if self.groups().iter().any(|g| g.key == key) {
                    self.selected_group = if self.selected_group.as_ref() == Some(&key) {
                        None
                    } else {
                        Some(key)
                    };
                    self.page = 0;
                }
            }
            Message::Scroll(event) if event.offset.is_finite() && event.offset >= 0. => {
                self.scroll = event.offset
            }
            Message::Scroll(_) => {}
            Message::ToastScroll(target, event) => {
                if event.offset.is_finite()
                    && event.offset >= 0.
                    && self
                        .snapshot
                        .active
                        .iter()
                        .any(|note| note.target() == Some(target))
                {
                    self.toast_scroll.insert(target, event.offset);
                }
            }
            Message::Page(delta) => {
                let count = self
                    .groups()
                    .iter()
                    .find(|g| self.selected_group.as_ref() == Some(&g.key))
                    .map_or(0, |g| g.entries.len());
                self.page = (self.page as i64 + delta as i64)
                    .clamp(0, count.saturating_sub(1) as i64) as usize;
            }
            Message::ActionPage(target, delta) => {
                if let Some(note) = self
                    .snapshot
                    .active
                    .iter()
                    .find(|note| note.target() == Some(target))
                    && !note.actions.is_empty()
                {
                    let index = self.action_pages.entry(target).or_default();
                    *index = (*index as i64 + delta as i64).rem_euclid(note.actions.len() as i64)
                        as usize;
                }
            }
            Message::DismissId(id) => {
                if let Some(target) = self
                    .snapshot
                    .active
                    .iter()
                    .find(|n| n.id == id)
                    .and_then(Notification::target)
                {
                    self.update(Message::Dismiss(target), e);
                }
            }
            Message::Dismiss(target) => {
                if self.ready
                    && self
                        .snapshot
                        .active
                        .iter()
                        .any(|note| note.target() == Some(target))
                {
                    e.task_on_with_failure(
                        crate::work::CONTROLS,
                        move || Message::Done(adapter.dismiss(target)),
                        |_| {
                            Message::Done(Err(lucent_domain::DomainError::Failed(
                                "Notification action failed".into(),
                            )))
                        },
                    );
                } else {
                    self.error = true;
                }
            }
            Message::Invoke(target, action) => {
                if self.ready
                    && self.snapshot.active.iter().any(|note| {
                        note.target() == Some(target) && note.actions.iter().any(|a| a.id == action)
                    })
                {
                    e.task_on_with_failure(
                        crate::work::CONTROLS,
                        move || Message::Done(adapter.invoke(target, &action)),
                        |_| {
                            Message::Done(Err(lucent_domain::DomainError::Failed(
                                "Notification action failed".into(),
                            )))
                        },
                    );
                } else {
                    self.error = true;
                }
            }
            Message::OpenLink(target, key) => {
                // A history link opens its content URI; it never invokes the old
                // sender. Capture this exact version before asynchronous token
                // delivery; the target default application is not known here.
                if let Some(selected) = self.selected_link(target, &key) {
                    if let Some(request) = self.next_link_request.checked_add(1) {
                        self.next_link_request = request;
                        self.link_opening = Some(LinkOpening {
                            request,
                            target,
                            selected,
                            waiting_for_token: true,
                            cancellation: Cancellation::default(),
                        });
                        e.activation_token(None, move |outcome| {
                            Message::LinkPrepared(request, outcome)
                        });
                    } else {
                        self.link_opening = None;
                        self.error = true;
                    }
                } else {
                    self.error = true;
                }
            }
            Message::LinkPrepared(request, outcome) => {
                let current = self
                    .link_opening
                    .as_mut()
                    .filter(|opening| opening.request == request && opening.waiting_for_token)
                    .map(|opening| {
                        opening.waiting_for_token = false;
                        (
                            opening.target,
                            opening.selected.clone(),
                            opening.cancellation.clone(),
                        )
                    });
                if let Some((target, selected, cancellation)) = current {
                    if self.selected_link(target, selected.value()).as_ref() == Some(&selected) {
                        let uri = self.uri.clone();
                        let context = crate::application_requests::launch_context(outcome);
                        e.task_cancellable_on_with_failure(
                            crate::work::CONTROLS,
                            cancellation,
                            move || Message::LinkDone(request, uri.open(&selected, context)),
                            move |_| {
                                Message::LinkDone(
                                    request,
                                    Err(lucent_domain::DomainError::Failed(
                                        "Link could not be opened".into(),
                                    )),
                                )
                            },
                        );
                    } else {
                        self.link_opening = None;
                    }
                }
            }
            Message::LinkDone(request, result) => {
                if self
                    .link_opening
                    .as_ref()
                    .is_some_and(|opening| opening.request == request && !opening.waiting_for_token)
                {
                    self.link_opening = None;
                    self.error = result.is_err();
                }
            }
            Message::Dnd => {
                let enabled = !self.snapshot.do_not_disturb;
                e.task_on_with_failure(
                    crate::work::CONTROLS,
                    move || Message::Done(adapter.set_do_not_disturb(enabled)),
                    |_| {
                        Message::Done(Err(lucent_domain::DomainError::Failed(
                            "Notification preference failed".into(),
                        )))
                    },
                );
            }
            Message::RetryHistory => e.task_on_with_failure(
                crate::work::CONTROLS,
                move || Message::Done(adapter.retry_history()),
                |_| {
                    Message::Done(Err(lucent_domain::DomainError::Failed(
                        "History retry failed".into(),
                    )))
                },
            ),
            Message::Clear => {
                self.link_opening = None;
                e.task_on_with_failure(
                    crate::work::CONTROLS,
                    move || Message::Done(adapter.clear_history()),
                    |_| {
                        Message::Done(Err(lucent_domain::DomainError::Failed(
                            "History clear failed".into(),
                        )))
                    },
                );
            }
            Message::Done(result) => self.error = result.is_err(),
        }
        self.refresh_artwork(false, e);
        e.redraw("notifications");
        e.redraw("bar");
    }
}
