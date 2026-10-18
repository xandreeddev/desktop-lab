//! Observed media players and target-bound intents. Native owners, object paths,
//! protocol values, artwork IO and rendering remain outside the domain.
use crate::{AppId, DomainError, Result, StopSignal};
use std::time::Instant;

/// A transient observed player lifetime. Never save or reuse it after retirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MediaPlayerId(u64);
impl MediaPlayerId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid(
                "Media player identity is empty".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}

/// A transient metadata lifetime, separate from a native player's track path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MediaTrackId(u64);
impl MediaTrackId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid("Media track identity is empty".into()));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}

/// A transient command admission, unique for this adapter instance's lifetime.
/// It is distinct from both player and track identities and is never persisted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MediaOperationId(u64);
impl MediaOperationId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid(
                "Media operation identity is empty".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Nonnegative microseconds representable by native signed media timestamps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct MediaTime(u64);
impl MediaTime {
    pub const MAX_MICROS: u64 = i64::MAX as u64;
    pub fn new(micros: u64) -> Result<Self> {
        if micros > Self::MAX_MICROS {
            return Err(DomainError::Invalid(
                "Media time exceeds the supported range".into(),
            ));
        }
        Ok(Self(micros))
    }
    pub fn micros(self) -> u64 {
        self.0
    }
}

/// An observed finite nonzero speed. Negative playback rates run in reverse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaRate(f64);
impl MediaRate {
    pub fn new(value: f64) -> Result<Self> {
        if !value.is_finite() || value == 0. {
            return Err(DomainError::Invalid(
                "Media rate must be finite and nonzero".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> f64 {
        self.0
    }
}

/// A bounded artwork location; constructing it never opens a file or URL.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MediaArtworkSource(String);
impl MediaArtworkSource {
    pub const MAX_BYTES: usize = 2048;
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= Self::MAX_BYTES
            && !value
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
            && valid_escapes(&value)
            && value.split_once("://").is_some_and(|(scheme, rest)| {
                let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                let authority = &rest[..end];
                match scheme.to_ascii_lowercase().as_str() {
                    "file" => {
                        (authority.is_empty() || authority.eq_ignore_ascii_case("localhost"))
                            && rest[end..].starts_with('/')
                            && !rest[end..].contains(['?', '#'])
                    }
                    "http" | "https" => valid_authority(authority),
                    _ => false,
                }
            });
        if !valid {
            return Err(DomainError::Invalid(
                "Media artwork source is not supported".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(&self) -> &str {
        &self.0
    }
}

fn valid_escapes(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            if !bytes.get(at + 1).is_some_and(u8::is_ascii_hexdigit)
                || !bytes.get(at + 2).is_some_and(u8::is_ascii_hexdigit)
            {
                return false;
            }
            at += 3;
        } else {
            at += 1;
        }
    }
    true
}

fn valid_authority(authority: &str) -> bool {
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some((host, tail)) = bracketed.split_once(']') else {
            return false;
        };
        if host.is_empty()
            || !host.contains(':')
            || !host
                .chars()
                .all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '.'))
        {
            return false;
        }
        if tail.is_empty() {
            return true;
        }
        let Some(port) = tail.strip_prefix(':') else {
            return false;
        };
        (host, Some(port))
    } else {
        let (host, port) = authority
            .rsplit_once(':')
            .map_or((authority, None), |(host, port)| (host, Some(port)));
        if host.is_empty()
            || !host
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '.' | '_'))
        {
            return false;
        }
        (host, port)
    };
    !host.is_empty()
        && port.is_none_or(|port| {
            !port.is_empty()
                && port.bytes().all(|b| b.is_ascii_digit())
                && port.parse::<u16>().is_ok()
        })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    #[default]
    Stopped,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaCapabilities {
    pub can_control: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    pub can_raise: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaTrack {
    pub id: MediaTrackId,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub length: Option<MediaTime>,
    pub artwork: Option<MediaArtworkSource>,
}

/// The adapter supplies a monotonic sample time. Projection receives its own
/// `now` input rather than reading a clock while applying pure policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaPositionSample {
    pub position: MediaTime,
    pub sampled_at: Instant,
    pub rate: MediaRate,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaPlayback {
    pub status: PlaybackStatus,
    pub capabilities: MediaCapabilities,
    pub track: Option<MediaTrack>,
    pub position: Option<MediaPositionSample>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MediaPlayerState {
    Ready(MediaPlayback),
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MediaPlayer {
    pub id: MediaPlayerId,
    pub name: String,
    pub desktop_entry: Option<AppId>,
    pub state: MediaPlayerState,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaSnapshot {
    pub players: Vec<MediaPlayer>,
    /// Current admitted operation, retained after termination. Player playback
    /// remains an independent observation, including after acknowledgement.
    pub operation: Option<MediaOperation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCommand {
    Play(MediaPlayerId),
    Pause(MediaPlayerId),
    Next(MediaPlayerId),
    Previous(MediaPlayerId),
    Stop(MediaPlayerId),
    Raise(MediaPlayerId),
    SetPosition {
        player: MediaPlayerId,
        track: MediaTrackId,
        position: MediaTime,
    },
}
impl MediaCommand {
    pub fn player(self) -> MediaPlayerId {
        match self {
            Self::Play(id)
            | Self::Pause(id)
            | Self::Next(id)
            | Self::Previous(id)
            | Self::Stop(id)
            | Self::Raise(id)
            | Self::SetPosition { player: id, .. } => id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaOperationPhase {
    /// Locally admitted; native validation or dispatch may still be pending.
    InProgress,
    /// The addressed native method replied successfully, not an observed state.
    Acknowledged,
    /// Rejection or transport failure. An unavailable outcome may be uncertain:
    /// it does not establish that a dispatched method had no effect.
    Failed(DomainError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaOperation {
    pub id: MediaOperationId,
    pub command: MediaCommand,
    pub phase: MediaOperationPhase,
}

pub trait MediaPort: Send + Sync {
    /// Observe current player lifetimes until cancelled. An empty successful
    /// snapshot means no players; an error means observation is unavailable.
    fn watch(&self, emit: &mut dyn FnMut(Result<MediaSnapshot>), stop: &dyn StopSignal);
    /// Admit one operation to a bounded local mailbox without waiting for native
    /// validation or a method reply. Busy/offline/full admission fails without
    /// dispatch. Watch the returned identity for method acknowledgement/error;
    /// admission never changes observed playback or guarantees target validity.
    /// Implementations revalidate player/track ownership before dispatch and
    /// retain terminal results until published. Watch failure makes a pending
    /// outcome uncertain; closing a client does not undo an admitted action.
    fn start(&self, command: MediaCommand) -> Result<MediaOperationId>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transient_handles_and_native_time_rate_ranges_reject_invalid_values() {
        assert!(MediaPlayerId::new(0).is_err());
        assert!(MediaTrackId::new(0).is_err());
        assert!(MediaOperationId::new(0).is_err());
        assert_eq!(MediaPlayerId::new(1).unwrap().value(), 1);
        assert_eq!(MediaTrackId::new(u64::MAX).unwrap().value(), u64::MAX);
        assert_eq!(MediaOperationId::new(u64::MAX).unwrap().value(), u64::MAX);
        assert_eq!(MediaTime::new(0).unwrap().micros(), 0);
        assert_eq!(
            MediaTime::new(i64::MAX as u64).unwrap().micros(),
            i64::MAX as u64
        );
        assert!(MediaTime::new(i64::MAX as u64 + 1).is_err());
        for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0., -0.] {
            assert!(MediaRate::new(rate).is_err());
        }
        for rate in [-2., 0.5, 1., 2.] {
            assert_eq!(MediaRate::new(rate).unwrap().value(), rate);
        }
    }

    #[test]
    fn artwork_is_bounded_local_file_or_http_content_without_native_operations() {
        for source in [
            "file:///home/demo/cover%20art.png",
            "file://localhost/tmp/cover.png",
            "FILE://LOCALHOST/tmp/cover.png",
            "https://example.test/cover.png?size=100",
            "http://127.0.0.1:8080/cover",
            "https://[::1]:8443/cover",
        ] {
            assert_eq!(MediaArtworkSource::new(source).unwrap().value(), source);
        }
        for source in [
            "",
            "/tmp/cover.png",
            "file://server/cover",
            "file:/tmp/cover",
            "file:///tmp/cover?query",
            "file:///tmp/cover#fragment",
            "data:image/png,x",
            "ftp://example.test/a",
            "https://",
            "https:///cover",
            "https://?q=cover",
            "https://user:pass@example.test/cover",
            "https://example.test:bad/cover",
            "https://example.test:65536/cover",
            "https://[::1/cover",
            "https://[]/cover",
            "https://example.test/a b",
            "https://example.test/a\n",
            "https://example.test/\\cover",
            "https://example.test/%",
            "https://example.test/%0g",
        ] {
            assert!(MediaArtworkSource::new(source).is_err(), "{source}");
        }
        assert!(
            MediaArtworkSource::new(format!(
                "https://example.test/{}",
                "x".repeat(MediaArtworkSource::MAX_BYTES)
            ))
            .is_err()
        );
    }
}
