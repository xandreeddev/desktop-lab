//! Native network child controller. The injected adapter owns transport/state.
mod control;
mod form;
#[cfg(test)]
pub(crate) mod tests;
mod view;
pub use control::Control;
use form::{Form, Target};
use lucent_api::{Effects, InputEdit, Motion, ScrollEvent, Subscription};
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
    Name(u64, String),
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
            }
            Message::Open => self.set_open(true, effects.now),
            Message::Toggle => self.set_open(!self.open, effects.now),
            Message::Close => self.set_open(false, effects.now),
            Message::Closed(generation) => {
                if !self.open && generation == self.close_generation {
                    self.mounted = false;
                }
            }
            Message::Tab(tab) => {
                self.tab = tab;
                self.form = None;
                self.forget = None;
                self.scroll = 0.;
            }
            Message::Device(id) => {
                if self
                    .snapshot()
                    .and_then(|s| s.device(id))
                    .is_some_and(|d| d.kind == NetworkKind::Wifi && d.managed)
                {
                    self.device = Some(id);
                    self.form = None;
                    self.scroll = 0.;
                }
            }
            Message::Scroll(event) => self.scroll = event.offset,
            Message::Request(control) => {
                if control.valid(self.snapshot()) {
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
                if let Some(device) = self.device {
                    self.begin(Target::Hidden { device });
                }
            }
            Message::Cancel => {
                self.form = None;
                self.forget = None;
                self.scroll = 0.;
            }
            Message::Name(generation, value) => {
                if let Some(f) = self.form.as_mut().filter(|f| {
                    f.generation == generation && matches!(f.target, Target::Hidden { .. })
                }) && value.len() <= 32
                {
                    f.name = value;
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
                if let Some(f) = self.form.as_mut().filter(|f| f.generation == generation) {
                    f.remember = !f.remember;
                }
            }
            Message::Submit(generation) => {
                if self.active.is_some() {
                    self.fail(None, "Wait for the current request before connecting");
                } else if self
                    .form
                    .as_ref()
                    .is_some_and(|f| f.generation == generation && f.valid(self.snapshot()))
                {
                    match self.form.as_mut().unwrap().submit() {
                        Ok(command) => {
                            self.form = None;
                            self.start(None, command, effects);
                        }
                        Err(error) => self.fail(None, error.to_string()),
                    }
                }
            }
            Message::AskForget(id) => {
                if self.snapshot().and_then(|s| s.profile(id)).is_some() {
                    self.forget = Some(id);
                    self.form = None;
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
        let mut result = vec![];
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
        serde_json::json!({"status":match &self.observation{Observation::Loading=>"loading",Observation::Ready(_)=>"ready",Observation::Unavailable(_)=>"unavailable"},"label":self.status(),"open":self.open,"visible":self.mounted,"tab":self.tab.name(),"scroll":self.scroll,"selected_device":self.device.map(|id|id.value()),"busy":self.active.is_some(),"error":self.failure.as_ref().map(|f|&f.reason),"notice":self.notice,"scan_pending":self.scan.is_some(),"form":self.form.as_ref().map(|f|serde_json::json!({"generation":f.generation,"hidden":matches!(f.target,Target::Hidden{..}),"remember":f.remember,"security":view::security_name(f.security)})),
            "wifi_enabled":self.snapshot().map(|s|s.wifi_enabled),"devices":self.snapshot().map(|s|s.devices.iter().map(|d|serde_json::json!({"id":d.id.value(),"interface":d.interface,"kind":format!("{:?}",d.kind),"state":format!("{:?}",d.state),"failure":format!("{:?}",d.failure),"scan_generation":d.scan_generation})).collect::<Vec<_>>()),
            "profiles":self.snapshot().map(|s|s.profiles.iter().map(|p|serde_json::json!({"id":p.id.value(),"key":p.key.as_str(),"name":p.name,"ssid":p.ssid.as_ref().map(|s|s.bytes())})).collect::<Vec<_>>()),
            "access_points":self.snapshot().map(|s|s.access_points.iter().map(|a|serde_json::json!({"id":a.id.value(),"device":a.device.value(),"ssid":a.ssid.as_ref().map(|s|s.bytes()),"security":view::security_name(a.security),"strength":a.strength.percent()})).collect::<Vec<_>>()),
            "connections":self.snapshot().map(|s|s.connections.iter().map(|c|serde_json::json!({"id":c.id.value(),"profile":c.profile.map(|id|id.value()),"devices":c.devices.iter().map(|id|id.value()).collect::<Vec<_>>(),"state":format!("{:?}",c.state)})).collect::<Vec<_>>())})
    }
}
