//! Client-owned observation and command failures; the adapter owns backend state.
use lucent_domain::*;
#[derive(Default, PartialEq)]
enum Observation {
    #[default]
    Loading,
    Ready(AudioSnapshot),
    Unavailable(String),
}
#[derive(Default)]
pub struct Audio {
    observation: Observation,
    control_error: Option<String>,
}
impl Audio {
    pub fn observe(&mut self, result: Result<AudioSnapshot>) -> bool {
        let next = match result {
            Ok(value) => Observation::Ready(value),
            Err(e) => Observation::Unavailable(e.to_string()),
        };
        if self.observation == next {
            return false;
        }
        self.observation = next;
        true
    }
    pub fn completed(&mut self, result: Result<()>) {
        self.control_error = result.err().map(|e| e.to_string());
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
        serde_json::json!({"status":status,"error":error,"control_error":self.control_error,
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
        let mut audio = Audio::default();
        assert_eq!(audio.inspect()["status"], "loading");
        assert_eq!(audio.label(), "—");
        assert!(audio.observe(Ok(fixture())));
        assert!(!audio.observe(Ok(fixture())));
        assert_eq!(audio.label(), "100");
        audio.completed(Err(DomainError::Unavailable("rejected".into())));
        audio.observe(Ok(fixture()));
        assert!(audio.inspect()["control_error"].is_string());
        audio.observe(Err(DomainError::Unavailable("disconnected".into())));
        assert_eq!(audio.label(), "—");
        assert!(audio.output().is_none());
        audio.observe(Ok(AudioSnapshot::default()));
        assert_eq!(audio.inspect()["status"], "ready");
        assert_eq!(audio.label(), "—");
        audio.observe(Ok(fixture()));
        audio.completed(Ok(()));
        assert!(audio.inspect()["control_error"].is_null());
    }
}
