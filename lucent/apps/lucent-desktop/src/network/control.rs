//! Public UI intent contains identities, never credentials or backend paths.
use lucent_domain::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    Wifi(bool),
    Scan(NetworkObjectId),
    Activate(NetworkObjectId, Option<NetworkObjectId>),
    Disconnect(NetworkObjectId),
    Forget(NetworkObjectId),
}
impl Control {
    pub fn valid(&self, snapshot: Option<&NetworkSnapshot>) -> bool {
        let Some(s) = snapshot else { return false };
        match self {
            Self::Wifi(enabled) => !enabled || s.wifi_hardware_enabled,
            Self::Scan(id) => {
                s.wifi_enabled
                    && s.wifi_hardware_enabled
                    && s.device(*id)
                        .is_some_and(|d| d.kind == NetworkKind::Wifi && d.managed)
            }
            Self::Activate(id, device) => s.profile(*id).is_some_and(|p| {
                device.is_none_or(|id| s.device(id).is_some_and(|d| d.managed && d.kind == p.kind))
            }),
            Self::Disconnect(id) => s.connection(*id).is_some(),
            Self::Forget(id) => s.profile(*id).is_some(),
        }
    }
    pub fn command(&self) -> NetworkCommand {
        match *self {
            Self::Wifi(value) => NetworkCommand::SetWifiEnabled(value),
            Self::Scan(id) => NetworkCommand::Scan(id),
            Self::Activate(profile, device) => NetworkCommand::Activate { profile, device },
            Self::Disconnect(id) => NetworkCommand::Disconnect(id),
            Self::Forget(id) => NetworkCommand::Forget(id),
        }
    }
}
