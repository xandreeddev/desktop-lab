use super::{
    native::*,
    snapshot::{Kind, Objects},
    *,
};
use std::collections::HashMap;
use zbus::zvariant::{OwnedObjectPath, Value};

fn empty(message: zbus::Message) -> Result<()> {
    message.body().deserialize().map_err(|_| protocol())
}

pub(super) enum Plan {
    Wifi(bool),
    Scan(OwnedObjectPath),
    Activate {
        profile: OwnedObjectPath,
        device: OwnedObjectPath,
    },
    Disconnect(OwnedObjectPath),
    Forget {
        profile: OwnedObjectPath,
        active: Vec<OwnedObjectPath>,
    },
    Join {
        device: OwnedObjectPath,
        ap: OwnedObjectPath,
        ssid: NetworkSsid,
        credentials: WifiCredentials,
        hidden: bool,
        remember: bool,
    },
}
fn root() -> OwnedObjectPath {
    OwnedObjectPath::try_from("/").unwrap()
}
fn invalid(message: &str) -> DomainError {
    DomainError::Invalid(message.into())
}
pub(super) fn plan(
    command: NetworkCommand,
    objects: &Objects,
    snapshot: &NetworkSnapshot,
) -> Result<Plan> {
    let wifi = |id| {
        if snapshot
            .device(id)
            .is_none_or(|v| v.kind != NetworkKind::Wifi || !v.managed)
        {
            return Err(invalid("Choose a current managed Wi-Fi device"));
        }
        objects.resolve(id, Kind::Device)
    };
    Ok(match command {
        NetworkCommand::SetWifiEnabled(enabled) => Plan::Wifi(enabled),
        NetworkCommand::Scan(id) => Plan::Scan(wifi(id)?),
        NetworkCommand::Activate { profile, device } => {
            let p = snapshot
                .profile(profile)
                .ok_or_else(|| invalid("Saved network is stale; choose a current profile"))?;
            if let Some(device) = device {
                let d = snapshot
                    .device(device)
                    .ok_or_else(|| invalid("Network device is stale"))?;
                if !d.managed || (p.kind != NetworkKind::Other && p.kind != d.kind) {
                    return Err(invalid("Profile and device kinds do not match"));
                }
            }
            Plan::Activate {
                profile: objects.resolve(profile, Kind::Profile)?,
                device: device
                    .map(|v| objects.resolve(v, Kind::Device))
                    .transpose()?
                    .unwrap_or_else(root),
            }
        }
        NetworkCommand::Disconnect(id) => Plan::Disconnect(objects.resolve(id, Kind::Connection)?),
        NetworkCommand::Forget(id) => Plan::Forget {
            profile: objects.resolve(id, Kind::Profile)?,
            active: snapshot
                .connections
                .iter()
                .filter(|v| v.profile == Some(id))
                .map(|v| objects.resolve(v.id, Kind::Connection))
                .collect::<Result<_>>()?,
        },
        NetworkCommand::Join {
            target,
            credentials,
            remember,
        } => {
            let (device, ap, ssid, hidden) = match target {
                NetworkJoinTarget::AccessPoint(id) => {
                    let ap = snapshot
                        .access_point(id)
                        .ok_or_else(|| invalid("Wi-Fi network is stale; choose a current item"))?;
                    if ap.security != credentials.security() {
                        return Err(invalid("Wi-Fi security changed; choose the network again"));
                    }
                    (
                        wifi(ap.device)?,
                        objects.resolve(id, Kind::AccessPoint)?,
                        ap.ssid
                            .clone()
                            .ok_or_else(|| invalid("Enter the hidden network name first"))?,
                        false,
                    )
                }
                NetworkJoinTarget::Hidden { device, ssid } => (wifi(device)?, root(), ssid, true),
            };
            Plan::Join {
                device,
                ap,
                ssid,
                credentials,
                hidden,
                remember,
            }
        }
    })
}
pub(super) async fn apply(transport: &Transport, plan: Plan) -> Result<()> {
    match plan {
        Plan::Wifi(enabled) => {
            empty(
                transport
                    .call(
                        ROOT,
                        "org.freedesktop.DBus.Properties",
                        "Set",
                        &(NM, "WirelessEnabled", Value::from(enabled)),
                    )
                    .await?,
            )?;
        }
        Plan::Scan(device) => {
            empty(
                transport
                    .call(
                        device.as_str(),
                        WIFI,
                        "RequestScan",
                        &(HashMap::<&str, Value<'_>>::new(),),
                    )
                    .await?,
            )?;
        }
        Plan::Activate { profile, device } => {
            let _: OwnedObjectPath = transport
                .call(ROOT, NM, "ActivateConnection", &(profile, device, root()))
                .await?
                .body()
                .deserialize()
                .map_err(|_| protocol())?;
        }
        Plan::Disconnect(active) => {
            empty(
                transport
                    .call(ROOT, NM, "DeactivateConnection", &(active,))
                    .await?,
            )?;
        }
        Plan::Forget { profile, active } => {
            for active in active {
                empty(
                    transport
                        .call(ROOT, NM, "DeactivateConnection", &(active,))
                        .await?,
                )?;
            }
            empty(
                transport
                    .call(profile.as_str(), PROFILE, "Delete", &())
                    .await?,
            )?;
        }
        Plan::Join {
            device,
            ap,
            ssid,
            credentials,
            hidden,
            remember,
        } => {
            let settings = join_settings(&ssid, &credentials, hidden, remember);
            let options = HashMap::from([(
                "persist",
                Value::from(if remember { "disk" } else { "volatile" }),
            )]);
            let _: (OwnedObjectPath, OwnedObjectPath, Properties) = transport
                .call(
                    ROOT,
                    NM,
                    "AddAndActivateConnection2",
                    &(settings, device, ap, options),
                )
                .await?
                .body()
                .deserialize()
                .map_err(|_| protocol())?;
        }
    }
    Ok(())
}
// Borrow the password only while serializing. No extra owned plaintext string,
// subprocess argument, client snapshot or debug representation is constructed.
// D-Bus and NetworkManager still serialize/store secrets under their own policy.
fn join_settings<'a>(
    ssid: &'a NetworkSsid,
    credentials: &'a WifiCredentials,
    hidden: bool,
    remember: bool,
) -> HashMap<&'static str, HashMap<&'static str, Value<'a>>> {
    let mut settings = HashMap::from([
        (
            "connection",
            HashMap::from([
                ("type", Value::from("802-11-wireless")),
                ("autoconnect", Value::from(remember)),
            ]),
        ),
        (
            "802-11-wireless",
            HashMap::from([
                ("ssid", Value::from(ssid.bytes())),
                ("mode", Value::from("infrastructure")),
                ("hidden", Value::from(hidden)),
            ]),
        ),
        ("ipv4", HashMap::from([("method", Value::from("auto"))])),
        ("ipv6", HashMap::from([("method", Value::from("auto"))])),
    ]);
    let key = match credentials.security() {
        WifiSecurity::Open => None,
        WifiSecurity::WpaPersonal => Some("wpa-psk"),
        WifiSecurity::Wpa3Personal => Some("sae"),
        WifiSecurity::EnhancedOpen => Some("owe"),
        _ => unreachable!("credentials constructor rejects unsupported security"),
    };
    if let Some(key) = key {
        let mut security = HashMap::from([("key-mgmt", Value::from(key))]);
        if let Some(password) = credentials.password() {
            security.insert("psk", Value::from(password.expose()));
        }
        settings.insert("802-11-wireless-security", security);
    }
    settings
}
