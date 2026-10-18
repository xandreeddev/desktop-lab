//! Bounded native decoding. Native object paths remain private authority; only
//! adapter-assigned incarnations reach the pure media model.
use super::*;
use std::collections::HashMap;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

pub(super) type Properties = HashMap<String, OwnedValue>;
pub(super) struct Raw {
    pub root: Properties,
    pub player: Properties,
    pub sampled_at: Instant,
}
pub(super) fn invalid() -> DomainError {
    DomainError::Failed("Media player returned an invalid observation".into())
}
pub(super) fn field<T: TryFrom<OwnedValue>>(properties: &Properties, name: &str) -> Option<T> {
    T::try_from(properties.get(name)?.try_clone().ok()?).ok()
}
fn text(value: &str, bytes: usize) -> String {
    let mut output = String::new();
    for character in value.chars().filter(|c| !c.is_control()) {
        if output.len() + character.len_utf8() > bytes {
            break;
        }
        output.push(character);
    }
    output
}
pub(super) fn track_path(metadata: &Properties) -> Option<OwnedObjectPath> {
    field::<OwnedObjectPath>(metadata, "mpris:trackid")
        .filter(|path| path.as_str() != "/org/mpris/MediaPlayer2/TrackList/NoTrack")
}
pub(super) fn metadata(properties: &Properties) -> Properties {
    field::<Properties>(properties, "Metadata")
        .filter(|metadata| metadata.len() <= 64)
        .unwrap_or_default()
}
pub(super) fn decode(
    adapter: &Mpris,
    id: MediaPlayerId,
    raw: Raw,
    previous_path: Option<&OwnedObjectPath>,
    previous_track: Option<MediaTrackId>,
) -> Result<(MediaPlayer, Option<OwnedObjectPath>)> {
    if raw.root.len() > 64 || raw.player.len() > 64 {
        return Err(invalid());
    }
    let status = match field::<String>(&raw.player, "PlaybackStatus").as_deref() {
        Some("Playing") => PlaybackStatus::Playing,
        Some("Paused") => PlaybackStatus::Paused,
        Some("Stopped") => PlaybackStatus::Stopped,
        _ => return Err(invalid()),
    };
    let rate = MediaRate::new(field::<f64>(&raw.player, "Rate").ok_or_else(invalid)?)?;
    let can_control = field(&raw.player, "CanControl").unwrap_or(false);
    let capabilities = MediaCapabilities {
        can_control,
        can_play: can_control && field(&raw.player, "CanPlay").unwrap_or(false),
        can_pause: can_control && field(&raw.player, "CanPause").unwrap_or(false),
        can_next: can_control && field(&raw.player, "CanGoNext").unwrap_or(false),
        can_previous: can_control && field(&raw.player, "CanGoPrevious").unwrap_or(false),
        can_seek: can_control && field(&raw.player, "CanSeek").unwrap_or(false),
        can_raise: field(&raw.root, "CanRaise").unwrap_or(false),
    };
    let metadata = metadata(&raw.player);
    let path = track_path(&metadata);
    let track = path
        .as_ref()
        .map(|path| -> Result<MediaTrack> {
            let track_id = match (previous_path, previous_track) {
                (Some(previous), Some(id)) if previous == path => id,
                _ => MediaTrackId::new(adapter.identity()?)?,
            };
            Ok(MediaTrack {
                id: track_id,
                title: text(
                    &field::<String>(&metadata, "xesam:title").unwrap_or_default(),
                    1024,
                ),
                artists: field::<Vec<String>>(&metadata, "xesam:artist")
                    .unwrap_or_default()
                    .into_iter()
                    .take(16)
                    .map(|artist| text(&artist, 256))
                    .collect(),
                album: text(
                    &field::<String>(&metadata, "xesam:album").unwrap_or_default(),
                    1024,
                ),
                length: field::<i64>(&metadata, "mpris:length")
                    .and_then(|value| u64::try_from(value).ok())
                    .and_then(|value| MediaTime::new(value).ok()),
                artwork: field::<String>(&metadata, "mpris:artUrl")
                    .and_then(|value| MediaArtworkSource::new(value).ok()),
            })
        })
        .transpose()?;
    let position = field::<i64>(&raw.player, "Position")
        .and_then(|value| u64::try_from(value).ok())
        .and_then(|value| MediaTime::new(value).ok())
        .map(|position| MediaPositionSample {
            position,
            sampled_at: raw.sampled_at,
            rate,
        });
    let name = text(
        &field::<String>(&raw.root, "Identity").unwrap_or_default(),
        256,
    );
    let desktop_entry = field::<String>(&raw.root, "DesktopEntry")
        .filter(|name| !name.is_empty() && name.len() <= 512)
        .and_then(|name| {
            AppId::new(if name.ends_with(".desktop") {
                name
            } else {
                format!("{name}.desktop")
            })
            .ok()
        });
    Ok((
        MediaPlayer {
            id,
            name: if name.is_empty() {
                "Media player".into()
            } else {
                name
            },
            desktop_entry,
            state: MediaPlayerState::Ready(MediaPlayback {
                status,
                capabilities,
                track,
                position,
            }),
        },
        path,
    ))
}
