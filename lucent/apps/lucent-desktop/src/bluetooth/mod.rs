//! Client state and effects over an injected domain port; no OS or rendering backend.
mod form;
#[cfg(test)]
pub(crate) mod tests;
mod view;
use form::Form;
use lucent_api::{Effects, InputEdit, Motion, ScrollEvent, Subscription};
use lucent_design::motion;
use lucent_domain::*;
use std::{collections::BTreeSet, sync::Arc, time::Duration};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Available,
    Paired,
    Radio,
}
#[derive(Clone)]
pub enum Message {
    Observed(Result<BluetoothSnapshot>),
    Open,
    Toggle,
    Close,
    Closed(u64),
    Tab(Tab),
    Controller(BluetoothObjectId),
    Scroll(ScrollEvent),
    Control(BluetoothCommand),
    Pair(BluetoothObjectId),
    Incoming(bool),
    Edit(BluetoothPairingId, InputEdit),
    Submit(BluetoothPairingId),
    Decision(BluetoothPairingId, bool),
    CancelPairing(BluetoothPairingId),
    Finished(u64, bool, Result<Outcome>),
    AskForget(BluetoothObjectId),
    ConfirmForget,
    Back,
    Dismiss,
    Ignore,
}
#[derive(Clone)]
pub enum Outcome {
    Done,
    Started(BluetoothPairingId),
}
#[derive(Default, PartialEq)]
enum Observation {
    #[default]
    Loading,
    Ready(BluetoothSnapshot),
    Unavailable(String),
}
pub struct Bluetooth {
    adapter: Arc<dyn BluetoothPort>,
    observation: Observation,
    open: bool,
    mounted: bool,
    motion: Motion,
    close_generation: u64,
    tab: Tab,
    controller: Option<BluetoothObjectId>,
    scroll: f32,
    revision: u64,
    active: Option<u64>,
    answering: Option<u64>,
    pending_pair: Option<BluetoothObjectId>,
    pending_scan: Option<BluetoothObjectId>,
    closing_device: Option<BluetoothObjectId>,
    scan_stops: BTreeSet<BluetoothObjectId>,
    form: Option<Form>,
    forget: Option<BluetoothObjectId>,
    error: String,
    notice: String,
}
impl Bluetooth {
    pub fn new(adapter: Arc<dyn BluetoothPort>) -> Self {
        Self {
            adapter,
            observation: Observation::Loading,
            open: false,
            mounted: false,
            motion: Motion::fixed(0.),
            close_generation: 0,
            tab: Tab::Available,
            controller: None,
            scroll: 0.,
            revision: 0,
            active: None,
            answering: None,
            pending_pair: None,
            pending_scan: None,
            closing_device: None,
            scan_stops: BTreeSet::new(),
            form: None,
            forget: None,
            error: String::new(),
            notice: String::new(),
        }
    }
    pub fn open(&self) -> bool {
        self.open
    }
    pub fn visible(&self) -> bool {
        self.mounted
    }
    pub fn animating(&self, now: f64) -> bool {
        self.mounted && self.motion.active(now)
    }
    pub fn snapshot(&self) -> Option<&BluetoothSnapshot> {
        match &self.observation {
            Observation::Ready(s) => Some(s),
            _ => None,
        }
    }
    pub fn status(&self) -> &'static str {
        match &self.observation {
            Observation::Loading => "Bluetooth loading",
            Observation::Unavailable(_) => "Bluetooth unavailable",
            Observation::Ready(s) if s.adapters.is_empty() => "No Bluetooth controller",
            Observation::Ready(s) if !s.adapters.iter().any(|a| a.powered) => "Bluetooth off",
            Observation::Ready(s) if s.devices.iter().any(|d| d.connected) => "Bluetooth connected",
            Observation::Ready(_) => "Bluetooth ready",
        }
    }
    fn set_open(&mut self, open: bool, now: f64) {
        if self.open == open {
            return;
        }
        self.open = open;
        self.mounted = true;
        if !open {
            self.form = None;
            self.forget = None;
            self.close_generation = self.close_generation.wrapping_add(1);
        }
        self.motion.target(
            if open { 1. } else { 0. },
            now,
            if open { motion::ENTER } else { motion::EXIT },
            motion::DECELERATE,
        );
    }
    fn sync_form(&mut self) {
        let request = self.snapshot().and_then(|s| s.pairing_requests.first());
        let next = request.and_then(Form::for_request);
        if !self.open {
            self.form = None;
        } else if self.form.as_ref().map(|f| (f.request, f.kind))
            != next.as_ref().map(|f| (f.request, f.kind))
        {
            self.form = next;
        }
    }
    fn submit_task(
        &mut self,
        reply: bool,
        effects: &mut Effects<Message>,
        operation: impl FnOnce(Arc<dyn BluetoothPort>) -> Result<Outcome> + Send + 'static,
    ) {
        if !reply && self.active.is_some() || reply && self.answering.is_some() {
            self.error = "A Bluetooth request is already being submitted".into();
            return;
        }
        let Some(revision) = self.revision.checked_add(1) else {
            self.error = "Bluetooth request limit reached; restart the desktop".into();
            return;
        };
        self.revision = revision;
        if reply {
            self.answering = Some(revision);
        } else {
            self.active = Some(revision);
        }
        self.error.clear();
        self.notice.clear();
        let adapter = self.adapter.clone();
        let run = move || Message::Finished(revision, reply, operation(adapter));
        if reply {
            effects.task_on_with_failure(crate::work::CONTROLS, run, move |failure| {
                Message::Finished(
                    revision,
                    true,
                    Err(DomainError::Unavailable(failure.to_string())),
                )
            });
        } else {
            effects.task_on(crate::work::BLUETOOTH, run);
        }
    }
    fn answer(
        &mut self,
        id: BluetoothPairingId,
        answer: BluetoothPairingAnswer,
        effects: &mut Effects<Message>,
    ) {
        let valid = self
            .snapshot()
            .and_then(|s| s.pairing_requests.iter().find(|p| p.id == id))
            .ok_or_else(|| {
                DomainError::Invalid("Pairing request ended; use the current request".into())
            })
            .and_then(|p| lucent_usecases::bluetooth::validate_pairing_answer(&p.prompt, &answer));
        if let Err(error) = valid {
            self.error = error.to_string();
            return;
        }
        let cancellation = if matches!(&answer, BluetoothPairingAnswer::Reject) {
            self.snapshot()
                .and_then(|s| lucent_usecases::bluetooth::pairing_attempt_for_rejected_reply(s, id))
        } else {
            None
        };
        self.submit_task(true, effects, move |adapter| {
            let response = adapter.answer_pairing(id, answer);
            if let Some(attempt) = cancellation {
                // The prompt may retire before this ordered task runs. Its
                // captured attempt ID cannot cancel a replacement conversation.
                let mut failure = match response {
                    Ok(()) | Err(DomainError::Invalid(_)) => None,
                    Err(error) => Some(error),
                };
                match adapter.cancel_pairing(attempt) {
                    Ok(()) | Err(DomainError::Invalid(_)) => (),
                    Err(error) => {
                        failure.get_or_insert(error);
                    }
                }
                failure.map_or(Ok(Outcome::Done), Err)
            } else {
                response.map(|()| Outcome::Done)
            }
        });
    }
    fn close(&mut self, effects: &mut Effects<Message>) {
        if !self.open {
            return;
        }
        let requests: Vec<_> = self
            .snapshot()
            .map(|s| s.pairing_requests.iter().map(|p| p.id).collect())
            .unwrap_or_default();
        let attempt = self
            .snapshot()
            .and_then(|s| s.pairing.as_ref())
            .filter(|a| a.phase == BluetoothPairingPhase::Pairing)
            .map(|a| a.id);
        self.closing_device = self
            .snapshot()
            .and_then(|s| s.pairing.as_ref())
            .filter(|a| a.phase == BluetoothPairingPhase::Pairing)
            .map(|a| a.device)
            .or(self.pending_pair);
        let scans: Vec<_> = self
            .snapshot()
            .map(|s| {
                s.adapters
                    .iter()
                    .filter(|a| a.discovery_owned)
                    .map(|a| a.id)
                    .collect()
            })
            .unwrap_or_default();
        self.scan_stops.extend(scans);
        self.scan_stops.extend(self.pending_scan);
        self.set_open(false, effects.now);
        if !requests.is_empty() || attempt.is_some() {
            // A close follows an already accepted answer on the same ordered
            // short lane. Old completions cannot clear this newer operation.
            self.answering = None;
            self.submit_task(true, effects, move |adapter| {
                let mut failure = None;
                for id in requests {
                    match adapter.answer_pairing(id, BluetoothPairingAnswer::Reject) {
                        Ok(()) | Err(DomainError::Invalid(_)) => (),
                        Err(error) => {
                            failure.get_or_insert(error);
                        }
                    }
                }
                if let Some(id) = attempt {
                    match adapter.cancel_pairing(id) {
                        Ok(()) | Err(DomainError::Invalid(_)) => (),
                        Err(error) => {
                            failure.get_or_insert(error);
                        }
                    }
                }
                failure.map_or(Ok(Outcome::Done), Err)
            });
        }
    }
    fn stop_scans(&mut self, effects: &mut Effects<Message>) {
        if self.active.is_some() || self.scan_stops.is_empty() {
            return;
        }
        let targets = std::mem::take(&mut self.scan_stops);
        self.submit_task(false, effects, move |adapter| {
            let mut failure = None;
            for id in targets {
                match adapter.control(BluetoothCommand::SetDiscovery {
                    adapter: id,
                    active: false,
                }) {
                    Ok(()) | Err(DomainError::Invalid(_)) => (),
                    Err(error) => {
                        failure.get_or_insert(error);
                    }
                }
            }
            failure.map_or(Ok(Outcome::Done), Err)
        });
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        let before = self.status();
        match message {
            Message::Observed(result) => {
                let old: Vec<_> = self
                    .snapshot()
                    .map(|s| s.pairing_requests.iter().map(|p| p.id).collect())
                    .unwrap_or_default();
                let next = match result {
                    Ok(s) => Observation::Ready(s),
                    Err(e) => Observation::Unavailable(e.to_string()),
                };
                if next == self.observation {
                    return;
                }
                self.observation = next;
                if self.closing_device.is_some_and(|id| {
                    self.snapshot().is_none_or(|s| {
                        !s.devices.iter().any(|d| d.id == id)
                            || s.pairing.as_ref().is_some_and(|a| {
                                a.device == id && a.phase != BluetoothPairingPhase::Pairing
                            })
                    })
                }) {
                    self.closing_device = None;
                }
                if !self.controller.is_some_and(|id| {
                    self.snapshot()
                        .is_some_and(|s| s.adapters.iter().any(|a| a.id == id))
                }) {
                    self.controller = self
                        .snapshot()
                        .and_then(|s| s.adapters.first())
                        .map(|a| a.id);
                }
                if self.forget.is_some_and(|id| {
                    !self
                        .snapshot()
                        .is_some_and(|s| s.devices.iter().any(|d| d.id == id))
                }) {
                    self.forget = None;
                }
                if self.snapshot().is_some_and(|s| {
                    s.pairing_requests
                        .iter()
                        .any(|p| !old.contains(&p.id) && Some(p.device) != self.closing_device)
                }) {
                    self.set_open(true, effects.now);
                    self.scroll = 0.;
                    self.forget = None;
                }
                self.sync_form();
            }
            Message::Open => {
                self.set_open(true, effects.now);
                self.sync_form();
            }
            Message::Toggle => {
                if self.open {
                    self.close(effects)
                } else {
                    self.set_open(true, effects.now);
                    self.sync_form();
                }
            }
            Message::Close => self.close(effects),
            Message::Closed(generation) => {
                if !self.open && generation == self.close_generation {
                    self.mounted = false;
                }
            }
            Message::Tab(tab) => {
                self.tab = tab;
                self.scroll = 0.;
                self.forget = None;
            }
            Message::Controller(id) => {
                if self
                    .snapshot()
                    .is_some_and(|s| s.adapters.iter().any(|a| a.id == id))
                {
                    self.controller = Some(id);
                    self.scroll = 0.;
                }
            }
            Message::Scroll(s) => {
                if s.offset.is_finite() {
                    self.scroll = s.offset.max(0.);
                }
            }
            Message::Control(command) => {
                let valid = self
                    .snapshot()
                    .ok_or_else(|| DomainError::Unavailable("Bluetooth is not ready".into()))
                    .and_then(|s| {
                        lucent_usecases::bluetooth::validate_bluetooth_command(s, &command)
                    });
                if let Err(e) = valid {
                    self.error = e.to_string();
                } else {
                    let scan = match command {
                        BluetoothCommand::SetDiscovery {
                            adapter,
                            active: true,
                        } => Some(adapter),
                        _ => None,
                    };
                    let before = self.revision;
                    self.submit_task(false, effects, move |adapter| {
                        adapter.control(command).map(|()| Outcome::Done)
                    });
                    if self.revision != before {
                        self.pending_scan = scan;
                    }
                }
            }
            Message::Pair(id) => {
                let valid = self
                    .snapshot()
                    .ok_or_else(|| DomainError::Unavailable("Bluetooth is not ready".into()))
                    .and_then(|s| lucent_usecases::bluetooth::validate_pairing_target(s, id));
                if let Err(e) = valid {
                    self.error = e.to_string();
                } else {
                    let before = self.revision;
                    self.submit_task(false, effects, move |adapter| {
                        adapter.start_pairing(id).map(Outcome::Started)
                    });
                    if self.revision != before {
                        self.pending_pair = Some(id);
                    }
                }
            }
            Message::Incoming(enabled) => self.submit_task(false, effects, move |adapter| {
                adapter
                    .set_incoming_pairing(enabled)
                    .map(|()| Outcome::Done)
            }),
            Message::Edit(id, edit) => {
                if self.answering.is_none()
                    && let Some(form) = self.form.as_mut().filter(|f| f.request == id)
                {
                    form.edit(edit);
                }
            }
            Message::Submit(id) => {
                if self.answering.is_some() {
                    return;
                }
                let value = self
                    .form
                    .as_mut()
                    .filter(|f| f.request == id)
                    .map(|f| f.answer());
                match value {
                    Some(Ok(answer)) => self.answer(id, answer, effects),
                    Some(Err(e)) => self.error = e.to_string(),
                    None => (),
                }
            }
            Message::Decision(id, accept) => self.answer(
                id,
                if accept {
                    BluetoothPairingAnswer::Accept
                } else {
                    BluetoothPairingAnswer::Reject
                },
                effects,
            ),
            Message::CancelPairing(id) => self.submit_task(true, effects, move |adapter| {
                adapter.cancel_pairing(id).map(|()| Outcome::Done)
            }),
            Message::Finished(revision, reply, result) => {
                let active = if reply {
                    &mut self.answering
                } else {
                    &mut self.active
                };
                if *active != Some(revision) {
                    return;
                }
                *active = None;
                let target = if !reply {
                    self.pending_scan = None;
                    self.pending_pair.take()
                } else {
                    None
                };
                match result {
                    Ok(Outcome::Done) => {
                        self.notice = "Request accepted; waiting for observed state.".into()
                    }
                    Ok(Outcome::Started(id))
                        if target.is_some() && target == self.closing_device =>
                    {
                        self.answering = None;
                        self.submit_task(true, effects, move |adapter| {
                            match adapter.cancel_pairing(id) {
                                Ok(()) | Err(DomainError::Invalid(_)) => Ok(Outcome::Done),
                                Err(e) => Err(e),
                            }
                        });
                    }
                    Ok(Outcome::Started(id)) => {
                        self.notice = format!(
                            "Pairing attempt {} started; waiting for the device.",
                            id.value()
                        )
                    }
                    Err(e) => {
                        self.error = e.to_string();
                        if target == self.closing_device {
                            self.closing_device = None;
                        }
                    }
                }
            }
            Message::AskForget(id) => {
                if self
                    .snapshot()
                    .is_some_and(|s| s.devices.iter().any(|d| d.id == id))
                {
                    self.forget = Some(id);
                    self.scroll = 0.;
                }
            }
            Message::ConfirmForget => {
                if let Some(id) = self.forget.take() {
                    self.update(Message::Control(BluetoothCommand::Forget(id)), effects);
                }
            }
            Message::Back => self.forget = None,
            Message::Dismiss => {
                self.error.clear();
                self.notice.clear();
            }
            Message::Ignore => return,
        }
        self.stop_scans(effects);
        if before != self.status() {
            effects.redraw("bar");
        }
        effects.redraw("bluetooth");
    }
    pub fn escape(&self) -> Message {
        if self.forget.is_some() {
            Message::Back
        } else {
            Message::Close
        }
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        if self.mounted && !self.open {
            let generation = self.close_generation;
            vec![Subscription::stream(
                format!("bluetooth-close-{generation}"),
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
        let pairing = self.snapshot().and_then(|s| s.pairing.as_ref()).map(|a| {
            serde_json::json!({"id":a.id.value(),"device":a.device.value(),"phase":match a.phase {
                BluetoothPairingPhase::Pairing => "pairing",
                BluetoothPairingPhase::Succeeded => "succeeded",
                BluetoothPairingPhase::Cancelled => "cancelled",
                BluetoothPairingPhase::Failed(_) => "failed",
            }})
        });
        serde_json::json!({"status":match self.observation {Observation::Loading=>"loading",Observation::Ready(_)=>"ready",Observation::Unavailable(_)=>"unavailable"},"label":self.status(),"open":self.open,"visible":self.mounted,"scroll":self.scroll,"tab":match self.tab {Tab::Available=>"available",Tab::Paired=>"paired",Tab::Radio=>"radio"},"controller":self.controller.map(|id|id.value()),"busy":self.active.is_some(),"answering":self.answering.is_some(),"error":self.error,"notice":self.notice,"controllers":self.snapshot().map(|s|s.adapters.len()),"devices":self.snapshot().map(|s|s.devices.len()),"requests":self.snapshot().map(|s|s.pairing_requests.iter().map(|p|serde_json::json!({"id":p.id.value(),"device":p.device.value()})).collect::<Vec<_>>()),"pairing":pairing,"incoming":self.snapshot().map(|s|s.incoming_pairing_enabled),"form":self.form.as_ref().map(|f|f.request.value())})
    }
}
