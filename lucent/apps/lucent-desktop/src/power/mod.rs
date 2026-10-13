//! Native power child: observations, explicit confirmation and bounded intent.
mod view;
use lucent_api::*;
use lucent_design::motion;
use lucent_domain::*;
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub enum Message {
    Observed(Result<PowerSnapshot>),
    Control(PowerCommand),
    Brightness(PowerObjectId, SliderEvent),
    Ask(PowerAction),
    Confirm(PowerAction),
    Back,
    Open,
    Toggle,
    Close,
    Closed(u64),
    Scroll(ScrollEvent),
    Admitted(u64, Result<PowerOperationId>),
    Dismiss,
    Ignore,
}
#[derive(Default, PartialEq)]
enum Observation {
    #[default]
    Loading,
    Ready(Box<PowerSnapshot>),
    Unavailable(String),
}
struct Admission {
    command: PowerCommand,
    epoch: u64,
    id: Option<PowerOperationId>,
}
pub struct Power {
    adapter: Arc<dyn PowerPort>,
    observation: Observation,
    epoch: u64,
    open: bool,
    mounted: bool,
    motion: Motion,
    close_generation: u64,
    scroll: f32,
    confirmation: Option<PowerAction>,
    revision: u64,
    active: Option<u64>,
    admission: Option<Admission>,
    brightness: Option<(PowerObjectId, Percentage)>,
    error: String,
}
impl Power {
    pub fn new(adapter: Arc<dyn PowerPort>) -> Self {
        Self {
            adapter,
            observation: Observation::Loading,
            epoch: 0,
            open: false,
            mounted: false,
            motion: Motion::fixed(0.),
            close_generation: 0,
            scroll: 0.,
            confirmation: None,
            revision: 0,
            active: None,
            admission: None,
            brightness: None,
            error: String::new(),
        }
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
    fn snapshot(&self) -> Option<&PowerSnapshot> {
        match &self.observation {
            Observation::Ready(s) => Some(s),
            _ => None,
        }
    }
    fn busy(&self) -> bool {
        self.active.is_some()
            || self.admission.is_some()
            || self.snapshot().is_some_and(|s| {
                s.operation
                    .as_ref()
                    .is_some_and(|o| o.phase == PowerOperationPhase::InProgress)
            })
    }
    pub fn label(&self) -> String {
        match self.snapshot().map(|s| &s.battery) {
            Some(PowerCapability::Ready(Some(battery))) => match battery.level {
                BatteryLevel::None => format!("{:.0}%", battery.charge.value()),
                BatteryLevel::Unknown => "Battery".into(),
                BatteryLevel::Low => "Low".into(),
                BatteryLevel::Critical => "Critical".into(),
                BatteryLevel::Normal => "Battery".into(),
                BatteryLevel::High => "High".into(),
                BatteryLevel::Full => "Full".into(),
            },
            _ => "Power".into(),
        }
    }
    fn set_open(&mut self, open: bool, now: f64) {
        if self.open == open {
            return;
        }
        self.open = open;
        self.mounted = true;
        if !open {
            self.confirmation = None;
            self.brightness = None;
            self.close_generation = self.close_generation.wrapping_add(1);
        }
        self.motion.target(
            if open { 1. } else { 0. },
            now,
            if open { motion::ENTER } else { motion::EXIT },
            motion::DECELERATE,
        );
    }
    fn retired(&self, admission: &Admission) -> bool {
        admission.epoch!=self.epoch||self.snapshot().is_none_or(|s|match admission.command {
            PowerCommand::SetProfile(id)=>!matches!(&s.profiles,PowerCapability::Ready(p) if p.profiles.iter().any(|p|p.id==id)),
            PowerCommand::SetBrightness(id,_)=>!matches!(&s.brightness,PowerCapability::Ready(d) if d.iter().any(|d|d.id==id)),
            PowerCommand::Session(_)=>!matches!(s.session,PowerCapability::Ready(_)),
        })
    }
    fn submit(&mut self, command: PowerCommand, effects: &mut Effects<Message>) {
        if self.busy() {
            return;
        }
        let valid = self
            .snapshot()
            .ok_or_else(|| DomainError::Unavailable("Power services are not ready".into()))
            .and_then(|s| lucent_usecases::power::validate_power_command(s, &command));
        if let Err(error) = valid {
            self.error = error.to_string();
            return;
        }
        self.revision = self.revision.wrapping_add(1);
        let revision = self.revision;
        self.active = Some(revision);
        self.error.clear();
        self.admission = Some(Admission {
            command: command.clone(),
            epoch: self.epoch,
            id: None,
        });
        let adapter = self.adapter.clone();
        effects.task_on_with_failure(
            crate::work::CONTROLS,
            move || Message::Admitted(revision, adapter.control(command)),
            move |failure| {
                Message::Admitted(revision, Err(DomainError::Failed(failure.to_string())))
            },
        );
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        let before = self.label();
        match message {
            Message::Observed(result) => {
                let next = match result {
                    Ok(s) => Observation::Ready(Box::new(s)),
                    Err(error) => Observation::Unavailable(error.to_string()),
                };
                if next == self.observation {
                    return;
                }
                if matches!(next, Observation::Unavailable(_)) {
                    self.epoch = self.epoch.wrapping_add(1);
                    self.brightness = None;
                    self.confirmation = None;
                }
                let old = self.snapshot().and_then(|s| s.operation.clone());
                self.observation = next;
                if self.admission.as_ref().is_some_and(|a| {
                    a.id.is_some_and(|id| {
                        self.retired(a)
                            || self
                                .snapshot()
                                .is_some_and(|s| s.operation.as_ref().is_some_and(|o| o.id == id))
                    })
                }) {
                    self.admission = None;
                }
                if let Some(operation) = self.snapshot().and_then(|s| s.operation.as_ref())
                    && old.as_ref() != Some(operation)
                    && let PowerOperationPhase::Failed(error) = &operation.phase
                {
                    self.error = error.clone();
                    self.brightness = None;
                }
                if self.confirmation.is_some_and(|a| {
                    self.snapshot().is_none_or(|s| {
                        lucent_usecases::power::validate_power_action(s, a).is_err()
                    })
                }) {
                    self.confirmation = None;
                }
            }
            Message::Open => self.set_open(true, effects.now),
            Message::Toggle => self.set_open(!self.open, effects.now),
            Message::Close => self.set_open(false, effects.now),
            Message::Closed(generation) => {
                if !self.open && generation == self.close_generation {
                    self.mounted = false;
                }
            }
            Message::Scroll(event) => {
                if event.offset.is_finite() {
                    self.scroll = event.offset.max(0.);
                }
            }
            Message::Ask(action) => {
                if !self.busy()
                    && self.snapshot().is_some_and(|s| {
                        lucent_usecases::power::validate_power_action(s, action).is_ok()
                    })
                {
                    self.confirmation = Some(action);
                    self.scroll = 0.;
                }
            }
            Message::Confirm(action) => {
                if self.confirmation == Some(action) {
                    self.confirmation = None;
                    self.submit(PowerCommand::Session(action), effects);
                }
            }
            Message::Back => self.confirmation = None,
            Message::Control(PowerCommand::Session(_)) => return, // Session actions require our explicit confirmation path.
            Message::Control(command) => self.submit(command, effects),
            Message::Brightness(id, event) => {
                if event.value.is_finite() && (0. ..=1.).contains(&event.value) {
                    let value = Percentage::new(f64::from(event.value) * 100.)
                        .expect("finite slider percentage");
                    let current=self.snapshot().is_some_and(|s|matches!(&s.brightness,PowerCapability::Ready(devices) if devices.iter().any(|d|d.id==id)));
                    if current && self.open {
                        if self.busy() {
                            let ours=self.admission.as_ref().is_some_and(|a|matches!(a.command,PowerCommand::SetBrightness(target,_) if target==id))||self.snapshot().is_some_and(|s|s.operation.as_ref().is_some_and(|o|matches!(o.command,PowerCommand::SetBrightness(target,_) if target==id)));
                            if ours {
                                self.brightness = Some((id, value));
                            }
                        } else {
                            self.submit(PowerCommand::SetBrightness(id, value), effects);
                        }
                    }
                }
            }
            Message::Admitted(revision, result) => {
                if self.active != Some(revision) {
                    return;
                }
                self.active = None;
                let admission = self.admission.take();
                match result {
                    Ok(id) => {
                        if let Some(mut a) = admission {
                            a.id = Some(id);
                            if !self.retired(&a)
                                && !self.snapshot().is_some_and(|s| {
                                    s.operation.as_ref().is_some_and(|o| o.id == id)
                                })
                            {
                                self.admission = Some(a);
                            }
                        }
                    }
                    Err(error) => {
                        self.error = error.to_string();
                        self.brightness = None;
                    }
                }
            }
            Message::Dismiss => self.error.clear(),
            Message::Ignore => return,
        }
        if !self.busy()
            && self.open
            && let Some((id, value)) = self.brightness.take()
        {
            self.submit(PowerCommand::SetBrightness(id, value), effects);
        }
        if before != self.label() {
            effects.redraw("bar");
        }
        effects.redraw("power");
    }
    pub fn escape(&self) -> Message {
        if self.confirmation.is_some() {
            Message::Back
        } else {
            Message::Close
        }
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        if self.mounted && !self.open {
            let generation = self.close_generation;
            vec![Subscription::stream(
                format!("power-close-{generation}"),
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
        serde_json::json!({"open":self.open,"visible":self.mounted,"busy":self.busy(),"confirming":self.confirmation.is_some(),"error":self.error,"status":match self.observation {Observation::Loading=>"loading",Observation::Ready(_)=>"ready",Observation::Unavailable(_)=>"unavailable"},"operation":self.snapshot().and_then(|s|s.operation.as_ref()).map(|o|serde_json::json!({"id":o.id.value(),"phase":match o.phase {PowerOperationPhase::InProgress=>"in-progress",PowerOperationPhase::Succeeded=>"succeeded",PowerOperationPhase::Failed(_)=>"failed"}}))})
    }
}

#[cfg(test)]
pub(super) mod tests;
