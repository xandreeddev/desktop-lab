//! Native tray client: injected ports, current identities and application-owned menus.
mod bar;
mod view;
use crate::ports::{AssetPort, Images};
pub use bar::Action as BarAction;
use lucent_api::*;
use lucent_design::motion;
use lucent_domain::*;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
#[derive(Clone)]
pub enum Message {
    Observed(Result<TraySnapshot>),
    Open,
    Toggle,
    Close,
    Closed(u64),
    Scroll(ScrollEvent),
    Act(TrayItemId, TrayAction),
    Bar(TrayItemId, BarAction),
    Menu(TrayItemId, i32),
    Invoke(i32),
    Back,
    Completed(u64, Result<()>),
    Loaded(u64, Result<TrayMenu>),
    Icons(u64, Images),
    Ignore,
}
pub struct Tray {
    adapter: Arc<dyn TrayPort>,
    assets: Arc<dyn AssetPort>,
    snapshot: TraySnapshot,
    status: String,
    ready: bool,
    pub(super) open: bool,
    mounted: bool,
    motion: Motion,
    close_generation: u64,
    revision: u64,
    active: Option<u64>,
    target: Option<TrayItemId>,
    menu: Option<TrayMenu>,
    parents: Vec<i32>,
    scroll: f32,
    error: String,
    icon_revision: u64,
    icon_active: Option<u64>,
    icon_pending: Option<TraySnapshot>,
    images: BTreeMap<String, Arc<ImageData>>,
    wheel: Option<bar::WheelDemand>,
    wheel_active: Option<u64>,
}
impl Tray {
    pub fn new(adapter: Arc<dyn TrayPort>, assets: Arc<dyn AssetPort>) -> Self {
        Self {
            adapter,
            assets,
            snapshot: Default::default(),
            status: "Connecting to tray applications…".into(),
            ready: false,
            open: false,
            mounted: false,
            motion: Motion::fixed(0.),
            close_generation: 0,
            revision: 0,
            active: None,
            target: None,
            menu: None,
            parents: vec![],
            scroll: 0.,
            error: String::new(),
            icon_revision: 0,
            icon_active: None,
            icon_pending: None,
            images: BTreeMap::new(),
            wheel: None,
            wheel_active: None,
        }
    }
    #[cfg(test)]
    pub(super) fn load_fixture_menu(&mut self, menu: TrayMenu) {
        self.target = Some(menu.item);
        self.menu = Some(menu);
        self.parents = vec![0];
    }
    pub fn open(&self) -> bool {
        self.open
    }
    pub fn visible(&self) -> bool {
        self.mounted
    }
    pub fn animating(&self, now: f64) -> bool {
        self.motion.active(now)
    }
    fn reset_menu(&mut self) {
        self.target = None;
        self.menu = None;
        self.parents.clear();
        self.scroll = 0.;
        self.revision = self.revision.wrapping_add(1);
        self.active = None;
        self.wheel = None;
        self.wheel_active = None;
    }
    fn set_open(&mut self, open: bool, now: f64) {
        if self.open == open {
            return;
        }
        self.open = open;
        self.mounted = true;
        if !open {
            self.close_generation = self.close_generation.wrapping_add(1);
            self.reset_menu();
        }
        self.motion.target(
            if open { 1. } else { 0. },
            now,
            if open { motion::ENTER } else { motion::EXIT },
            motion::DECELERATE,
        );
    }
    fn prepare_icons(&mut self, snapshot: TraySnapshot, effects: &mut Effects<Message>) {
        if self.icon_active.is_some() {
            self.icon_revision = self.icon_revision.wrapping_add(1);
            self.icon_pending = Some(snapshot);
            return;
        }
        self.icon_revision = self.icon_revision.wrapping_add(1);
        let revision = self.icon_revision;
        self.icon_active = Some(revision);
        let assets = self.assets.clone();
        effects.task_on_with_failure(
            crate::work::ASSETS,
            move || Message::Icons(revision, assets.tray_icons(&snapshot.items)),
            move |_| Message::Icons(revision, vec![]),
        );
    }
    fn next(&mut self) -> u64 {
        self.revision = self.revision.wrapping_add(1);
        self.active = Some(self.revision);
        self.error.clear();
        self.revision
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Observed(Ok(snapshot)) => {
                if self.wheel.as_ref().is_some_and(|demand| {
                    !snapshot
                        .items
                        .iter()
                        .any(|item| item.id == demand.item && item.status != TrayStatus::Passive)
                }) {
                    self.wheel = None;
                }
                if self
                    .target
                    .is_some_and(|id| !snapshot.items.iter().any(|i| i.id == id))
                {
                    self.reset_menu();
                }
                if let Some(menu) = &self.menu {
                    let previous = self.snapshot.items.iter().find(|i| i.id == menu.item);
                    let current = snapshot.items.iter().find(|i| i.id == menu.item);
                    if previous
                        .zip(current)
                        .is_none_or(|(a, b)| a.menu_generation != b.menu_generation)
                    {
                        self.reset_menu();
                    }
                }
                if self
                    .snapshot
                    .items
                    .iter()
                    .map(|i| (i.id, &i.icon, &i.attention_icon, &i.overlay_icon, i.status))
                    .ne(snapshot
                        .items
                        .iter()
                        .map(|i| (i.id, &i.icon, &i.attention_icon, &i.overlay_icon, i.status)))
                {
                    self.prepare_icons(snapshot.clone(), effects);
                }
                self.snapshot = snapshot;
                self.ready = true;
                self.status.clear();
            }
            Message::Observed(Err(error)) => {
                self.ready = false;
                self.status = error.to_string();
                self.snapshot = Default::default();
                self.images.clear();
                self.icon_pending = None;
                self.icon_revision = self.icon_revision.wrapping_add(1);
                self.reset_menu();
            }
            Message::Open => self.set_open(true, effects.now),
            Message::Toggle => self.set_open(!self.open, effects.now),
            Message::Close => {
                // Also retire an action started directly from a bar icon while
                // no panel was open. Late failures must not reopen a dismissed UI.
                if self.open {
                    self.set_open(false, effects.now);
                } else {
                    self.reset_menu();
                }
            }
            Message::Closed(id) => {
                if !self.open && id == self.close_generation {
                    self.mounted = false;
                }
            }
            Message::Scroll(event) => self.scroll = event.offset,
            Message::Back => {
                if self.parents.len() > 1 {
                    self.parents.pop();
                    self.scroll = 0.;
                } else {
                    self.reset_menu();
                }
            }
            Message::Act(id, action) => {
                if !self.open
                    || self.active.is_some()
                    || !self.snapshot.items.iter().any(|i| i.id == id)
                {
                    return;
                }
                self.activate(id, action, effects);
            }
            Message::Bar(id, action) => self.bar_action(id, action, effects),
            Message::Menu(id, parent) => {
                if !self.open
                    || self.active.is_some()
                    || !self.snapshot.items.iter().any(|i| i.id == id && i.has_menu)
                {
                    return;
                }
                let revision = self.next();
                self.target = Some(id);
                let adapter = self.adapter.clone();
                effects.task_on_with_failure(
                    crate::work::TRAY,
                    move || Message::Loaded(revision, adapter.menu(id, parent)),
                    move |e| Message::Loaded(revision, Err(DomainError::Failed(e.to_string()))),
                );
                if parent == 0 {
                    self.parents = vec![0];
                } else {
                    self.parents.push(parent);
                }
                self.scroll = 0.;
            }
            Message::Invoke(id) => {
                if !self.open || self.active.is_some() {
                    return;
                }
                let Some(menu) = self.menu.as_ref() else {
                    return;
                };
                let Some(entry) = lucent_usecases::tray::menu_entry(&menu.root, id) else {
                    return;
                };
                if id == menu.root.id || !entry.enabled || !entry.visible || entry.separator {
                    return;
                }
                if entry.is_submenu() {
                    self.update(Message::Menu(menu.item, id), effects);
                    return;
                }
                if let Err(error) =
                    lucent_usecases::tray::validate_menu_action(menu, menu.revision, id)
                {
                    self.error = error.to_string();
                    return;
                }
                let item = menu.item;
                let menu_revision = menu.revision;
                let revision = self.next();
                let adapter = self.adapter.clone();
                effects.task_on_with_failure(
                    crate::work::TRAY,
                    move || {
                        Message::Completed(revision, adapter.invoke(item, menu_revision, id, 0))
                    },
                    move |e| Message::Completed(revision, Err(DomainError::Failed(e.to_string()))),
                );
            }
            Message::Completed(revision, result) if self.active == Some(revision) => {
                self.active = None;
                let scrolling = self.wheel_active.take() == Some(revision);
                if let Err(error) = result {
                    self.wheel = None;
                    self.error = error.to_string();
                    self.menu = None;
                    self.parents.clear();
                    self.set_open(true, effects.now);
                } else if scrolling {
                    self.drain_wheel(effects);
                } else if self.menu.is_some() {
                    self.set_open(false, effects.now);
                }
            }
            Message::Loaded(revision, result) if self.active == Some(revision) => {
                self.active = None;
                match result {
                    Ok(menu) if self.target == Some(menu.item) => self.menu = Some(menu),
                    Ok(_) => (),
                    Err(error) => {
                        self.error = error.to_string();
                        self.menu = None;
                        self.parents.clear();
                    }
                }
            }
            Message::Icons(revision, images) if self.icon_active == Some(revision) => {
                self.icon_active = None;
                if revision == self.icon_revision && self.ready {
                    self.images = images.into_iter().collect();
                }
                if let Some(snapshot) = self.icon_pending.take() {
                    self.prepare_icons(snapshot, effects);
                }
            }
            Message::Completed(..) | Message::Loaded(..) | Message::Icons(..) | Message::Ignore => {
            }
        }
        effects.redraw("tray");
        effects.redraw("bar");
    }
    pub fn escape(&self) -> Message {
        if self.menu.is_some() {
            Message::Back
        } else {
            Message::Close
        }
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        if self.mounted && !self.open {
            let generation = self.close_generation;
            vec![Subscription::stream(
                format!("tray-close-{generation}"),
                move |out, cancel| {
                    cancel.sleep(Duration::from_secs_f64(motion::EXIT));
                    if !cancel.cancelled() {
                        out.send(Message::Closed(generation));
                    }
                },
            )]
        } else {
            vec![]
        }
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"open":self.open,"visible":self.mounted,"ready":self.ready,"items":self.snapshot.items.len(),"menu":self.menu.is_some(),"busy":self.active.is_some()})
    }
}
#[cfg(test)]
pub(super) mod tests;
