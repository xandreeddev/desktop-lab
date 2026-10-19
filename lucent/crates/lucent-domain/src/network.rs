//! Network observations and intent, independent of D-Bus and UI geometry.
use crate::{DomainError, Result, Secret, StopSignal};

/// Temporary identity scoped to an adapter. Removal or service reconnection
/// retires it. Never persist it or infer a backend path from its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NetworkObjectId(u64);
impl NetworkObjectId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid(
                "Network object identity is empty".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}

/// An SSID is 1–32 octets, not necessarily UTF-8. Display text cannot be used
/// to reconstruct it; connection requests retain the original bytes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NetworkSsid(Vec<u8>);
impl NetworkSsid {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self> {
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > 32 {
            return Err(DomainError::Invalid(
                "Wi-Fi name must contain 1–32 bytes".into(),
            ));
        }
        Ok(Self(bytes))
    }
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn display_name(&self) -> String {
        String::from_utf8_lossy(&self.0)
            .chars()
            .map(|c| if c.is_control() { '\u{fffd}' } else { c })
            .collect()
    }
}

/// A saved profile's stable key is distinct from its temporary control handle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkProfileKey(String);
impl NetworkProfileKey {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
            return Err(DomainError::Invalid("Invalid network profile key".into()));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkStrength(u8);
impl NetworkStrength {
    pub fn new(percent: u8) -> Result<Self> {
        if percent > 100 {
            return Err(DomainError::Invalid(
                "Wi-Fi strength exceeds 100 percent".into(),
            ));
        }
        Ok(Self(percent))
    }
    pub fn percent(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkKind {
    Wired,
    Wifi,
    Other,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkState {
    Unknown,
    Asleep,
    Disconnected,
    Disconnecting,
    Connecting,
    Local,
    Site,
    Global,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkConnectivity {
    Unknown,
    None,
    Portal,
    Limited,
    Full,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkDeviceState {
    Unknown,
    Unmanaged,
    Unavailable,
    Disconnected,
    Connecting,
    NeedsAuthentication,
    Connected,
    Disconnecting,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkFailure {
    None,
    Unknown,
    Authentication,
    Configuration,
    Address,
    Service,
    Removed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkConnectionState {
    Unknown,
    Connecting,
    Connected,
    Disconnecting,
    Disconnected,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiSecurity {
    Open,
    WpaPersonal,
    Wpa3Personal,
    EnhancedOpen,
    Enterprise,
    Wep,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkDevice {
    pub id: NetworkObjectId,
    pub interface: String,
    pub kind: NetworkKind,
    pub managed: bool,
    pub state: NetworkDeviceState,
    pub failure: NetworkFailure,
    pub active_connection: Option<NetworkObjectId>,
    pub active_access_point: Option<NetworkObjectId>,
    /// Changes after a completed scan. None means no completed scan is known;
    /// clients compare generations instead of treating request acceptance as completion.
    pub scan_generation: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkProfile {
    pub id: NetworkObjectId,
    pub key: NetworkProfileKey,
    pub name: String,
    pub kind: NetworkKind,
    pub ssid: Option<NetworkSsid>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkAccessPoint {
    pub id: NetworkObjectId,
    pub device: NetworkObjectId,
    /// Hidden access points may not advertise a name.
    pub ssid: Option<NetworkSsid>,
    pub strength: NetworkStrength,
    pub security: WifiSecurity,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkConnection {
    pub id: NetworkObjectId,
    pub profile: Option<NetworkObjectId>,
    pub name: String,
    pub kind: NetworkKind,
    pub devices: Vec<NetworkObjectId>,
    pub state: NetworkConnectionState,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub state: NetworkState,
    pub connectivity: NetworkConnectivity,
    pub wifi_enabled: bool,
    pub wifi_hardware_enabled: bool,
    pub primary_connection: Option<NetworkObjectId>,
    pub devices: Vec<NetworkDevice>,
    pub profiles: Vec<NetworkProfile>,
    pub access_points: Vec<NetworkAccessPoint>,
    pub connections: Vec<NetworkConnection>,
}
impl NetworkSnapshot {
    pub fn device(&self, id: NetworkObjectId) -> Option<&NetworkDevice> {
        self.devices.iter().find(|v| v.id == id)
    }
    pub fn profile(&self, id: NetworkObjectId) -> Option<&NetworkProfile> {
        self.profiles.iter().find(|v| v.id == id)
    }
    pub fn access_point(&self, id: NetworkObjectId) -> Option<&NetworkAccessPoint> {
        self.access_points.iter().find(|v| v.id == id)
    }
    pub fn connection(&self, id: NetworkObjectId) -> Option<&NetworkConnection> {
        self.connections.iter().find(|v| v.id == id)
    }
}

/// Neither Clone nor Debug: a client consumes this value into a control effect.
/// Secrets never belong in snapshots, inspector trees or saved UI preferences.
pub struct WifiCredentials {
    security: WifiSecurity,
    password: Option<Secret>,
}
impl WifiCredentials {
    pub fn new(security: WifiSecurity, password: Option<Secret>) -> Result<Self> {
        let valid = match security {
            WifiSecurity::Open | WifiSecurity::EnhancedOpen => password.is_none(),
            WifiSecurity::WpaPersonal => password.as_ref().is_some_and(|p| {
                let bytes = p.expose().as_bytes();
                (bytes.len() == 64 && bytes.iter().all(u8::is_ascii_hexdigit))
                    || ((8..=63).contains(&bytes.len())
                        && bytes.iter().all(|b| (32..=126).contains(b)))
            }),
            WifiSecurity::Wpa3Personal => password.as_ref().is_some_and(|p| {
                !p.is_empty() && p.expose().len() <= 4096 && !p.expose().contains('\0')
            }),
            WifiSecurity::Enterprise | WifiSecurity::Wep | WifiSecurity::Unsupported => {
                return Err(DomainError::Invalid(
                    "This Wi-Fi security requires an existing configured profile".into(),
                ));
            }
        };
        if !valid {
            return Err(DomainError::Invalid(
                "Credentials do not match Wi-Fi security".into(),
            ));
        }
        Ok(Self { security, password })
    }
    pub fn security(&self) -> WifiSecurity {
        self.security
    }
    pub fn password(&self) -> Option<&Secret> {
        self.password.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkJoinTarget {
    AccessPoint(NetworkObjectId),
    Hidden {
        device: NetworkObjectId,
        ssid: NetworkSsid,
    },
}
/// Commands with credentials intentionally cannot be cloned or debug-printed.
pub enum NetworkCommand {
    SetWifiEnabled(bool),
    Scan(NetworkObjectId),
    Activate {
        profile: NetworkObjectId,
        device: Option<NetworkObjectId>,
    },
    Disconnect(NetworkObjectId),
    Forget(NetworkObjectId),
    Join {
        target: NetworkJoinTarget,
        credentials: WifiCredentials,
        remember: bool,
    },
}
/// A single interactive credential exchange, retired on cancellation, profile
/// replacement, daemon loss or response. It is not a saved profile identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NetworkCredentialRequestId(u64);
impl NetworkCredentialRequestId {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(DomainError::Invalid(
                "Network credential identity is empty".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}
/// Nonsecret description of an actual backend credential request. The stable
/// profile key describes the destination; only the live request authorizes reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkCredentialRequest {
    pub id: NetworkCredentialRequestId,
    pub profile: NetworkProfileKey,
    pub name: String,
    pub ssid: NetworkSsid,
    pub security: WifiSecurity,
    pub new_secret_required: bool,
}
pub trait NetworkPort: Send + Sync {
    /// One cancellable observer owns the native connection and reconnects. An
    /// error is unavailable state, never a fabricated disconnected snapshot.
    /// Do not call control synchronously from this callback: it runs on the owner.
    fn watch(&self, emit: &mut dyn FnMut(Result<NetworkSnapshot>), stop: &dyn StopSignal);
    /// Success acknowledges service acceptance, not connection/scan completion.
    /// Observations remain authoritative. A timeout can follow an accepted
    /// operation; cancellation cannot undo it. Forget deactivates then deletes,
    /// and is not atomic. Known-stale identities fail instead of retargeting.
    fn control(&self, command: NetworkCommand) -> Result<()>;
    /// Observe actual interactive requests independently of command admission.
    /// This must continue while a native activation is awaiting its reply. The
    /// snapshot contains no credentials; an empty list retires previous requests.
    fn watch_credentials(
        &self,
        emit: &mut dyn FnMut(Result<Vec<NetworkCredentialRequest>>),
        stop: &dyn StopSignal,
    ) {
        emit(Err(DomainError::Unavailable(
            "Network credential prompts are unavailable".into(),
        )));
        while !stop.cancelled() {
            stop.wait(std::time::Duration::from_secs(1));
        }
    }
    /// Consume a response to exactly one still-live request. None explicitly
    /// cancels it. Success acknowledges answer delivery, not association or save.
    /// Never retain/replay the credentials, even after an uncertain completion.
    fn answer_credentials(
        &self,
        _request: NetworkCredentialRequestId,
        _credentials: Option<WifiCredentials>,
    ) -> Result<()> {
        Err(DomainError::Unavailable(
            "Network credential responses are unavailable".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssid_retains_octets_and_never_turns_display_text_into_a_request() {
        let name = NetworkSsid::new(vec![0xff, b'a', b'\n', 0]).unwrap();
        assert_eq!(name.bytes(), &[0xff, b'a', b'\n', 0]);
        assert_eq!(name.display_name(), "\u{fffd}a\u{fffd}\u{fffd}");
        assert!(NetworkSsid::new(vec![]).is_err());
        assert!(NetworkSsid::new(vec![b'a'; 33]).is_err());
        assert!(NetworkSsid::new(vec![b'a'; 32]).is_ok());
    }
    #[test]
    fn handles_strength_and_stable_keys_have_distinct_validated_invariants() {
        assert!(NetworkCredentialRequestId::new(0).is_err());
        assert_eq!(NetworkCredentialRequestId::new(7).unwrap().value(), 7);
        assert!(NetworkObjectId::new(0).is_err());
        assert_eq!(NetworkObjectId::new(3).unwrap().value(), 3);
        assert!(NetworkStrength::new(101).is_err());
        assert_eq!(NetworkStrength::new(100).unwrap().percent(), 100);
        assert!(NetworkProfileKey::new("").is_err());
        assert!(NetworkProfileKey::new("private\nkey").is_err());
        assert!(NetworkProfileKey::new("a".repeat(1025)).is_err());
    }
    #[test]
    fn credentials_validate_without_echoing_secret_values() {
        let credentials =
            |kind, text: &str| WifiCredentials::new(kind, Some(Secret::new(text.to_owned())));
        assert!(WifiCredentials::new(WifiSecurity::Open, None).is_ok());
        assert!(credentials(WifiSecurity::Open, "abcdefgh").is_err());
        for text in ["short", "password\n", "åååååååå", &"x".repeat(64)] {
            assert!(credentials(WifiSecurity::WpaPersonal, text).is_err());
        }
        assert!(credentials(WifiSecurity::WpaPersonal, "abcdefgh").is_ok());
        assert!(credentials(WifiSecurity::WpaPersonal, &"a".repeat(64)).is_ok());
        assert!(credentials(WifiSecurity::Wpa3Personal, "å").is_ok());
        assert!(credentials(WifiSecurity::Wpa3Personal, "a\0b").is_err());
        assert!(credentials(WifiSecurity::Enterprise, "password").is_err());
        assert!(!credentials(WifiSecurity::WpaPersonal, "sensitive").is_err());
        assert!(
            !credentials(WifiSecurity::WpaPersonal, "secret")
                .err()
                .unwrap()
                .to_string()
                .contains("secret")
        );
    }
}
