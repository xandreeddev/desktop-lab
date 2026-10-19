//! Native network child controller. The injected adapter owns transport/state.
mod control;
mod form;
#[cfg(test)]
pub(crate) mod tests;
mod view;
pub use control::Control;
use form::{Form, Submission, Target};
use lucent_api::{
    ClipboardTarget, Effects, InputEdit, Motion, ScrollEvent, Subscription, TextEdit,
};
use lucent_design::motion;
use lucent_domain::*;
use std::{sync::Arc, time::Duration};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Available,
    Saved,
    Connections,
}
impl Tab {
    pub fn name(self) -> &'static str {
        match self {
            Self::Available => "Wi-Fi",
            Self::Saved => "Saved",
            Self::Connections => "Active",
        }
    }
}
#[derive(Clone)]
pub enum Message {
    Observed(Result<NetworkSnapshot>),
    Credentials(Result<Vec<NetworkCredentialRequest>>),
    Answered(NetworkCredentialRequestId, Result<()>),
    Open,
    Toggle,
    Close,
    Closed(u64),
    Tab(Tab),
    Device(NetworkObjectId),
    Scroll(ScrollEvent),
    Request(Control),
    Finished(u64, Result<()>),
    WorkerFailed(String),
    Retry,
    Dismiss,
    Join(NetworkObjectId),
    Hidden,
    Cancel,
    Name(u64, TextEdit),
    Password(u64, InputEdit),
    Security(u64, WifiSecurity),
    Remember(u64),
    Submit(u64),
    AskForget(NetworkObjectId),
    ConfirmForget,
    ScanExpired(u64),
    Ignore,
}
#[derive(Default, PartialEq)]
enum Observation {
    #[default]
    Loading,
    Ready(NetworkSnapshot),
    Unavailable(String),
}
struct Active {
    revision: u64,
    control: Option<Control>,
}
struct Failure {
    control: Option<Control>,
    reason: String,
}
struct Scan {
    revision: u64,
    device: NetworkObjectId,
    before: Option<u64>,
}
pub struct Network {
    adapter: Arc<dyn NetworkPort>,
    observation: Observation,
    open: bool,
    mounted: bool,
    motion: Motion,
    close_generation: u64,
    tab: Tab,
    device: Option<NetworkObjectId>,
    scroll: f32,
    revision: u64,
    form_generation: u64,
    form: Option<Form>,
    forget: Option<NetworkObjectId>,
    active: Option<Active>,
    failure: Option<Failure>,
    scan: Option<Scan>,
    notice: String,
    credential_requests: Vec<NetworkCredentialRequest>,
    credential_reply: Option<NetworkCredentialRequestId>,
    retired_request: u64,
    activation_profile: Option<NetworkProfileKey>,
    dismissed_profile: Option<NetworkProfileKey>,
}
impl Network {
    pub fn new(adapter: Arc<dyn NetworkPort>) -> Self {
        Self {
            adapter,
            observation: Observation::Loading,
            open: false,
            mounted: false,
            motion: Motion::fixed(0.),
            close_generation: 0,
            tab: Tab::Available,
            device: None,
            scroll: 0.,
            revision: 0,
            form_generation: 0,
            form: None,
            forget: None,
            active: None,
            failure: None,
            scan: None,
            notice: String::new(),
            credential_requests: vec![],
            credential_reply: None,
            retired_request: 0,
            activation_profile: None,
            dismissed_profile: None,
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
    pub fn snapshot(&self) -> Option<&NetworkSnapshot> {
        match &self.observation {
            Observation::Ready(s) => Some(s),
            _ => None,
        }
    }
    pub fn status(&self) -> &'static str {
        match &self.observation {
            Observation::Loading => "Network loading",
            Observation::Unavailable(_) => "Network unavailable",
            Observation::Ready(s) => match s.connectivity {
                NetworkConnectivity::Portal => "Sign-in required",
                NetworkConnectivity::Limited => "Limited connectivity",
                _ => match s.state {
                    NetworkState::Global => "Connected",
                    NetworkState::Site | NetworkState::Local => "Local connection",
                    NetworkState::Connecting => "Connecting",
                    NetworkState::Disconnecting => "Disconnecting",
                    NetworkState::Asleep => "Network asleep",
                    NetworkState::Disconnected => "Disconnected",
                    NetworkState::Unknown => "Network state unknown",
                },
            },
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
    fn fail(&mut self, control: Option<Control>, reason: impl Into<String>) {
        self.failure = Some(Failure {
            control,
            reason: reason.into(),
        });
    }
    fn start(
        &mut self,
        control: Option<Control>,
        command: NetworkCommand,
        effects: &mut Effects<Message>,
    ) {
        if self.active.is_some() {
            self.fail(control, "A network change is already being submitted");
            return;
        }
        let Some(revision) = self.revision.checked_add(1) else {
            self.fail(
                control,
                "Network command limit reached; restart the desktop",
            );
            return;
        };
        self.revision = revision;
        self.failure = None;
        self.notice.clear();
        self.form = None;
        if let Some(Control::Scan(device)) = control.as_ref() {
            self.scan = Some(Scan {
                revision,
                device: *device,
                before: self
                    .snapshot()
                    .and_then(|s| s.device(*device))
                    .and_then(|d| d.scan_generation),
            });
        }
        self.active = Some(Active { revision, control });
        let adapter = self.adapter.clone();
        effects.task_on(crate::work::NETWORK, move || {
            Message::Finished(revision, adapter.control(command))
        });
    }
    fn begin(&mut self, target: Target) {
        if self.active.is_some() {
            return;
        }
        let Some(generation) = self.form_generation.checked_add(1) else {
            self.fail(None, "Network form limit reached; restart the desktop");
            return;
        };
        let form = Form::new(generation, target);
        if !form.valid(self.snapshot()) {
            self.fail(None, "This network is no longer available");
            return;
        }
        self.form_generation = generation;
        self.form = Some(form);
        self.forget = None;
        self.scroll = 0.;
        self.failure = None;
        self.notice.clear();
    }
    fn answer(
        &mut self,
        request: NetworkCredentialRequestId,
        credentials: Option<WifiCredentials>,
        effects: &mut Effects<Message>,
    ) {
        self.retired_request = self.retired_request.max(request.value());
        let adapter = self.adapter.clone();
        let submitting = credentials.is_some();
        if submitting {
            self.credential_reply = Some(request);
        }
        effects.task_on_with_failure(
            crate::work::CONTROLS,
            move || Message::Answered(request, adapter.answer_credentials(request, credentials)),
            move |_| {
                Message::Answered(
                    request,
                    Err(DomainError::Unavailable(
                        "Network credential answer could not be delivered; it will not be retried"
                            .into(),
                    )),
                )
            },
        );
    }
    fn cancel_form(&mut self, effects: &mut Effects<Message>) {
        if let Some(request) = self.form.take().and_then(|f| f.credential_request()) {
            self.answer(request, None, effects);
        }
    }
    fn close(&mut self, now: f64, effects: &mut Effects<Message>) {
        self.dismissed_profile = self.activation_profile.clone();
        self.cancel_form(effects);
        // Retire requests already observed, including a second queued callback.
        let requests: Vec<_> = self
            .credential_requests
            .iter()
            .filter(|r| r.id.value() > self.retired_request)
            .map(|r| r.id)
            .collect();
        for request in requests {
            self.answer(request, None, effects);
        }
        self.set_open(false, now);
    }
    fn credentials(
        &mut self,
        value: Result<Vec<NetworkCredentialRequest>>,
        effects: &mut Effects<Message>,
    ) {
        self.credential_requests = value.unwrap_or_default();
        if self
            .form
            .as_ref()
            .and_then(Form::credential_request)
            .is_some_and(|id| !self.credential_requests.iter().any(|r| r.id == id))
        {
            self.form = None; // daemon cancellation/timeout must drop secret immediately
        }
        let Some(request) = self
            .credential_requests
            .iter()
            .find(|r| r.id.value() > self.retired_request)
            .cloned()
        else {
            return;
        };
        if self.dismissed_profile.as_ref() == Some(&request.profile) {
            self.answer(request.id, None, effects);
            return;
        }
        if self.form.as_ref().and_then(Form::credential_request) == Some(request.id) {
            return;
        }
        let device = self.device.filter(|id| {
            self.snapshot()
                .and_then(|s| s.device(*id))
                .is_some_and(|d| d.managed && d.kind == NetworkKind::Wifi)
        });
        let Some(device) = device else {
            if !matches!(self.observation, Observation::Loading) {
                self.answer(request.id, None, effects);
            }
            return;
        };
        // The prompt can arrive before Activate's method acknowledgement. It
        // bypasses that command lane; its exact request supplies reply authority.
        self.form_generation = match self.form_generation.checked_add(1) {
            Some(n) => n,
            None => {
                self.answer(request.id, None, effects);
                return;
            }
        };
        let form = Form::new(
            self.form_generation,
            Target::Credentials {
                request: request.clone(),
                device,
            },
        );
        if !form.valid(self.snapshot()) {
            self.answer(request.id, None, effects);
            return;
        }
        self.form = Some(form);
        self.forget = None;
        self.failure = None;
        self.notice.clear();
        self.scroll = 0.;
        self.set_open(true, effects.now);
    }
    fn observe(&mut self, result: Result<NetworkSnapshot>) -> bool {
        let next = match result {
            Ok(s) => Observation::Ready(s),
            Err(e) => Observation::Unavailable(e.to_string()),
        };
        if self.observation == next {
            return false;
        }
        self.observation = next;
        if !self.device.is_some_and(|id| {
            self.snapshot()
                .and_then(|s| s.device(id))
                .is_some_and(|d| d.kind == NetworkKind::Wifi && d.managed)
        }) {
            self.device = self
                .snapshot()
                .and_then(|s| {
                    s.devices
                        .iter()
                        .find(|d| d.kind == NetworkKind::Wifi && d.managed)
                })
                .map(|d| d.id);
            self.scroll = 0.;
        }
        if self
            .form
            .as_ref()
            .is_some_and(|f| !f.valid(self.snapshot()))
        {
            self.form = None;
            self.fail(None, "The selected network changed. Choose it again.");
        }
        if self
            .forget
            .is_some_and(|id| self.snapshot().and_then(|s| s.profile(id)).is_none())
        {
            self.forget = None;
        }
        if let Some(scan) = &self.scan {
            match self.snapshot().and_then(|s| s.device(scan.device)) {
                None => self.scan = None,
                Some(d) if d.scan_generation != scan.before => {
                    self.scan = None;
                    self.notice = "Scan completed".into();
                }
                _ => {}
            }
        }
        true
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        // The exiting overlay releases all input. Ignore messages from a stale
        // scene as well; observations and owned task completions still apply.
        if !self.open
            && !matches!(
                message,
                Message::Observed(_)
                    | Message::Credentials(_)
                    | Message::Answered(..)
                    | Message::Open
                    | Message::Toggle
                    | Message::Close
                    | Message::Closed(_)
                    | Message::Finished(..)
                    | Message::WorkerFailed(_)
                    | Message::ScanExpired(_)
            )
        {
            return;
        }
        let before = self.status();
        match message {
            Message::Observed(result) => {
                if !self.observe(result) {
                    return;
                }
                if self.snapshot().is_some() {
                    self.credentials(Ok(self.credential_requests.clone()), effects);
                } else {
                    self.credential_requests.clear();
                    self.activation_profile = None;
                    self.dismissed_profile = None;
                }
            }
            Message::Credentials(value) => self.credentials(value, effects),
            Message::Answered(id, result) => {
                if self.credential_reply == Some(id) {
                    self.credential_reply = None;
                    match result {
                        Ok(()) => {
                            self.notice = "Password delivered; waiting for connection state.".into()
                        }
                        Err(e) => self.fail(None, e.to_string()),
                    }
                }
            }
            Message::Open => self.set_open(true, effects.now),
            Message::Toggle if self.open => self.close(effects.now, effects),
            Message::Toggle => self.set_open(true, effects.now),
            Message::Close => self.close(effects.now, effects),
            Message::Closed(generation) => {
                if !self.open && generation == self.close_generation {
                    self.mounted = false;
                }
            }
            Message::Tab(tab) => {
                self.cancel_form(effects);
                self.tab = tab;
                self.forget = None;
                self.scroll = 0.;
            }
            Message::Device(id) => {
                if self
                    .snapshot()
                    .and_then(|s| s.device(id))
                    .is_some_and(|d| d.kind == NetworkKind::Wifi && d.managed)
                {
                    self.cancel_form(effects);
                    self.device = Some(id);
                    self.scroll = 0.;
                }
            }
            Message::Scroll(event) => self.scroll = event.offset,
            Message::Request(control) => {
                if control.valid(self.snapshot()) {
                    self.cancel_form(effects);
                    if let Control::Activate(profile, _) = &control {
                        self.activation_profile = self
                            .snapshot()
                            .and_then(|s| s.profile(*profile))
                            .map(|p| p.key.clone());
                        self.dismissed_profile = None;
                    }
                    self.start(Some(control.clone()), control.command(), effects);
                } else {
                    self.fail(
                        Some(control),
                        "This network control is unavailable. Choose a current item.",
                    );
                }
            }
            Message::Finished(revision, result) => {
                if self.active.as_ref().is_none_or(|a| a.revision != revision) {
                    return;
                }
                let active = self.active.take().unwrap();
                match result {
                    Ok(()) => {
                        if self.failure.is_none() {
                            self.notice =
                                "Request accepted; connection state is shown above.".into();
                        }
                    }
                    Err(e) => {
                        if self.scan.as_ref().is_some_and(|s| s.revision == revision) {
                            self.scan = None;
                        }
                        if self.failure.is_none() {
                            self.fail(active.control, e.to_string());
                        }
                    }
                }
            }
            Message::WorkerFailed(reason) => {
                if let Some(active) = &self.active {
                    self.update(
                        Message::Finished(active.revision, Err(DomainError::Failed(reason))),
                        effects,
                    );
                }
            }
            Message::Retry => {
                if let Some(control) = self
                    .failure
                    .as_ref()
                    .and_then(|f| f.control.clone())
                    .filter(|c| c.valid(self.snapshot()))
                {
                    self.update(Message::Request(control), effects);
                } else {
                    self.fail(
                        None,
                        "Choose the network again. Passwords are not retained for retries.",
                    );
                }
            }
            Message::Dismiss => {
                self.failure = None;
                self.notice.clear();
            }
            Message::Join(id) => {
                self.cancel_form(effects);
                let target = self
                    .snapshot()
                    .and_then(|s| s.access_point(id))
                    .and_then(|a| {
                        a.ssid.clone().map(|ssid| Target::Visible {
                            access_point: id,
                            device: a.device,
                            ssid,
                            security: a.security,
                        })
                    });
                if let Some(target) = target {
                    self.begin(target);
                } else {
                    self.fail(None, "Choose a current network, or enter its hidden name.");
                }
            }
            Message::Hidden => {
                self.cancel_form(effects);
                if let Some(device) = self.device {
                    self.begin(Target::Hidden { device });
                }
            }
            Message::Cancel => {
                self.cancel_form(effects);
                self.forget = None;
                self.scroll = 0.;
            }
            Message::Name(generation, edit) => {
                if let Some(f) = self.form.as_mut().filter(|f| {
                    f.generation == generation && matches!(f.target, Target::Hidden { .. })
                }) && let Err(error) = f.name_field.update(
                    &mut f.name,
                    edit,
                    32,
                    ClipboardTarget::new("network", format!("network-name-{generation}")),
                    effects,
                    move |edit| Message::Name(generation, edit),
                ) {
                    self.fail(None, error.to_string());
                }
            }
            Message::Password(generation, edit) => {
                if let Some(f) = self.form.as_mut().filter(|f| f.generation == generation) {
                    f.password_edit(edit);
                }
            }
            Message::Security(generation, security) => {
                if let Some(f) = self.form.as_mut().filter(|f| f.generation == generation) {
                    f.set_security(security);
                }
            }
            Message::Remember(generation) => {
                if let Some(f) = self
                    .form
                    .as_mut()
                    .filter(|f| f.generation == generation && f.credential_request().is_none())
                {
                    f.remember = !f.remember;
                }
            }
            Message::Submit(generation) => {
                let credential_form = self
                    .form
                    .as_ref()
                    .and_then(Form::credential_request)
                    .is_some();
                if self.credential_reply.is_some() || (self.active.is_some() && !credential_form) {
                    self.fail(None, "Wait for the current request before connecting");
                } else if self
                    .form
                    .as_ref()
                    .is_some_and(|f| f.generation == generation && f.valid(self.snapshot()))
                {
                    match self.form.as_mut().unwrap().submit() {
                        Ok(Submission::Join(command)) => {
                            self.form = None;
                            self.start(None, command, effects);
                        }
                        Ok(Submission::Credentials(id, credentials)) => {
                            self.form = None;
                            self.answer(id, Some(credentials), effects);
                        }
                        Err(error) => self.fail(None, error.to_string()),
                    }
                }
            }
            Message::AskForget(id) => {
                if self.snapshot().and_then(|s| s.profile(id)).is_some() {
                    self.cancel_form(effects);
                    self.forget = Some(id);
                    self.scroll = 0.;
                }
            }
            Message::ConfirmForget => {
                if let Some(id) = self.forget.take() {
                    self.update(Message::Request(Control::Forget(id)), effects);
                }
            }
            Message::ScanExpired(revision) => {
                if self.scan.as_ref().is_some_and(|s| s.revision == revision) {
                    self.scan = None;
                    self.notice = "Scan request accepted; completion was not observed.".into();
                }
            }
            Message::Ignore => return,
        }
        if self.status() != before {
            effects.redraw("bar");
            effects.redraw("widgets");
        }
        effects.redraw("network");
    }
    pub fn escape(&self) -> Message {
        if self.form.is_some() || self.forget.is_some() {
            Message::Cancel
        } else {
            Message::Close
        }
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        let adapter = self.adapter.clone();
        let mut result = vec![Subscription::stream(
            "network-credentials",
            move |out, cancel| {
                adapter.watch_credentials(
                    &mut |value| out.send(Message::Credentials(value)),
                    &crate::adapters::WatchStop(cancel),
                );
            },
        )];
        if self.mounted && !self.open {
            let generation = self.close_generation;
            result.push(Subscription::stream(
                format!("network-close-{generation}"),
                move |out, cancel| {
                    cancel.sleep(Duration::from_secs_f64(motion::EXIT));
                    if !cancel.cancelled() {
                        out.send(Message::Closed(generation));
                    }
                },
            ));
        }
        if let Some(scan) = &self.scan {
            let revision = scan.revision;
            result.push(Subscription::stream(
                format!("network-scan-{revision}"),
                move |out, cancel| {
                    cancel.sleep(Duration::from_secs(30));
                    if !cancel.cancelled() {
                        out.send(Message::ScanExpired(revision));
                    }
                },
            ));
        }
        result
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"status":match &self.observation{Observation::Loading=>"loading",Observation::Ready(_)=>"ready",Observation::Unavailable(_)=>"unavailable"},"label":self.status(),"open":self.open,"visible":self.mounted,"tab":self.tab.name(),"scroll":self.scroll,"selected_device":self.device.map(|id|id.value()),"busy":self.active.is_some(),"error":self.failure.as_ref().map(|f|&f.reason),"notice":self.notice,"scan_pending":self.scan.is_some(),"form":self.form.as_ref().map(|f|serde_json::json!({"generation":f.generation,"credential_request":f.credential_request().map(|id|id.value()),"hidden":matches!(f.target,Target::Hidden{..}),"remember":f.remember,"security":view::security_name(f.security)})),
            "wifi_enabled":self.snapshot().map(|s|s.wifi_enabled),"devices":self.snapshot().map(|s|s.devices.iter().map(|d|serde_json::json!({"id":d.id.value(),"interface":d.interface,"kind":format!("{:?}",d.kind),"state":format!("{:?}",d.state),"failure":format!("{:?}",d.failure),"scan_generation":d.scan_generation})).collect::<Vec<_>>()),
            "profiles":self.snapshot().map(|s|s.profiles.iter().map(|p|serde_json::json!({"id":p.id.value(),"key":p.key.as_str(),"name":p.name,"ssid":p.ssid.as_ref().map(|s|s.bytes())})).collect::<Vec<_>>()),
            "access_points":self.snapshot().map(|s|s.access_points.iter().map(|a|serde_json::json!({"id":a.id.value(),"device":a.device.value(),"ssid":a.ssid.as_ref().map(|s|s.bytes()),"security":view::security_name(a.security),"strength":a.strength.percent()})).collect::<Vec<_>>()),
            "connections":self.snapshot().map(|s|s.connections.iter().map(|c|serde_json::json!({"id":c.id.value(),"profile":c.profile.map(|id|id.value()),"devices":c.devices.iter().map(|id|id.value()).collect::<Vec<_>>(),"state":format!("{:?}",c.state)})).collect::<Vec<_>>())})
    }
}
