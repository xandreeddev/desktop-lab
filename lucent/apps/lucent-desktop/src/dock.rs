//! Dock policy, transient gestures and visibility. OS operations stay behind root use cases.
use lucent_api::*;
use lucent_design::{component, motion};
use lucent_domain::Application;
use lucent_domain::*;
use std::time::Duration;
#[derive(Clone)]
pub enum Message {
    Pin(AppId),
    SetPin(AppId, bool),
    Press,
    Move(AppId, Option<AppId>),
    Drag(AppId, DragEvent),
    AutoHide(bool),
    Pointer(bool),
    Forced(bool),
    Hide(u64),
    Cancel,
}
pub enum Intent {
    Save,
    Error(String),
}
#[derive(Clone, Copy)]
pub struct Input<'a> {
    pub apps: &'a [Application],
    pub compositor: &'a CompositorSnapshot,
    pub settings: &'a DesktopSettings,
}
struct Drag {
    app: AppId,
    pins: Vec<AppId>,
    visible: Vec<AppId>,
    origin: usize,
    destination: usize,
    dx: f32,
}
pub struct Dock {
    cancelled: bool,
    drag: Option<Drag>,
    inside: bool,
    forced: bool,
    hide_generation: u64,
    hide_pending: bool,
    motion: Motion,
}
impl Default for Dock {
    fn default() -> Self {
        Self {
            cancelled: false,
            drag: None,
            inside: false,
            forced: false,
            hide_generation: 0,
            hide_pending: false,
            motion: Motion::fixed(1.),
        }
    }
}
impl Dock {
    pub fn defaults(apps: &[Application]) -> Vec<AppId> {
        let mut result = vec![];
        for query in [
            "foot", "firefox", "chromium", "nautilus", "spotify", "aether",
        ] {
            if let Some(app) = apps
                .iter()
                .find(|app| {
                    app.id
                        .as_str()
                        .eq_ignore_ascii_case(&format!("{query}.desktop"))
                })
                .or_else(|| {
                    apps.iter()
                        .find(|app| app.id.as_str().to_lowercase().contains(query))
                })
                && !result.contains(&app.id)
            {
                result.push(app.id.clone());
            }
        }
        result
    }
    pub fn pins(settings: &DesktopSettings, apps: &[Application]) -> Vec<AppId> {
        lucent_usecases::dock::effective_pins(settings, &Self::defaults(apps))
    }
    pub fn apps<'a>(&self, input: Input<'a>, capacity: usize) -> Vec<&'a Application> {
        let pins = Self::pins(input.settings, input.apps);
        let mut apps: Vec<_> = pins
            .iter()
            .filter_map(|id| input.apps.iter().find(|a| &a.id == id))
            .collect();
        for window in &input.compositor.windows {
            if let Some(app) = input
                .apps
                .iter()
                .find(|app| app.matches_window_class(&window.app_class))
                && !apps.iter().any(|a| a.id == app.id)
            {
                apps.push(app);
            }
        }
        apps.truncate(capacity.min(component::dock::MAX_APPS as usize));
        apps
    }
    #[cfg(test)]
    pub(super) fn fixture_hidden(&mut self) {
        self.motion = Motion::fixed(0.);
        self.hide_pending = false;
    }
    pub fn progress(&self, now: f64) -> f32 {
        self.motion.value(now)
    }
    pub fn animating(&self, now: f64) -> bool {
        self.motion.active(now)
    }
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
    fn reconcile(&mut self, auto_hide: bool, now: f64) {
        let show = !auto_hide || self.inside || self.forced || self.drag.is_some();
        if show {
            self.hide_pending = false;
            self.hide_generation = self.hide_generation.wrapping_add(1);
            self.motion
                .target(1., now, motion::ENTER, motion::DECELERATE);
        } else {
            self.hide_pending = true;
            self.hide_generation = self.hide_generation.wrapping_add(1);
        }
    }
    pub fn update(
        &mut self,
        message: Message,
        settings: &mut DesktopSettings,
        apps: &[Application],
        visible: &[AppId],
        effects: &mut Effects<Message>,
    ) -> Option<Intent> {
        let result = match message {
            Message::Press => {
                self.cancelled = false;
                self.drag = None;
                Ok(false)
            }
            Message::SetPin(id, pinned) => {
                let pins = Self::pins(settings, apps);
                lucent_usecases::dock::set_pin(settings, &id, pinned, &pins, apps)
            }
            Message::Pin(id) => {
                let pins = Self::pins(settings, apps);
                lucent_usecases::dock::set_pin(settings, &id, !pins.contains(&id), &pins, apps)
            }
            Message::Move(id, before) => {
                let pins = Self::pins(settings, apps);
                lucent_usecases::dock::move_pin(settings, &id, before.as_ref(), &pins, apps)
            }
            Message::AutoHide(value) => {
                let changed = settings.dock.auto_hide != value;
                settings.dock.auto_hide = value;
                self.reconcile(value, effects.now);
                Ok(changed)
            }
            Message::Pointer(inside) => {
                if self.inside != inside {
                    self.inside = inside;
                    self.reconcile(settings.dock.auto_hide, effects.now);
                }
                Ok(false)
            }
            Message::Forced(forced) => {
                if self.forced != forced {
                    self.forced = forced;
                    self.reconcile(settings.dock.auto_hide, effects.now);
                }
                Ok(false)
            }
            Message::Hide(id) => {
                if self.hide_pending
                    && self.hide_generation == id
                    && !self.inside
                    && !self.forced
                    && self.drag.is_none()
                    && settings.dock.auto_hide
                {
                    self.hide_pending = false;
                    self.motion
                        .target(0., effects.now, motion::EXIT, motion::DECELERATE);
                }
                Ok(false)
            }
            Message::Cancel => {
                self.cancelled = true;
                self.drag = None;
                self.reconcile(settings.dock.auto_hide, effects.now);
                Ok(false)
            }
            Message::Drag(id, event) => {
                if self.cancelled {
                    return None;
                }
                if !event.dx.is_finite()
                    || !event.dy.is_finite()
                    || !apps.iter().any(|app| app.id == id)
                {
                    self.drag = None;
                    return None;
                }
                if self.drag.is_none() {
                    let origin = visible.iter().position(|app| app == &id)?;
                    self.drag = Some(Drag {
                        app: id.clone(),
                        pins: Self::pins(settings, apps),
                        visible: visible.to_vec(),
                        origin,
                        destination: origin,
                        dx: event.dx,
                    });
                    self.reconcile(settings.dock.auto_hide, effects.now);
                }
                let drag = self.drag.as_mut().filter(|drag| drag.app == id)?;
                drag.dx = event.dx;
                let step = component::dock_row::ITEM_SIZE + lucent_design::space::SM;
                drag.destination = (drag.origin as f64 + (event.dx / step).round() as f64)
                    .clamp(0., drag.visible.len().saturating_sub(1) as f64)
                    as usize;
                if event.finished {
                    let drag = self.drag.take().unwrap();
                    let mut preview = drag.visible.clone();
                    preview.remove(drag.origin);
                    preview.insert(drag.destination, drag.app.clone());
                    let before = preview
                        .iter()
                        .skip(drag.destination + 1)
                        .find(|id| drag.pins.contains(id));
                    let changed = if drag.destination != drag.origin {
                        lucent_usecases::dock::move_pin(
                            settings, &drag.app, before, &drag.pins, apps,
                        )
                    } else {
                        Ok(false)
                    };
                    self.reconcile(settings.dock.auto_hide, effects.now);
                    changed
                } else {
                    Ok(false)
                }
            }
        };
        effects.redraw("dock");
        match result {
            Ok(true) => Some(Intent::Save),
            Ok(false) => None,
            Err(error) => Some(Intent::Error(error.to_string())),
        }
    }
    /// Keep source hit geometry stable throughout capture; only presentation moves.
    pub fn offset(&self, id: &AppId, index: usize) -> f32 {
        let Some(drag) = &self.drag else {
            return 0.;
        };
        if &drag.app == id {
            return drag.dx;
        }
        let step = component::dock_row::ITEM_SIZE + lucent_design::space::SM;
        if drag.destination > drag.origin && index > drag.origin && index <= drag.destination {
            -step
        } else if drag.destination < drag.origin && index >= drag.destination && index < drag.origin
        {
            step
        } else {
            0.
        }
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        if self.hide_pending {
            let generation = self.hide_generation;
            vec![Subscription::stream(
                format!("dock-hide-{generation}"),
                move |out, cancel| {
                    cancel.sleep(Duration::from_secs_f32(component::dock::HIDE_DELAY));
                    if !cancel.cancelled() {
                        out.send(Message::Hide(generation));
                    }
                },
            )]
        } else {
            vec![]
        }
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"inside":self.inside,"forced":self.forced,"dragging":self.drag.is_some(),"hide_pending":self.hide_pending,"visibility_target":self.motion.target_value()})
    }
}
#[cfg(test)]
mod tests;
