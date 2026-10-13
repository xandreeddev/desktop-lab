//! Validate intent against the current observation before entering FFI.
use super::snapshot::{Key, Kind, Objects, volume};
use lucent_domain::*;
use pulse::{PA_VOLUME_MAX, PA_VOLUME_NORM, pa_cvolume, pa_cvolume_scale};
use std::ffi::CString;

pub(super) enum Prepared {
    Volume(Key, pa_cvolume),
    Mute(Key, bool),
    Default(AudioDirection, CString),
    Route(AudioDirection, u32, u32),
}
pub(super) fn prepare(objects: &Objects, mut command: AudioCommand) -> Result<Prepared> {
    if matches!(
        command,
        AudioCommand::Raise | AudioCommand::Lower | AudioCommand::ToggleMute
    ) {
        let id = objects.default_output()?;
        let record = objects.resolve(id)?;
        command = lucent_usecases::audio::resolve_default_audio(command, &record.device(id)?)?;
    }
    match command {
        AudioCommand::SetVolume(id, desired) => {
            let record = objects.resolve(id)?;
            if !record.writable {
                return Err(DomainError::Unavailable(
                    "This audio object's volume is read-only".into(),
                ));
            }
            let mut channels = record.volumes.ok_or_else(|| {
                DomainError::Unavailable("This audio object has no volume control".into())
            })?;
            volume(&channels)?;
            let value = desired.ratio() * f64::from(PA_VOLUME_NORM);
            if value > f64::from(PA_VOLUME_MAX) {
                return Err(DomainError::Invalid(
                    "Requested volume exceeds the audio service limit".into(),
                ));
            }
            // SAFETY: validated, fully initialized volume; C scales it in place.
            if unsafe { pa_cvolume_scale(&mut channels, value.round() as u32) }.is_null() {
                return Err(DomainError::Invalid(
                    "Could not scale audio channels".into(),
                ));
            }
            Ok(Prepared::Volume(record.key, channels))
        }
        AudioCommand::SetMute(id, muted) => Ok(Prepared::Mute(objects.resolve(id)?.key, muted)),
        AudioCommand::SetDefault(id) => {
            let record = objects.resolve(id)?;
            if !record.key.0.device() {
                return Err(DomainError::Invalid(
                    "An application stream cannot be a default device".into(),
                ));
            }
            AudioDeviceKey::new(record.name.clone())?;
            Ok(Prepared::Default(
                record.key.0.direction(),
                CString::new(record.name.as_str())
                    .map_err(|_| DomainError::Invalid("Invalid device name".into()))?,
            ))
        }
        AudioCommand::Route { stream, device } => {
            let stream = objects.resolve(stream)?;
            let device = objects.resolve(device)?;
            match (stream.key.0, device.key.0) {
                (Kind::Playback, Kind::Output) | (Kind::Capture, Kind::Input) => Ok(
                    Prepared::Route(stream.key.0.direction(), stream.key.1, device.key.1),
                ),
                _ => Err(DomainError::Invalid(
                    "Audio stream and target device directions do not match".into(),
                )),
            }
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::snapshot::{Data, tests::record};
    #[test]
    fn audio_controls_preserve_balance_validate_routing_and_reject_stale_objects() {
        let mut data = Data::default();
        for (kind, index) in [
            (Kind::Output, 10),
            (Kind::Input, 20),
            (Kind::Playback, 30),
            (Kind::Capture, 40),
        ] {
            data.receive(Some(record(kind, index)));
        }
        let mut objects = Objects::default();
        let mut next = 0;
        let snapshot = objects
            .replace(data, &mut || {
                next += 1;
                AudioObjectId::new(next)
            })
            .unwrap();
        let output = snapshot
            .devices
            .iter()
            .find(|d| d.direction == AudioDirection::Playback)
            .unwrap()
            .id;
        let input = snapshot
            .devices
            .iter()
            .find(|d| d.direction == AudioDirection::Capture)
            .unwrap()
            .id;
        let playback = snapshot
            .streams
            .iter()
            .find(|s| s.direction == AudioDirection::Playback)
            .unwrap()
            .id;
        let capture = snapshot
            .streams
            .iter()
            .find(|s| s.direction == AudioDirection::Capture)
            .unwrap()
            .id;
        assert!(matches!(
            prepare(
                &objects,
                AudioCommand::Route {
                    stream: playback,
                    device: output
                }
            )
            .unwrap(),
            Prepared::Route(AudioDirection::Playback, 30, 10)
        ));
        assert!(matches!(
            prepare(
                &objects,
                AudioCommand::Route {
                    stream: capture,
                    device: input
                }
            )
            .unwrap(),
            Prepared::Route(AudioDirection::Capture, 40, 20)
        ));
        assert!(
            prepare(
                &objects,
                AudioCommand::Route {
                    stream: playback,
                    device: input
                }
            )
            .is_err()
        );
        assert!(prepare(&objects, AudioCommand::SetDefault(playback)).is_err());
        assert!(
            prepare(
                &objects,
                AudioCommand::SetMute(AudioObjectId::new(99).unwrap(), true)
            )
            .is_err()
        );
        objects
            .entries
            .get_mut(&Key(Kind::Output, 10))
            .unwrap()
            .1
            .volumes
            .as_mut()
            .unwrap()
            .values[1] = PA_VOLUME_NORM / 2;
        let Prepared::Volume(_, channels) = prepare(
            &objects,
            AudioCommand::SetVolume(output, AudioVolume::new(0.5).unwrap()),
        )
        .unwrap() else {
            panic!("volume command")
        };
        assert_eq!(
            channels.values[..2],
            [PA_VOLUME_NORM / 2, PA_VOLUME_NORM / 4]
        );
        assert!(
            prepare(
                &objects,
                AudioCommand::SetVolume(output, AudioVolume::new(f64::MAX).unwrap())
            )
            .is_err()
        );
        objects
            .entries
            .get_mut(&Key(Kind::Playback, 30))
            .unwrap()
            .1
            .writable = false;
        assert!(
            prepare(
                &objects,
                AudioCommand::SetVolume(playback, AudioVolume::NORMAL)
            )
            .is_err()
        );
    }
}
