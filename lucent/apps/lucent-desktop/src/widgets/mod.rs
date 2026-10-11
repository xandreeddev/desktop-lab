//! Widget interaction belongs to this child; persistence and OS actions return
//! as intents to the composition root. Views only borrow snapshots and artwork.
pub mod catalog;
mod view;
use crate::actions::Action;
use lucent_api::{DragEvent, Effects};
use lucent_design::component;
use lucent_domain::{DesktopSettings, FocusTimer, Placement};
use std::collections::BTreeMap;
pub use view::{Input, View};

#[derive(Clone)]
pub enum Message {
    Toggle(String),
    Show(String),
    Move(String, DragEvent),
    Notes(String),
    RetrySave,
    TimerToggle,
    TimerReset,
    Tick(u64),
    Month(i32),
    ResetLayout,
    Action(Action),
}
pub enum Intent {
    Save,
    RetrySave,
    Action(Action),
    Error(lucent_domain::DomainError),
}
/// Preferences remain in the root aggregate. The child borrows them for this
/// update rather than maintaining a second persisted copy.
pub struct Update<'a> {
    pub settings: &'a mut DesktopSettings,
    pub viewport: (f32, f32),
    pub unix_seconds: u64,
}
#[derive(Clone, Copy)]
enum Drag {
    Active {
        origin: Placement,
        preview: Placement,
    },
    // Ignore the rest of a gesture after hide/reset/invalid input. Its final
    // release removes this marker; a subsequent press can start a new drag.
    Cancelled,
}
#[derive(Default)]
pub struct Host {
    drags: BTreeMap<String, Drag>,
    timer: FocusTimer,
    month_offset: i32,
}
impl Host {
    pub fn grid_visible(&self) -> bool {
        self.drags
            .values()
            .any(|drag| matches!(drag, Drag::Active { .. }))
    }
    pub fn preview_position(&self, id: &str) -> Option<Placement> {
        match self.drags.get(id) {
            Some(Drag::Active { preview, .. }) => Some(*preview),
            _ => None,
        }
    }
    fn cancel_drag(&mut self, id: &str, finished: bool) {
        if finished {
            self.drags.remove(id);
        } else {
            self.drags.insert(id.into(), Drag::Cancelled);
        }
    }
    pub fn timer_remaining(&self) -> u64 {
        self.timer.remaining
    }
    #[cfg(test)]
    pub fn month_offset(&self) -> i32 {
        self.month_offset
    }
    pub fn update(
        &mut self,
        message: Message,
        input: Update<'_>,
        effects: &mut Effects<Message>,
    ) -> Option<Intent> {
        let settings = input.settings;
        match message {
            Message::Toggle(id) | Message::Show(id) if catalog::find(&id).is_none() => None,
            Message::Show(id) if settings.visible_widgets.contains(&id) => None,
            Message::Toggle(id) | Message::Show(id) => {
                if self.drags.contains_key(&id) {
                    self.cancel_drag(&id, false);
                }
                lucent_usecases::toggle_widget(settings, &id);
                effects.redraw("widgets");
                effects.redraw("dock");
                Some(Intent::Save)
            }
            Message::Move(id, drag) => {
                catalog::find(&id)?;
                if matches!(self.drags.get(&id), Some(Drag::Cancelled))
                    || !settings.visible_widgets.contains(&id)
                {
                    self.cancel_drag(&id, drag.finished);
                    effects.redraw("widgets");
                    return None;
                }
                // The framework emits progress before release. A release after
                // cancellation or without a matching gesture cannot commit.
                if drag.finished && !self.drags.contains_key(&id) {
                    return None;
                }
                if [input.viewport.0, input.viewport.1]
                    .iter()
                    .any(|v| !v.is_finite() || *v < 0.)
                {
                    self.cancel_drag(&id, drag.finished);
                    effects.redraw("widgets");
                    return Some(Intent::Error(lucent_domain::DomainError::Invalid(
                        "Widget output dimensions must be finite and non-negative".into(),
                    )));
                }
                let result = (|| {
                    let origin = match self.drags.get(&id) {
                        Some(Drag::Active { origin, .. }) => *origin,
                        _ => lucent_usecases::constrain_placement(
                            settings
                                .positions
                                .get(&id)
                                .copied()
                                .unwrap_or_else(|| default_position(&id, input.viewport)),
                            widget_size(&id),
                            input.viewport,
                        )?,
                    };
                    let placement = Placement::new(origin.x() + drag.dx, origin.y() + drag.dy)?;
                    if drag.finished {
                        let snapped = lucent_usecases::snap_placement(
                            placement,
                            component::widget_layout::GRID_STEP,
                        )?;
                        lucent_usecases::move_widget(
                            settings,
                            &id,
                            snapped,
                            widget_size(&id),
                            input.viewport,
                        )
                    } else {
                        let preview = lucent_usecases::constrain_placement(
                            placement,
                            widget_size(&id),
                            input.viewport,
                        )?;
                        self.drags
                            .insert(id.clone(), Drag::Active { origin, preview });
                        Ok(())
                    }
                })();
                if drag.finished || result.is_err() {
                    self.cancel_drag(&id, drag.finished);
                }
                effects.redraw("widgets");
                match result {
                    Err(error) => Some(Intent::Error(error)),
                    Ok(()) if drag.finished => Some(Intent::Save),
                    Ok(()) => None,
                }
            }
            Message::Notes(text) => {
                settings.notes = text;
                effects.redraw("widgets");
                Some(Intent::Save)
            }
            Message::TimerToggle => {
                self.timer.toggle(input.unix_seconds);
                effects.redraw("widgets");
                None
            }
            Message::TimerReset => {
                self.timer = FocusTimer::default();
                effects.redraw("widgets");
                None
            }
            Message::Tick(now) => {
                let before = self.timer.remaining;
                self.timer.tick(now);
                if before != self.timer.remaining
                    && settings.visible_widgets.iter().any(|id| id == "timer")
                {
                    effects.redraw("widgets");
                }
                None
            }
            Message::Month(delta) => {
                self.month_offset = self.month_offset.saturating_add(delta).clamp(-120, 120);
                effects.redraw("widgets");
                None
            }
            Message::ResetLayout => {
                for drag in self.drags.values_mut() {
                    *drag = Drag::Cancelled;
                }
                settings.positions.clear();
                effects.redraw("widgets");
                Some(Intent::Save)
            }
            Message::Action(action) => Some(Intent::Action(action)),
            Message::RetrySave => Some(Intent::RetrySave),
        }
    }
}

pub fn widget_size(id: &str) -> (f32, f32) {
    catalog::find(id).map(|entry| entry.size).unwrap_or((
        component::widget_size::fallback::WIDTH,
        component::widget_size::fallback::HEIGHT,
    ))
}
pub fn default_position(id: &str, viewport: (f32, f32)) -> Placement {
    let (x, y) = catalog::find(id)
        .map(|entry| {
            (
                if entry.from_right {
                    viewport.0 - entry.position.0
                } else {
                    entry.position.0
                },
                entry.position.1,
            )
        })
        .unwrap_or((
            component::widget_layout::LEFT,
            component::widget_layout::TOP,
        ));
    let size = widget_size(id);
    let position = lucent_usecases::snap_placement(
        Placement::new(x, y).expect("finite viewport and design tokens"),
        component::widget_layout::GRID_STEP,
    )
    .expect("valid grid token and default placement");
    Placement::new(
        position.x().clamp(0., (viewport.0 - size.0).max(0.)),
        position.y().clamp(0., (viewport.1 - size.1).max(0.)),
    )
    .expect("finite clamped default position")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send(
        host: &mut Host,
        settings: &mut DesktopSettings,
        message: Message,
        now: u64,
    ) -> (Option<Intent>, Effects<Message>) {
        let mut effects = Effects::default();
        let intent = host.update(
            message,
            Update {
                settings,
                viewport: (1920., 1080.),
                unix_seconds: now,
            },
            &mut effects,
        );
        assert!(effects.tasks.is_empty(), "the host never performs OS work");
        (intent, effects)
    }
    fn drag(finished: bool) -> Message {
        Message::Move(
            "calendar".into(),
            DragEvent {
                dx: 19.,
                dy: 25.,
                finished,
            },
        )
    }
    #[test]
    fn hidden_and_unknown_widgets_cannot_resume_a_drag_or_request_persistence() {
        let mut host = Host::default();
        let mut settings = DesktopSettings::default();
        assert!(send(&mut host, &mut settings, drag(false), 0).0.is_none());
        assert!(host.grid_visible());
        assert!(matches!(
            send(
                &mut host,
                &mut settings,
                Message::Toggle("calendar".into()),
                0
            )
            .0,
            Some(Intent::Save)
        ));
        assert!(!host.grid_visible());
        let position = settings.positions.get("calendar").copied();
        assert!(send(&mut host, &mut settings, drag(true), 0).0.is_none());
        assert_eq!(settings.positions.get("calendar").copied(), position);
        assert!(!host.grid_visible());
        for message in [
            Message::Show("missing".into()),
            Message::Toggle("missing".into()),
            Message::Move(
                "missing".into(),
                DragEvent {
                    dx: 1.,
                    dy: 2.,
                    finished: true,
                },
            ),
        ] {
            assert!(send(&mut host, &mut settings, message, 0).0.is_none());
        }
        assert!(!settings.positions.contains_key("missing"));
        assert!(!settings.visible_widgets.iter().any(|id| id == "missing"));
    }
    #[test]
    fn dragging_saves_only_on_release_and_reset_preserves_other_preferences() {
        let mut host = Host::default();
        let mut settings = DesktopSettings {
            notes: "Keep this".into(),
            ..Default::default()
        };
        let visibility = settings.visible_widgets.clone();
        let origin = default_position("calendar", (1920., 1080.));
        assert!(send(&mut host, &mut settings, drag(false), 0).0.is_none());
        assert!(!settings.positions.contains_key("calendar"));
        assert_eq!(
            host.preview_position("calendar"),
            Some(Placement::new(origin.x() + 19., origin.y() + 25.).unwrap())
        );
        assert!(matches!(
            send(&mut host, &mut settings, drag(true), 0).0,
            Some(Intent::Save)
        ));
        assert!(!host.grid_visible());
        let step = component::widget_layout::GRID_STEP;
        assert_eq!(settings.positions["calendar"].x() % step, 0.);
        assert_eq!(settings.positions["calendar"].y() % step, 0.);
        send(&mut host, &mut settings, drag(false), 0);
        assert!(matches!(
            send(&mut host, &mut settings, Message::ResetLayout, 0).0,
            Some(Intent::Save)
        ));
        assert!(!host.grid_visible());
        assert!(settings.positions.is_empty());
        assert_eq!(settings.notes, "Keep this");
        assert_eq!(settings.visible_widgets, visibility);
    }

    #[test]
    fn reset_cancels_the_rest_of_the_gesture_before_a_new_drag_can_start() {
        let mut host = Host::default();
        let mut settings = DesktopSettings::default();
        send(&mut host, &mut settings, drag(false), 0);
        send(&mut host, &mut settings, Message::ResetLayout, 0);
        assert!(send(&mut host, &mut settings, drag(false), 0).0.is_none());
        assert!(!host.grid_visible());
        assert!(send(&mut host, &mut settings, drag(true), 0).0.is_none());
        assert!(settings.positions.is_empty());
        send(&mut host, &mut settings, drag(false), 0);
        assert!(host.grid_visible());
        assert!(matches!(
            send(&mut host, &mut settings, drag(true), 0).0,
            Some(Intent::Save)
        ));
    }

    #[test]
    fn hiding_and_reshowing_during_a_gesture_does_not_commit_its_preview() {
        let mut host = Host::default();
        let mut settings = DesktopSettings::default();
        send(&mut host, &mut settings, drag(false), 0);
        send(
            &mut host,
            &mut settings,
            Message::Toggle("calendar".into()),
            0,
        );
        send(
            &mut host,
            &mut settings,
            Message::Show("calendar".into()),
            0,
        );
        assert!(send(&mut host, &mut settings, drag(false), 0).0.is_none());
        assert!(send(&mut host, &mut settings, drag(true), 0).0.is_none());
        assert!(!host.grid_visible());
        assert!(settings.positions.is_empty());
    }

    #[test]
    fn dragging_starts_at_the_visible_clamped_position_after_output_resize() {
        let mut host = Host::default();
        let mut settings = DesktopSettings::default();
        let saved = Placement::new(5000., -400.).unwrap();
        settings.positions.insert("calendar".into(), saved);
        let maximum = 1920. - widget_size("calendar").0;
        send(
            &mut host,
            &mut settings,
            Message::Move(
                "calendar".into(),
                DragEvent {
                    dx: -19.,
                    dy: 25.,
                    finished: false,
                },
            ),
            0,
        );
        assert_eq!(settings.positions["calendar"], saved);
        assert_eq!(
            host.preview_position("calendar"),
            Some(Placement::new(maximum - 19., 25.).unwrap())
        );
        send(
            &mut host,
            &mut settings,
            Message::Move(
                "calendar".into(),
                DragEvent {
                    dx: -19.,
                    dy: 25.,
                    finished: true,
                },
            ),
            0,
        );
        assert_eq!(
            settings.positions["calendar"],
            lucent_usecases::snap_placement(
                Placement::new(maximum - 19., 25.).unwrap(),
                component::widget_layout::GRID_STEP
            )
            .unwrap()
        );
    }
    #[test]
    fn malformed_output_geometry_cannot_create_an_origin_or_mutate_preferences() {
        let mut settings = DesktopSettings::default();
        for viewport in [(f32::NAN, 1080.), (1920., f32::INFINITY), (-1., 1080.)] {
            let mut host = Host::default();
            let mut effects = Effects::default();
            let result = host.update(
                drag(false),
                Update {
                    settings: &mut settings,
                    viewport,
                    unix_seconds: 0,
                },
                &mut effects,
            );
            assert!(matches!(result, Some(Intent::Error(_))));
            assert!(!host.grid_visible());
            assert!(settings.positions.is_empty());
            assert!(effects.tasks.is_empty());
        }
    }
    #[test]
    fn timer_uses_supplied_time_and_only_redraws_visible_changes() {
        let mut host = Host::default();
        let mut settings = DesktopSettings::default();
        assert!(
            send(&mut host, &mut settings, Message::Tick(100), 100)
                .1
                .redraw
                .is_empty()
        );
        send(&mut host, &mut settings, Message::TimerToggle, 100);
        assert!(
            send(&mut host, &mut settings, Message::Tick(110), 110)
                .1
                .redraw
                .is_empty()
        );
        assert_eq!(host.timer_remaining(), 1490);
        send(&mut host, &mut settings, Message::Show("timer".into()), 110);
        assert!(
            send(&mut host, &mut settings, Message::Tick(111), 111)
                .1
                .redraw
                .contains("widgets")
        );
        send(&mut host, &mut settings, Message::TimerToggle, 111);
        assert!(
            send(&mut host, &mut settings, Message::Tick(200), 200)
                .1
                .redraw
                .is_empty()
        );
        assert_eq!(host.timer_remaining(), 1489);
        assert!(
            send(&mut host, &mut settings, Message::TimerReset, 200)
                .0
                .is_none()
        );
        assert_eq!(host.timer_remaining(), 1500);
    }
    #[test]
    fn notes_and_media_return_typed_parent_intents() {
        let mut host = Host::default();
        let mut settings = DesktopSettings::default();
        assert!(matches!(
            send(&mut host, &mut settings, Message::Notes("Hello".into()), 0).0,
            Some(Intent::Save)
        ));
        assert_eq!(settings.notes, "Hello");
        assert!(matches!(
            send(&mut host, &mut settings, Message::Action(Action::Next), 0).0,
            Some(Intent::Action(Action::Next))
        ));
    }
}
