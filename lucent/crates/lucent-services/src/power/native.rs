//! Native service discovery, validated observations and captured command plans.
use super::*;
use futures_lite::future;
use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
};
use zbus::{
    Connection, Proxy,
    zvariant::{OwnedObjectPath, OwnedValue},
};
pub(super) const UPOWER: &str = "org.freedesktop.UPower";
pub(super) const DEVICE: &str = "org.freedesktop.UPower.Device";
pub(super) const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
pub(super) const LEGACY_PROFILES: &str = "net.hadess.PowerProfiles";
pub(super) const LOGIN: &str = "org.freedesktop.login1";
pub(super) const MANAGER: &str = "org.freedesktop.login1.Manager";
pub(super) const SESSION: &str = "org.freedesktop.login1.Session";
type Properties = HashMap<String, OwnedValue>;
pub(super) fn unavailable(text: &str) -> DomainError {
    DomainError::Unavailable(text.into())
}
fn invalid() -> DomainError {
    DomainError::Failed("Power service returned an invalid observation".into())
}
pub(super) fn bus_error(error: zbus::Error) -> DomainError {
    match error {
        zbus::Error::MethodError(name, _, _) => match name.as_str() {
            "org.freedesktop.DBus.Error.AccessDenied"
            | "org.freedesktop.PolicyKit1.Error.NotAuthorized" => {
                DomainError::Failed("Power permission denied".into())
            }
            "org.freedesktop.DBus.Error.UnknownMethod"
            | "org.freedesktop.DBus.Error.UnknownObject" => {
                DomainError::Invalid("Power capability changed; choose a current control".into())
            }
            "org.freedesktop.DBus.Error.NameHasNoOwner"
            | "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NoReply"
            | "org.freedesktop.DBus.Error.Timeout"
            | "org.freedesktop.DBus.Error.Disconnected" => {
                unavailable("Power service is unavailable")
            }
            _ => DomainError::Failed("Power service rejected the request".into()),
        },
        _ => unavailable("Power transport is unavailable"),
    }
}
pub(super) async fn bounded<T>(
    operation: impl Future<Output = Result<T>>,
    deadline: Instant,
) -> Result<T> {
    future::or(operation, async {
        async_io::Timer::at(deadline).await;
        Err(unavailable("Power operation timed out"))
    })
    .await
}
fn field<T: TryFrom<OwnedValue>>(properties: &Properties, key: &str) -> Result<T> {
    T::try_from(
        properties
            .get(key)
            .ok_or_else(invalid)?
            .try_clone()
            .map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())
}
fn optional<T: TryFrom<OwnedValue>>(properties: &Properties, key: &str) -> Result<Option<T>> {
    if properties.contains_key(key) {
        field(properties, key).map(Some)
    } else {
        Ok(None)
    }
}
fn capability<T>(value: Result<T>) -> PowerCapability<T> {
    match value {
        Ok(value) => PowerCapability::Ready(value),
        Err(error) => PowerCapability::Unavailable(error.to_string()),
    }
}
#[derive(Clone)]
pub(super) struct Target {
    pub owner: String,
    pub path: &'static str,
    pub interface: &'static str,
}
impl Target {
    async fn properties(&self, connection: &Connection, deadline: Instant) -> Result<Properties> {
        bounded(
            async {
                connection
                    .call_method(
                        Some(self.owner.as_str()),
                        self.path,
                        Some("org.freedesktop.DBus.Properties"),
                        "GetAll",
                        &(self.interface,),
                    )
                    .await
                    .map_err(bus_error)?
                    .body()
                    .deserialize()
                    .map_err(|_| invalid())
            },
            deadline,
        )
        .await
    }
}
pub(super) struct Native {
    pub connection: Connection,
    pub battery: Option<Target>,
    pub profiles: Option<Target>,
    pub login: Option<Target>,
    pub session: Option<OwnedObjectPath>,
    pub profiles_by_name: BTreeMap<String, PowerObjectId>,
    pub backlights: super::backlights::Catalog,
}
async fn owner(connection: &Connection, name: &'static str) -> Result<Option<String>> {
    let proxy = zbus::fdo::DBusProxy::new(connection)
        .await
        .map_err(bus_error)?;
    match proxy
        .get_name_owner(name.try_into().map_err(|_| invalid())?)
        .await
    {
        Ok(owner) => Ok(Some(owner.to_string())),
        Err(zbus::fdo::Error::NameHasNoOwner(_)) => Ok(None),
        Err(_) => Err(unavailable("Power service ownership is unavailable")),
    }
}
impl Native {
    pub async fn new(connection: Connection) -> Result<Self> {
        let deadline = Instant::now() + DEADLINE;
        bounded(
            async {
                let battery = owner(&connection, UPOWER).await?.map(|owner| Target {
                    owner,
                    path: "/org/freedesktop/UPower",
                    interface: UPOWER,
                });
                let profiles = if let Some(owner) = owner(&connection, PROFILES).await? {
                    Some(Target {
                        owner,
                        path: "/org/freedesktop/UPower/PowerProfiles",
                        interface: PROFILES,
                    })
                } else {
                    owner(&connection, LEGACY_PROFILES)
                        .await?
                        .map(|owner| Target {
                            owner,
                            path: "/net/hadess/PowerProfiles",
                            interface: LEGACY_PROFILES,
                        })
                };
                let login = owner(&connection, LOGIN).await?.map(|owner| Target {
                    owner,
                    path: "/org/freedesktop/login1",
                    interface: MANAGER,
                });
                Ok(Self {
                    connection,
                    battery,
                    profiles,
                    login,
                    session: None,
                    profiles_by_name: BTreeMap::new(),
                    backlights: Default::default(),
                })
            },
            deadline,
        )
        .await
    }
    async fn battery(&self, deadline: Instant) -> Result<Option<Battery>> {
        let target = self
            .battery
            .as_ref()
            .ok_or_else(|| unavailable("Battery service is unavailable"))?;
        let root = target.properties(&self.connection, deadline).await?;
        let device = Target {
            owner: target.owner.clone(),
            path: "/org/freedesktop/UPower/devices/DisplayDevice",
            interface: DEVICE,
        };
        let props = device.properties(&self.connection, deadline).await?;
        if !field::<bool>(&props, "IsPresent")? {
            return Ok(None);
        }
        let state = match field::<u32>(&props, "State")? {
            0 => BatteryState::Unknown,
            1 => BatteryState::Charging,
            2 => BatteryState::Discharging,
            3 => BatteryState::Empty,
            4 => BatteryState::Full,
            5 => BatteryState::PendingCharge,
            6 => BatteryState::PendingDischarge,
            _ => return Err(invalid()),
        };
        let level = match optional::<u32>(&props, "BatteryLevel")?.unwrap_or(1) {
            0 => BatteryLevel::Unknown,
            1 => BatteryLevel::None,
            3 => BatteryLevel::Low,
            4 => BatteryLevel::Critical,
            6 => BatteryLevel::Normal,
            7 => BatteryLevel::High,
            8 => BatteryLevel::Full,
            _ => BatteryLevel::Unknown,
        };
        let seconds = |key| -> Result<Option<u64>> {
            Ok(optional::<i64>(&props, key)?
                .filter(|v| *v > 0)
                .map(|v| v as u64))
        };
        Ok(Some(Battery {
            charge: Percentage::new(field(&props, "Percentage")?).map_err(|_| invalid())?,
            state,
            level,
            on_battery: field(&root, "OnBattery")?,
            seconds_to_empty: seconds("TimeToEmpty")?,
            seconds_to_full: seconds("TimeToFull")?,
        }))
    }
    async fn profiles(
        &mut self,
        adapter: &PowerServices,
        deadline: Instant,
    ) -> Result<PowerProfiles> {
        let target = self
            .profiles
            .as_ref()
            .ok_or_else(|| unavailable("Power profiles are unavailable"))?;
        let props = target.properties(&self.connection, deadline).await?;
        let rows: Vec<Properties> = field(&props, "Profiles")?;
        if rows.len() > 16 {
            return Err(invalid());
        }
        let mut next = BTreeMap::new();
        for row in rows {
            let name: String = field(&row, "Profile")?;
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                || next.contains_key(&name)
            {
                return Err(invalid());
            }
            let id = match self.profiles_by_name.get(&name) {
                Some(id) => *id,
                None => PowerObjectId::new(adapter.identity()?)?,
            };
            next.insert(name, id);
        }
        let active: String = field(&props, "ActiveProfile")?;
        let result = PowerProfiles {
            active: next.get(&active).copied(),
            profiles: next
                .iter()
                .map(|(name, id)| PowerProfile {
                    id: *id,
                    name: name.clone(),
                })
                .collect(),
            performance_degraded: optional::<String>(&props, "PerformanceDegraded")?
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(128).collect()),
        };
        self.profiles_by_name = next;
        Ok(result)
    }
    async fn actions(&self, deadline: Instant) -> Result<Vec<PowerActionCapability>> {
        let target = self
            .login
            .as_ref()
            .ok_or_else(|| unavailable("Session power controls are unavailable"))?;
        let mut result = vec![];
        for (action, member) in [
            (PowerAction::Suspend, "CanSuspend"),
            (PowerAction::Hibernate, "CanHibernate"),
            (PowerAction::HybridSleep, "CanHybridSleep"),
            (PowerAction::Reboot, "CanReboot"),
            (PowerAction::PowerOff, "CanPowerOff"),
        ] {
            let value: String = bounded(
                async {
                    self.connection
                        .call_method(
                            Some(target.owner.as_str()),
                            target.path,
                            Some(target.interface),
                            member,
                            &(),
                        )
                        .await
                        .map_err(bus_error)?
                        .body()
                        .deserialize()
                        .map_err(|_| invalid())
                },
                deadline,
            )
            .await?;
            let permission = match value.as_str() {
                "yes" => PowerPermission::Allowed,
                "challenge" => PowerPermission::Authentication,
                "no" => PowerPermission::Denied,
                "na" => PowerPermission::Unsupported,
                _ => return Err(invalid()),
            };
            result.push(PowerActionCapability { action, permission });
        }
        Ok(result)
    }
    async fn current_session(
        &self,
        adapter: &PowerServices,
        deadline: Instant,
    ) -> Result<OwnedObjectPath> {
        let target = self
            .login
            .as_ref()
            .ok_or_else(|| unavailable("Session brightness controls are unavailable"))?;
        let query = async {
            let by_pid = self
                .connection
                .call_method(
                    Some(target.owner.as_str()),
                    target.path,
                    Some(target.interface),
                    "GetSessionByPID",
                    &(std::process::id(),),
                )
                .await;
            let path: OwnedObjectPath = match by_pid {
                Ok(reply) => reply.body().deserialize().map_err(|_| invalid())?,
                Err(_) => {
                    let hint = adapter
                        .session_hint
                        .as_ref()
                        .filter(|s| s.len() <= 64)
                        .ok_or_else(|| {
                            unavailable("No graphical session is available for brightness")
                        })?;
                    self.connection
                        .call_method(
                            Some(target.owner.as_str()),
                            target.path,
                            Some(target.interface),
                            "GetSession",
                            &(hint,),
                        )
                        .await
                        .map_err(bus_error)?
                        .body()
                        .deserialize()
                        .map_err(|_| invalid())?
                }
            };
            let props: Properties = self
                .connection
                .call_method(
                    Some(target.owner.as_str()),
                    path.as_str(),
                    Some("org.freedesktop.DBus.Properties"),
                    "GetAll",
                    &(SESSION,),
                )
                .await
                .map_err(bus_error)?
                .body()
                .deserialize()
                .map_err(|_| invalid())?;
            let user: (u32, OwnedObjectPath) = field(&props, "User")?;
            // The adapter can control only the current user's active Wayland session.
            if user.0 != unsafe { libc::getuid() }
                || !field::<bool>(&props, "Active")?
                || field::<String>(&props, "Type")? != "wayland"
            {
                return Err(unavailable(
                    "No owned active Wayland session is available for brightness",
                ));
            }
            Ok(path)
        };
        bounded(query, deadline).await
    }
    pub async fn read(&mut self, adapter: &PowerServices) -> PowerSnapshot {
        let deadline = Instant::now() + DEADLINE;
        let battery = capability(self.battery(deadline).await);
        let profiles = capability(self.profiles(adapter, deadline).await);
        let session = capability(self.actions(deadline).await);
        let brightness = match self.backlights.read(adapter) {
            Ok(devices) if devices.is_empty() => {
                self.session = None;
                PowerCapability::Ready(devices)
            }
            Ok(devices) => match self.current_session(adapter, deadline).await {
                Ok(path) => {
                    self.session = Some(path);
                    PowerCapability::Ready(devices)
                }
                Err(error) => {
                    self.session = None;
                    PowerCapability::Unavailable(error.to_string())
                }
            },
            Err(error) => {
                self.session = None;
                PowerCapability::Unavailable(error.to_string())
            }
        };
        PowerSnapshot {
            battery,
            profiles,
            brightness,
            session,
            ..Default::default()
        }
    }
    pub fn plan(&self, command: &PowerCommand) -> Result<Plan> {
        match command {
            PowerCommand::SetProfile(id) => {
                let target = self
                    .profiles
                    .as_ref()
                    .ok_or_else(|| unavailable("Power profiles are unavailable"))?
                    .clone();
                let name = self
                    .profiles_by_name
                    .iter()
                    .find(|(_, value)| **value == *id)
                    .map(|(name, _)| name.clone())
                    .ok_or_else(|| DomainError::Invalid("Power profile retired".into()))?;
                Ok(Plan::Profile(target, name))
            }
            PowerCommand::SetBrightness(id, percentage) => {
                let target = self
                    .login
                    .as_ref()
                    .ok_or_else(|| unavailable("Brightness controls are unavailable"))?
                    .clone();
                let path = self
                    .session
                    .clone()
                    .ok_or_else(|| unavailable("Brightness session is unavailable"))?;
                let device = self.backlights.resolve(*id)?;
                let level = (percentage.value() * f64::from(device.maximum) / 100.).round() as u32;
                Ok(Plan::Brightness(target, path, device.name, level))
            }
            PowerCommand::Session(action) => Ok(Plan::Session(
                self.login
                    .as_ref()
                    .ok_or_else(|| unavailable("Session controls are unavailable"))?
                    .clone(),
                *action,
            )),
        }
    }
}
pub(super) enum Plan {
    Profile(Target, String),
    Brightness(Target, OwnedObjectPath, String, u32),
    Session(Target, PowerAction),
}
impl Plan {
    pub async fn apply(self, connection: &Connection) -> Result<()> {
        let deadline = match &self {
            Self::Profile(..) | Self::Session(..) => INTERACTIVE_DEADLINE,
            Self::Brightness(..) => DEADLINE,
        };
        let deadline = Instant::now() + deadline;
        bounded(
            async {
                match self {
                    Self::Profile(target, name) => {
                        let proxy = Proxy::new(
                            connection,
                            target.owner.as_str(),
                            target.path,
                            "org.freedesktop.DBus.Properties",
                        )
                        .await
                        .map_err(bus_error)?;
                        let _: Option<()> = proxy
                            .call_with_flags(
                                "Set",
                                zbus::proxy::MethodFlags::AllowInteractiveAuth.into(),
                                &(
                                    target.interface,
                                    "ActiveProfile",
                                    zbus::zvariant::Value::from(name),
                                ),
                            )
                            .await
                            .map_err(bus_error)?;
                    }
                    Self::Brightness(target, path, name, level) => {
                        connection
                            .call_method(
                                Some(target.owner.as_str()),
                                path.as_str(),
                                Some(SESSION),
                                "SetBrightness",
                                &("backlight", name, level),
                            )
                            .await
                            .map_err(bus_error)?;
                    }
                    Self::Session(target, action) => {
                        let member = match action {
                            PowerAction::Suspend => "Suspend",
                            PowerAction::Hibernate => "Hibernate",
                            PowerAction::HybridSleep => "HybridSleep",
                            PowerAction::Reboot => "Reboot",
                            PowerAction::PowerOff => "PowerOff",
                        };
                        let proxy = Proxy::new(
                            connection,
                            target.owner.as_str(),
                            target.path,
                            target.interface,
                        )
                        .await
                        .map_err(bus_error)?;
                        let _: Option<()> = proxy
                            .call_with_flags(
                                member,
                                zbus::proxy::MethodFlags::AllowInteractiveAuth.into(),
                                &(true,),
                            )
                            .await
                            .map_err(bus_error)?;
                    }
                }
                Ok(())
            },
            deadline,
        )
        .await
    }
}
