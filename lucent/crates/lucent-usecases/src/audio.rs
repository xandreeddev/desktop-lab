//! Default-output keyboard intent. Device access belongs to the audio adapter.
use lucent_domain::*;
/// Resolve against the latest acknowledged output. Raising stops at the normal
/// maximum; lowering an amplified value reduces it gradually, preserving the
/// existing desktop keybinding behavior. A slider supplies an absolute command.
pub fn resolve_default_audio(command: AudioCommand, device: &AudioDevice) -> Result<AudioCommand> {
    if device.direction != AudioDirection::Playback {
        return Err(DomainError::Invalid(
            "Default output is not a playback device".into(),
        ));
    }
    Ok(match command {
        AudioCommand::Raise => AudioCommand::SetVolume(
            device.id,
            AudioVolume::new((device.volume.ratio() + 0.05).min(1.))?,
        ),
        AudioCommand::Lower => AudioCommand::SetVolume(
            device.id,
            AudioVolume::new((device.volume.ratio() - 0.05).max(0.))?,
        ),
        AudioCommand::ToggleMute => AudioCommand::SetMute(device.id, !device.muted),
        _ => {
            return Err(DomainError::Invalid(
                "Expected a default-output keyboard action".into(),
            ));
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_output_steps_preserve_amplified_lowering_and_clamp_silence() {
        let mut device = AudioDevice {
            id: AudioObjectId::new(1).unwrap(),
            key: AudioDeviceKey::new("test").unwrap(),
            name: "test".into(),
            direction: AudioDirection::Playback,
            volume: AudioVolume::new(1.25).unwrap(),
            muted: true,
            monitor: false,
        };
        assert_eq!(
            resolve_default_audio(AudioCommand::Lower, &device).unwrap(),
            AudioCommand::SetVolume(device.id, AudioVolume::new(1.20).unwrap())
        );
        assert_eq!(
            resolve_default_audio(AudioCommand::Raise, &device).unwrap(),
            AudioCommand::SetVolume(device.id, AudioVolume::NORMAL)
        );
        assert_eq!(
            resolve_default_audio(AudioCommand::ToggleMute, &device).unwrap(),
            AudioCommand::SetMute(device.id, false)
        );
        device.volume = AudioVolume::SILENCE;
        assert_eq!(
            resolve_default_audio(AudioCommand::Lower, &device).unwrap(),
            AudioCommand::SetVolume(device.id, AudioVolume::SILENCE)
        );
        device.direction = AudioDirection::Capture;
        assert!(resolve_default_audio(AudioCommand::Raise, &device).is_err());
    }
}
