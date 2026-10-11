//! Audio identities, observations and intent. No transport or rendering types.
use crate::{DomainError, Result, StopSignal};

/// Opaque identity scoped to one AudioPort adapter's lifetime. Adapters allocate
/// a fresh identity after an object disappears or the service reconnects.
/// Clients must not persist these handles or infer a backend index from them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AudioObjectId(u64);
impl AudioObjectId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid(
                "Audio object identity is empty".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Backend-provided stable device key, useful for remembering a preferred device.
/// Controls use AudioObjectId so known-stale views cannot target a reconnected object.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AudioDeviceKey(String);
impl AudioDeviceKey {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
            return Err(DomainError::Invalid("Invalid audio device key".into()));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Relative volume: zero is silence and one is the normal maximum. Observed
/// amplification above one remains visible; the client chooses its control cap.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct AudioVolume(f64);
impl AudioVolume {
    pub const SILENCE: Self = Self(0.);
    pub const NORMAL: Self = Self(1.);
    pub fn new(ratio: f64) -> Result<Self> {
        if !ratio.is_finite() || ratio < 0. {
            return Err(DomainError::Invalid(
                "Audio volume must be finite and non-negative".into(),
            ));
        }
        Ok(Self(ratio))
    }
    pub fn ratio(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioDirection {
    Playback,
    Capture,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioDevice {
    pub id: AudioObjectId,
    pub key: AudioDeviceKey,
    pub name: String,
    pub direction: AudioDirection,
    pub volume: AudioVolume,
    pub muted: bool,
    /// A capture endpoint recording another output rather than a microphone.
    pub monitor: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioStream {
    pub id: AudioObjectId,
    pub name: String,
    pub application: String,
    pub direction: AudioDirection,
    pub device: Option<AudioObjectId>,
    pub volume: Option<AudioVolume>,
    pub volume_writable: bool,
    pub muted: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioSnapshot {
    pub devices: Vec<AudioDevice>,
    pub streams: Vec<AudioStream>,
    pub default_output: Option<AudioObjectId>,
    pub default_input: Option<AudioObjectId>,
}
impl AudioSnapshot {
    pub fn device(&self, id: AudioObjectId) -> Option<&AudioDevice> {
        self.devices.iter().find(|device| device.id == id)
    }
    pub fn default_device(&self, direction: AudioDirection) -> Option<&AudioDevice> {
        self.device(match direction {
            AudioDirection::Playback => self.default_output?,
            AudioDirection::Capture => self.default_input?,
        })
        .filter(|device| device.direction == direction)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AudioCommand {
    Raise,
    Lower,
    ToggleMute,
    SetVolume(AudioObjectId, AudioVolume),
    SetMute(AudioObjectId, bool),
    SetDefault(AudioObjectId),
    Route {
        stream: AudioObjectId,
        device: AudioObjectId,
    },
}

pub trait AudioPort: Send + Sync {
    /// The adapter owns observation, reconnection and cancellation. Errors are
    /// unavailable state, never a fabricated zero-volume snapshot.
    fn watch(&self, emit: &mut dyn FnMut(Result<AudioSnapshot>), stop: &dyn StopSignal);
    /// A success acknowledges the backend operation. Observations remain the
    /// source of truth. Known-stale identities fail; commands are not atomic with
    /// independent server-side device changes. Cancellation cannot undo a
    /// command already accepted by the service.
    fn control(&self, command: AudioCommand) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_values_reject_invalid_identity_and_nonfinite_gain() {
        assert!(AudioObjectId::new(0).is_err());
        assert_eq!(AudioObjectId::new(1).unwrap().value(), 1);
        for invalid in ["", "line\nname", "embedded\0nul"] {
            assert!(AudioDeviceKey::new(invalid).is_err());
        }
        assert!(AudioDeviceKey::new("a".repeat(1025)).is_err());
        assert_eq!(
            AudioDeviceKey::new("output.speaker").unwrap().as_str(),
            "output.speaker"
        );
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1] {
            assert!(AudioVolume::new(invalid).is_err());
        }
        assert_eq!(AudioVolume::new(1.25).unwrap().ratio(), 1.25);
        assert_eq!(AudioVolume::new(0.).unwrap(), AudioVolume::SILENCE);
    }
}
