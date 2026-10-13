//! Client-owned observation and command failures; the adapter owns backend state.
use lucent_api::{Effects, ScrollEvent, SliderEvent, Subscription};
use lucent_design::motion;
use lucent_domain::*;
use std::{sync::Arc, time::Duration};
pub mod commands;
pub mod panel;
#[derive(Clone)]
pub enum Message {
    Observed(Result<AudioSnapshot>),
    Request(AudioCommand),
    Control(commands::Message),
    Open,
    Toggle,
    Close,
    Closed(u64),
    Tab(panel::Tab),
    Scroll(ScrollEvent),
    Volume(AudioObjectId, SliderEvent),
    ChooseRoute(AudioObjectId),
    Back,
    Ignore,
}
#[derive(Default, PartialEq)]
enum Observation {
    #[default]
    Loading,
    Ready(AudioSnapshot),
    Unavailable(String),
}
pub struct Audio {
    observation: Observation,
    commands: commands::Commands,
    panel: panel::Panel,
}
impl Audio {
    pub fn new(adapter: Arc<dyn AudioPort>) -> Self {
        Self {
            observation: Observation::Loading,
            commands: commands::Commands::new(adapter),
            panel: Default::default(),
        }
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Observed(result) => {
                if !self.observe(result) {
                    return;
                }
            }
            Message::Request(command) => {
                let snapshot = match &self.observation {
                    Observation::Ready(value) => Some(value),
                    _ => None,
                };
                effects.delegate(
                    |child| self.commands.request(command, snapshot, child),
                    Message::Control,
                );
            }
            Message::Control(message) => {
                let snapshot = match &self.observation {
                    Observation::Ready(value) => Some(value),
                    _ => None,
                };
                effects.delegate(
                    |child| self.commands.update(message, snapshot, child),
                    Message::Control,
                );
            }
            Message::Open => self.panel.set_open(true, effects.now),
            Message::Toggle => self.panel.set_open(!self.panel.open, effects.now),
            Message::Close => self.panel.set_open(false, effects.now),
            Message::Closed(generation) => {
                if !self.panel.open && generation == self.panel.close_generation {
                    self.panel.mounted = false;
                }
            }
            Message::Tab(tab) => {
                self.panel.tab = tab;
                self.panel.route = None;
                self.panel.scroll = 0.;
            }
            Message::Scroll(event) => self.panel.scroll = event.offset,
            Message::Volume(id, event) => {
                if event.value.is_finite() && (0. ..=1.).contains(&event.value) {
                    self.update(
                        Message::Request(AudioCommand::SetVolume(
                            id,
                            AudioVolume::new(f64::from(event.value))
                                .expect("finite normalized volume"),
                        )),
                        effects,
                    );
                }
            }
            Message::ChooseRoute(id) => {
                if self
                    .snapshot()
                    .is_some_and(|s| s.streams.iter().any(|stream| stream.id == id))
                {
                    self.panel.route = Some(id);
                    self.panel.scroll = 0.;
                }
            }
            Message::Back => {
                self.panel.route = None;
                self.panel.scroll = 0.;
            }
            Message::Ignore => return,
        }
        effects.redraw("bar");
        effects.redraw("audio");
    }
    pub fn open(&self) -> bool {
        self.panel.open
    }
    pub fn visible(&self) -> bool {
        self.panel.mounted
    }
    pub fn animating(&self, now: f64) -> bool {
        self.panel.mounted && self.panel.motion.active(now)
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        if !self.panel.mounted || self.panel.open {
            return vec![];
        }
        let generation = self.panel.close_generation;
        vec![Subscription::stream(
            format!("audio-close-{generation}"),
            move |out, cancel| {
                cancel.sleep(Duration::from_secs_f64(motion::EXIT));
                if !cancel.cancelled() {
                    out.send(Message::Closed(generation));
                }
            },
        )]
    }
    pub fn observe(&mut self, result: Result<AudioSnapshot>) -> bool {
        let next = match result {
            Ok(value) => Observation::Ready(value),
            Err(e) => Observation::Unavailable(e.to_string()),
        };
        if self.observation == next {
            return false;
        }
        self.commands.reconcile(match &next {
            Observation::Ready(value) => Some(value),
            _ => None,
        });
        if self.panel.route.is_some_and(|id| !matches!(&next, Observation::Ready(s) if s.streams.iter().any(|stream| stream.id==id))) {
            self.panel.route = None; self.panel.scroll = 0.;
        }
        self.observation = next;
        true
    }
    pub fn snapshot(&self) -> Option<&AudioSnapshot> {
        match &self.observation {
            Observation::Ready(value) => Some(value),
            _ => None,
        }
    }
    pub fn output(&self) -> Option<&AudioDevice> {
        self.snapshot()?.default_device(AudioDirection::Playback)
    }
    pub fn label(&self) -> String {
        match self.output() {
            Some(device) if device.muted => "×".into(),
            Some(device) => format!("{:.0}", device.volume.ratio() * 100.),
            None => "—".into(),
        }
    }
    pub fn inspect(&self) -> serde_json::Value {
        let (status, error) = match &self.observation {
            Observation::Loading => ("loading", None),
            Observation::Ready(_) => ("ready", None),
            Observation::Unavailable(error) => ("unavailable", Some(error)),
        };
        serde_json::json!({"status":status,"error":error,"control_error":self.commands.error(),
            "control_retry_available":self.commands.can_retry(self.snapshot()),
            "open":self.panel.open,"visible":self.panel.mounted,"tab":self.panel.tab.name(),"scroll":self.panel.scroll,
            "routing_stream":self.panel.route.map(|id|id.value()),
            "device_details":self.snapshot().map(|s|s.devices.iter().map(|d|serde_json::json!({"id":d.id.value(),"key":d.key.as_str(),"name":d.name,"direction":if d.direction==AudioDirection::Playback {"output"} else {"input"},"volume":d.volume.ratio(),"muted":d.muted})).collect::<Vec<_>>()),
            "stream_details":self.snapshot().map(|s|s.streams.iter().map(|v|serde_json::json!({"id":v.id.value(),"name":v.name,"application":v.application,"device":v.device.map(|id|id.value()),"direction":if v.direction==AudioDirection::Playback {"output"} else {"input"},"volume":v.volume.map(|v|v.ratio()),"muted":v.muted,"volume_writable":v.volume_writable})).collect::<Vec<_>>()),
            "busy":self.commands.busy(),"pending_commands":self.commands.pending_count(),
            "devices":self.snapshot().map(|s|s.devices.len()),"streams":self.snapshot().map(|s|s.streams.len()),
            "output_volume":self.output().map(|d|d.volume.ratio()),"output_muted":self.output().map(|d|d.muted)})
    }
}
#[cfg(test)]
pub fn fixture() -> AudioSnapshot {
    let id = AudioObjectId::new(1).unwrap();
    AudioSnapshot {
        devices: vec![AudioDevice {
            id,
            key: AudioDeviceKey::new("fixture.output").unwrap(),
            name: "Fixture output".into(),
            direction: AudioDirection::Playback,
            volume: AudioVolume::NORMAL,
            muted: false,
            monitor: false,
        }],
        default_output: Some(id),
        ..Default::default()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_state_does_not_show_stale_or_fabricated_volume_and_keeps_command_failure() {
        let mut audio = Audio::new(Arc::new(crate::test_adapters::Fake::default()));
        assert_eq!(audio.inspect()["status"], "loading");
        assert_eq!(audio.label(), "—");
        assert!(audio.observe(Ok(fixture())));
        assert!(!audio.observe(Ok(fixture())));
        assert_eq!(audio.label(), "100");
        audio.update(
            Message::Request(AudioCommand::Raise),
            &mut Effects::default(),
        );
        audio.update(
            Message::Control(commands::Message::WorkerFailed("rejected".into())),
            &mut Effects::default(),
        );
        audio.observe(Ok(fixture()));
        assert!(audio.inspect()["control_error"].is_string());
        audio.observe(Err(DomainError::Unavailable("disconnected".into())));
        assert_eq!(audio.label(), "—");
        assert!(audio.output().is_none());
        audio.observe(Ok(AudioSnapshot::default()));
        assert_eq!(audio.inspect()["status"], "ready");
        assert_eq!(audio.label(), "—");
        audio.observe(Ok(fixture()));
        let mut effects = Effects::default();
        audio.update(Message::Control(commands::Message::Retry), &mut effects);
        for task in effects.tasks {
            audio.update(task.run(), &mut Effects::default());
        }
        assert!(audio.inspect()["control_error"].is_null());
    }

    #[test]
    fn panel_motion_is_continuous_and_stale_close_cannot_unmount_a_reopened_panel() {
        let mut audio = Audio::new(Arc::new(crate::test_adapters::Fake::default()));
        let mut effects = Effects::default();
        audio.update(Message::Open, &mut effects);
        assert!(audio.visible());
        assert!(audio.open());
        assert!(audio.animating(0.));
        effects.now = 0.1;
        let before = audio.panel.motion.value(effects.now);
        audio.update(Message::Close, &mut effects);
        assert_eq!(audio.panel.motion.value(effects.now), before);
        let generation = audio.panel.close_generation;
        assert_eq!(audio.subscriptions().len(), 1);
        effects.now = 0.15;
        audio.update(Message::Open, &mut effects);
        assert!(audio.subscriptions().is_empty());
        audio.update(Message::Closed(generation), &mut effects);
        assert!(audio.visible());
        effects.now = 1.;
        audio.update(Message::Close, &mut effects);
        audio.update(Message::Closed(generation), &mut effects);
        assert!(audio.visible());
        audio.update(Message::Closed(audio.panel.close_generation), &mut effects);
        assert!(!audio.visible());
        assert!(!audio.animating(1.));
    }
}
