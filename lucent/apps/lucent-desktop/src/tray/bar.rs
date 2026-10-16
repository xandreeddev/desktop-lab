//! Bar presentation and gesture policy. Geometry/input and native methods remain
//! in the SDK and injected adapters respectively.
use super::*;
use lucent_design::{
    Theme,
    component::{bar, tray},
    font, icon, radius, space,
};

#[derive(Clone)]
pub enum Action {
    Activate,
    Secondary,
    Menu,
    Wheel(WheelEvent),
}

pub(super) struct WheelDemand {
    pub item: TrayItemId,
    horizontal: f64,
    vertical: f64,
}
impl WheelDemand {
    fn add(&mut self, event: WheelEvent) {
        // Keep only one target and at most 64 unsent steps on each axis while
        // an application is busy. This is command admission, not a UI token.
        self.horizontal = (self.horizontal + event.horizontal).clamp(-64., 64.);
        self.vertical = (self.vertical + event.vertical).clamp(-64., 64.);
    }
    fn next(&mut self) -> Option<TrayAction> {
        for (horizontal, pending) in [(false, &mut self.vertical), (true, &mut self.horizontal)] {
            let delta = pending.trunc() as i32;
            if delta != 0 {
                *pending -= f64::from(delta);
                return Some(TrayAction::Scroll { delta, horizontal });
            }
        }
        None
    }
}

impl Tray {
    pub fn bar_count(&self) -> usize {
        self.snapshot
            .items
            .iter()
            .filter(|item| item.status != TrayStatus::Passive)
            .count()
    }

    pub fn bar_icons(&self, count: usize, theme: Theme) -> Vec<Element<Message>> {
        self.snapshot
            .items
            .iter()
            .filter(|item| item.status != TrayStatus::Passive)
            .take(count)
            .map(|item| {
                let id = item.id;
                let key = format!("tray:{}:base", id.value());
                let base = if let Some(image) = self.images.get(&key) {
                    Element::image(image.clone())
                        .contain()
                        .size(icon::TINY, icon::TINY)
                } else {
                    // An app without valid artwork still has a visible, actionable
                    // control. Titles are already bounded in the native adapter.
                    Element::text(
                        item.title
                            .chars()
                            .chain(item.application.chars())
                            .find(|c| !c.is_whitespace())
                            .unwrap_or('?')
                            .to_string(),
                    )
                    .font(font::CAPTION)
                    .color(theme.on_surface)
                    .align(Align::Center)
                    .size(icon::TINY, icon::TINY)
                };
                let mut artwork = vec![base.at(space::XS, space::XS)];
                if let Some(image) = self.images.get(&format!("tray:{}:overlay", id.value())) {
                    artwork.push(
                        Element::image(image.clone())
                            .contain()
                            .size(tray::BAR_OVERLAY_SIZE, tray::BAR_OVERLAY_SIZE)
                            .at(
                                bar::CONTROL_SIZE - space::XS - tray::BAR_OVERLAY_SIZE,
                                bar::CONTROL_SIZE - space::XS - tray::BAR_OVERLAY_SIZE,
                            ),
                    );
                }
                Element::stack(artwork)
                    .size(bar::CONTROL_SIZE, bar::CONTROL_SIZE)
                    .radius(radius::SMALL)
                    .background(if item.status == TrayStatus::NeedsAttention {
                        theme.warning.alpha(lucent_design::opacity::HOVER)
                    } else {
                        Color::TRANSPARENT
                    })
                    .on_click(Message::Bar(id, Action::Activate))
                    .on_secondary_click(Message::Bar(id, Action::Menu))
                    .on_middle_click(Message::Bar(id, Action::Secondary))
                    .on_wheel(move |event| Message::Bar(id, Action::Wheel(event)))
                    .id(format!("bar-tray-item-{}", id.value()))
            })
            .collect()
    }

    pub(super) fn bar_action(
        &mut self,
        id: TrayItemId,
        action: Action,
        effects: &mut Effects<Message>,
    ) {
        let Some(item) = self
            .snapshot
            .items
            .iter()
            .find(|item| item.id == id && item.status != TrayStatus::Passive)
        else {
            return;
        };
        if let Action::Wheel(event) = action {
            if !event.horizontal.is_finite()
                || !event.vertical.is_finite()
                || (self.active.is_some() && self.active != self.wheel_active)
            {
                return;
            }
            if self.wheel.as_ref().is_none_or(|demand| demand.item != id) {
                self.wheel = Some(WheelDemand {
                    item: id,
                    horizontal: 0.,
                    vertical: 0.,
                });
            }
            self.wheel.as_mut().unwrap().add(event);
            self.drain_wheel(effects);
            return;
        }
        if self.active.is_some() {
            return;
        }
        self.wheel = None;
        let menu = matches!(action, Action::Menu)
            || matches!(action, Action::Activate) && item.item_is_menu;
        if menu && item.has_menu {
            self.reset_menu();
            self.set_open(true, effects.now);
            self.update(Message::Menu(id, 0), effects);
        } else {
            self.set_open(false, effects.now);
            // SNI accepts a screen-position hint. Wayland layer surfaces do not
            // expose a global origin; retain the existing neutral (0,0) hint,
            // never mislabel surface-local coordinates as screen coordinates.
            let action = if menu {
                TrayAction::ContextMenu { x: 0, y: 0 }
            } else if matches!(action, Action::Secondary) {
                TrayAction::SecondaryActivate { x: 0, y: 0 }
            } else {
                TrayAction::Activate { x: 0, y: 0 }
            };
            self.start_action(id, action, effects);
        }
    }

    pub(super) fn activate(
        &mut self,
        id: TrayItemId,
        action: TrayAction,
        effects: &mut Effects<Message>,
    ) {
        let Some(item) = self.snapshot.items.iter().find(|item| item.id == id) else {
            return;
        };
        if matches!(action, TrayAction::Activate { .. }) && item.item_is_menu {
            if item.has_menu {
                self.update(Message::Menu(id, 0), effects);
            } else {
                self.start_action(id, TrayAction::ContextMenu { x: 0, y: 0 }, effects);
            }
        } else {
            self.start_action(id, action, effects);
        }
    }

    fn start_action(&mut self, id: TrayItemId, action: TrayAction, effects: &mut Effects<Message>) {
        let revision = self.next();
        self.target = Some(id);
        let adapter = self.adapter.clone();
        effects.task_on_with_failure(
            crate::work::TRAY,
            move || Message::Completed(revision, adapter.act(id, action)),
            move |e| Message::Completed(revision, Err(DomainError::Failed(e.to_string()))),
        );
    }

    pub(super) fn drain_wheel(&mut self, effects: &mut Effects<Message>) {
        if self.active.is_some() {
            return;
        }
        let Some(demand) = &mut self.wheel else {
            return;
        };
        if !self
            .snapshot
            .items
            .iter()
            .any(|item| item.id == demand.item && item.status != TrayStatus::Passive)
        {
            self.wheel = None;
            return;
        }
        let id = demand.item;
        if let Some(action) = demand.next() {
            self.start_action(id, action, effects);
            self.wheel_active = self.active;
        }
    }
}
