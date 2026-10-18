//! Current-target media admission and monotonic progress projection. No clocks,
//! service calls, native identities or presentation policy are owned here.
use lucent_domain::*;
use std::time::Instant;

pub fn validate_media_command(snapshot: &MediaSnapshot, command: &MediaCommand) -> Result<()> {
    let target = command.player();
    let mut matches = snapshot.players.iter().filter(|player| player.id == target);
    let Some(player) = matches.next().filter(|_| matches.next().is_none()) else {
        return Err(DomainError::Invalid("Choose a current media player".into()));
    };
    let MediaPlayerState::Ready(playback) = &player.state else {
        return Err(DomainError::Unavailable(
            "Media player is unavailable".into(),
        ));
    };
    let capabilities = &playback.capabilities;
    let available = match command {
        MediaCommand::Raise(_) => capabilities.can_raise,
        MediaCommand::Play(_) => capabilities.can_control && capabilities.can_play,
        MediaCommand::Pause(_) => capabilities.can_control && capabilities.can_pause,
        MediaCommand::Next(_) => capabilities.can_control && capabilities.can_next,
        MediaCommand::Previous(_) => capabilities.can_control && capabilities.can_previous,
        MediaCommand::Stop(_) => capabilities.can_control,
        MediaCommand::SetPosition {
            track, position, ..
        } => {
            if !capabilities.can_control || !capabilities.can_seek {
                false
            } else {
                let Some(current) = playback
                    .track
                    .as_ref()
                    .filter(|current| current.id == *track)
                else {
                    return Err(DomainError::Invalid(
                        "Choose the current media track before seeking".into(),
                    ));
                };
                if current.length.is_some_and(|length| *position > length) {
                    return Err(DomainError::Invalid(
                        "Seek position exceeds the current track length".into(),
                    ));
                }
                true
            }
        }
    };
    if available {
        Ok(())
    } else {
        Err(DomainError::Invalid(
            "This media control is not currently available".into(),
        ))
    }
}

/// Project an observed position without fabricating a missing sample. Paused or
/// stopped players retain their observation. Rates, reverse playback, future
/// sample times and unknown track lengths cannot produce invalid native times.
pub fn project_media_position(playback: &MediaPlayback, now: Instant) -> Option<MediaTime> {
    let sample = playback.position?;
    let mut position = sample.position.micros();
    if playback.status == PlaybackStatus::Playing {
        let elapsed = now
            .checked_duration_since(sample.sampled_at)
            .unwrap_or_default();
        let advance = elapsed.as_secs_f64() * sample.rate.value().abs() * 1_000_000.;
        let advance = if !advance.is_finite() || advance >= MediaTime::MAX_MICROS as f64 {
            MediaTime::MAX_MICROS
        } else {
            advance as u64
        };
        position = if sample.rate.value().is_sign_negative() {
            position.saturating_sub(advance)
        } else {
            position.saturating_add(advance).min(MediaTime::MAX_MICROS)
        };
    }
    if let Some(length) = playback.track.as_ref().and_then(|track| track.length) {
        position = position.min(length.micros());
    }
    MediaTime::new(position).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn track(id: u64, length: Option<u64>) -> MediaTrack {
        MediaTrack {
            id: MediaTrackId::new(id).unwrap(),
            title: "Fixture track".into(),
            artists: vec!["Fixture artist".into()],
            album: "Fixture album".into(),
            length: length.map(|value| MediaTime::new(value).unwrap()),
            artwork: None,
        }
    }
    fn playback() -> MediaPlayback {
        MediaPlayback {
            status: PlaybackStatus::Playing,
            capabilities: MediaCapabilities {
                can_control: true,
                can_play: true,
                can_pause: true,
                can_next: true,
                can_previous: true,
                can_seek: true,
                can_raise: true,
            },
            track: Some(track(10, Some(120_000_000))),
            position: None,
        }
    }
    fn snapshot(playback: MediaPlayback) -> MediaSnapshot {
        MediaSnapshot {
            players: vec![MediaPlayer {
                id: MediaPlayerId::new(1).unwrap(),
                name: "Fixture player".into(),
                desktop_entry: None,
                state: MediaPlayerState::Ready(playback),
            }],
            operation: None,
        }
    }
    #[test]
    fn progress_uses_the_supplied_time_rate_and_observed_status() {
        let sampled_at = Instant::now();
        let mut state = playback();
        assert_eq!(project_media_position(&state, sampled_at), None);
        state.position = Some(MediaPositionSample {
            position: MediaTime::new(10_000_000).unwrap(),
            sampled_at,
            rate: MediaRate::new(0.5).unwrap(),
        });
        let later = sampled_at + Duration::from_secs(3);
        assert_eq!(
            project_media_position(&state, later).unwrap().micros(),
            11_500_000
        );
        assert_eq!(
            project_media_position(&state, sampled_at - Duration::from_secs(1))
                .unwrap()
                .micros(),
            10_000_000
        );
        for status in [PlaybackStatus::Paused, PlaybackStatus::Stopped] {
            state.status = status;
            assert_eq!(
                project_media_position(&state, later).unwrap().micros(),
                10_000_000
            );
        }
        assert_eq!(state.position.unwrap().position.micros(), 10_000_000);
    }
    #[test]
    fn progress_clamps_reverse_known_duration_and_extreme_rates_without_overflow() {
        let sampled_at = Instant::now();
        let mut state = playback();
        state.position = Some(MediaPositionSample {
            position: MediaTime::new(10_000_000).unwrap(),
            sampled_at,
            rate: MediaRate::new(-2.).unwrap(),
        });
        assert_eq!(
            project_media_position(&state, sampled_at + Duration::from_secs(6))
                .unwrap()
                .micros(),
            0
        );
        state.position.as_mut().unwrap().rate = MediaRate::new(2.).unwrap();
        assert_eq!(
            project_media_position(&state, sampled_at + Duration::from_secs(100))
                .unwrap()
                .micros(),
            120_000_000
        );
        state.track.as_mut().unwrap().length = None;
        assert_eq!(
            project_media_position(&state, sampled_at + Duration::from_secs(100))
                .unwrap()
                .micros(),
            210_000_000
        );
        state.position.as_mut().unwrap().rate = MediaRate::new(f64::MAX).unwrap();
        assert_eq!(
            project_media_position(&state, sampled_at + Duration::from_secs(1))
                .unwrap()
                .micros(),
            MediaTime::MAX_MICROS
        );
        state.position.as_mut().unwrap().rate = MediaRate::new(-f64::MAX).unwrap();
        assert_eq!(
            project_media_position(&state, sampled_at + Duration::from_secs(1))
                .unwrap()
                .micros(),
            0
        );
    }
    #[test]
    fn commands_require_a_unique_current_available_player_lifetime() {
        let id = MediaPlayerId::new(1).unwrap();
        let mut state = snapshot(playback());
        assert!(validate_media_command(&state, &MediaCommand::Pause(id)).is_ok());
        assert!(
            validate_media_command(&state, &MediaCommand::Pause(MediaPlayerId::new(2).unwrap()))
                .is_err()
        );
        state.players.push(state.players[0].clone());
        assert!(validate_media_command(&state, &MediaCommand::Pause(id)).is_err());
        state.players.pop();
        state.players[0].state = MediaPlayerState::Unavailable("Connection retired".into());
        assert!(validate_media_command(&state, &MediaCommand::Pause(id)).is_err());
        state.players[0].id = MediaPlayerId::new(3).unwrap();
        state.players[0].state = MediaPlayerState::Ready(playback());
        assert!(validate_media_command(&state, &MediaCommand::Pause(id)).is_err());
    }
    #[test]
    fn commands_obey_each_current_capability_and_raise_is_independent() {
        let id = MediaPlayerId::new(1).unwrap();
        let commands = [
            MediaCommand::Play(id),
            MediaCommand::Pause(id),
            MediaCommand::Next(id),
            MediaCommand::Previous(id),
            MediaCommand::Stop(id),
            MediaCommand::Raise(id),
        ];
        for command in commands {
            let mut observed = playback();
            assert!(validate_media_command(&snapshot(observed.clone()), &command).is_ok());
            match command {
                MediaCommand::Play(_) => observed.capabilities.can_play = false,
                MediaCommand::Pause(_) => observed.capabilities.can_pause = false,
                MediaCommand::Next(_) => observed.capabilities.can_next = false,
                MediaCommand::Previous(_) => observed.capabilities.can_previous = false,
                MediaCommand::Stop(_) => observed.capabilities.can_control = false,
                MediaCommand::Raise(_) => observed.capabilities.can_raise = false,
                _ => unreachable!(),
            }
            assert!(validate_media_command(&snapshot(observed), &command).is_err());
        }
        let mut observed = playback();
        observed.capabilities.can_control = false;
        for command in commands
            .into_iter()
            .filter(|command| !matches!(command, MediaCommand::Raise(_)))
        {
            assert!(validate_media_command(&snapshot(observed.clone()), &command).is_err());
        }
        assert!(validate_media_command(&snapshot(observed), &MediaCommand::Raise(id)).is_ok());
    }
    #[test]
    fn seeking_validates_track_lifetime_position_and_current_permission() {
        let id = MediaPlayerId::new(1).unwrap();
        let command = |track_id, micros| MediaCommand::SetPosition {
            player: id,
            track: MediaTrackId::new(track_id).unwrap(),
            position: MediaTime::new(micros).unwrap(),
        };
        let mut observed = playback();
        assert!(
            validate_media_command(&snapshot(observed.clone()), &command(10, 120_000_000)).is_ok()
        );
        assert!(
            validate_media_command(&snapshot(observed.clone()), &command(10, 120_000_001)).is_err()
        );
        assert!(validate_media_command(&snapshot(observed.clone()), &command(11, 10)).is_err());
        observed.capabilities.can_seek = false;
        assert!(validate_media_command(&snapshot(observed.clone()), &command(10, 10)).is_err());
        observed.capabilities.can_seek = true;
        observed.capabilities.can_control = false;
        assert!(validate_media_command(&snapshot(observed.clone()), &command(10, 10)).is_err());
        observed.capabilities.can_control = true;
        observed.track.as_mut().unwrap().length = None;
        assert!(
            validate_media_command(
                &snapshot(observed.clone()),
                &command(10, MediaTime::MAX_MICROS)
            )
            .is_ok()
        );
        observed.track = None;
        assert!(validate_media_command(&snapshot(observed), &command(10, 0)).is_err());
    }
}
