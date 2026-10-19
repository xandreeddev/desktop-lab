//! NetworkManager SecretAgent transport. Only the captured unique daemon may
//! ask; user input uses a separate bounded lane from five-second native methods.
use super::{native::*, snapshot::Kind, *};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Condvar,
};
use zbus::{
    message::Header,
    zvariant::{OwnedObjectPath, Signature, Type, Value},
};

pub(super) const PATH: &str = "/org/freedesktop/NetworkManager/SecretAgent";
const MANAGER: &str = "/org/freedesktop/NetworkManager/AgentManager";
const SECURITY: &str = "802-11-wireless-security";
pub(super) const PROMPT_TIMEOUT: Duration = Duration::from_secs(90);
const LIMIT: usize = 8;

#[derive(zbus::DBusError, Debug)]
#[zbus(prefix = "org.freedesktop.NetworkManager.SecretAgent")]
enum AgentError {
    UserCanceled(String),
    NoSecrets(String),
    PermissionDenied(String),
}
fn no_secrets() -> AgentError {
    AgentError::NoSecrets("No supported network secret is available".into())
}
fn cancelled() -> AgentError {
    AgentError::UserCanceled("Network credential request cancelled".into())
}
type AgentResult<T> = std::result::Result<T, AgentError>;

#[derive(Clone, PartialEq, Eq)]
struct ProfileContext {
    id: NetworkObjectId,
    key: NetworkProfileKey,
    ssid: NetworkSsid,
    name: String,
}
struct Pending {
    path: String,
    profile_id: NetworkObjectId,
    request: NetworkCredentialRequest,
    reply: async_channel::Sender<Option<WifiCredentials>>,
}
struct Waiter {
    path: String,
    minimum: u64,
    extension: Arc<Mutex<Option<Instant>>>,
    grace: Duration,
}
struct State {
    online: bool,
    revision: u64,
    profiles: BTreeMap<String, ProfileContext>,
    pending: BTreeMap<NetworkCredentialRequestId, Pending>,
    waiting: Option<Waiter>,
    wifi_paths: BTreeSet<String>,
}
/// No transport or secret value is stored in an observation. A pending reply
/// owns only its one-shot sender; credentials move straight to the callback.
pub(super) struct Hub {
    state: Mutex<State>,
    changed: Condvar,
    next: AtomicU64,
    watching: AtomicBool,
}
impl Default for Hub {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                online: false,
                revision: 0,
                profiles: BTreeMap::new(),
                pending: BTreeMap::new(),
                waiting: None,
                wifi_paths: BTreeSet::new(),
            }),
            changed: Condvar::new(),
            next: AtomicU64::new(1),
            watching: AtomicBool::new(false),
        }
    }
}
impl Hub {
    #[cfg(test)]
    pub(super) fn state_for_test(&self) -> Option<NetworkCredentialRequest> {
        self.state
            .lock()
            .unwrap()
            .pending
            .values()
            .next()
            .map(|p| p.request.clone())
    }
    fn change(&self, state: &mut State) {
        state.revision = state.revision.wrapping_add(1);
        self.changed.notify_all();
    }
    fn online(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.online = true;
        self.change(&mut state);
    }
    fn retire(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.online = false;
        state.pending.clear(); // drops senders and wakes every awaiting callback
        state.profiles.clear();
        state.wifi_paths.clear();
        state.waiting = None;
        self.change(&mut state);
    }
    pub(super) fn refresh(&self, objects: &Objects, snapshot: &NetworkSnapshot) {
        let profiles: BTreeMap<_, _> = if snapshot.wifi_enabled
            && snapshot.wifi_hardware_enabled
            && snapshot
                .devices
                .iter()
                .any(|d| d.kind == NetworkKind::Wifi && d.managed)
        {
            snapshot
                .profiles
                .iter()
                .filter_map(|p| {
                    if p.kind != NetworkKind::Wifi {
                        return None;
                    }
                    Some((
                        objects.resolve(p.id, Kind::Profile).ok()?.to_string(),
                        ProfileContext {
                            id: p.id,
                            key: p.key.clone(),
                            ssid: p.ssid.clone()?,
                            name: p.name.clone(),
                        },
                    ))
                })
                .collect()
        } else {
            BTreeMap::new()
        };
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let before = state.pending.len();
        state.pending.retain(|_, pending| {
            profiles.get(&pending.path).is_some_and(|profile| {
                profile.id == pending.profile_id
                    && profile.key == pending.request.profile
                    && profile.ssid == pending.request.ssid
            })
        });
        state.profiles = profiles;
        state.wifi_paths = snapshot
            .devices
            .iter()
            .filter(|d| d.kind == NetworkKind::Wifi && d.managed)
            .filter_map(|d| {
                objects
                    .resolve(d.id, Kind::Device)
                    .ok()
                    .map(|p| p.to_string())
            })
            .collect();
        if state.pending.len() != before {
            self.change(&mut state);
        }
    }
    pub(super) fn watch(
        &self,
        emit: &mut dyn FnMut(Result<Vec<NetworkCredentialRequest>>),
        stop: &dyn StopSignal,
    ) {
        if self
            .watching
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            emit(Err(unavailable(
                "Network credential prompts already have an observer",
            )));
            return;
        }
        struct Watching<'a>(&'a Hub);
        impl Drop for Watching<'_> {
            fn drop(&mut self) {
                self.0.watching.store(false, Ordering::Release);
                let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
                state.pending.clear();
                self.0.change(&mut state);
            }
        }
        let _watching = Watching(self);
        let mut revision = None;
        while !stop.cancelled() {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if revision != Some(state.revision) {
                revision = Some(state.revision);
                let value = if state.online {
                    Ok(state.pending.values().map(|p| p.request.clone()).collect())
                } else {
                    Err(unavailable("Network credential service is unavailable"))
                };
                drop(state);
                emit(value);
                continue;
            }
            state = self
                .changed
                .wait_timeout(state, Duration::from_millis(100))
                .unwrap_or_else(|e| e.into_inner())
                .0;
            drop(state);
        }
    }
    pub(super) fn answer(
        &self,
        id: NetworkCredentialRequestId,
        credentials: Option<WifiCredentials>,
    ) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| unavailable("Network credential state is unavailable"))?;
        let pending = state.pending.get(&id).ok_or_else(|| {
            DomainError::Invalid(
                "Network credential request expired; do not replay the answer".into(),
            )
        })?;
        if credentials
            .as_ref()
            .is_some_and(|c| c.security() != pending.request.security)
        {
            return Err(DomainError::Invalid(
                "Credentials do not match this network request".into(),
            ));
        }
        let pending = state.pending.remove(&id).unwrap();
        self.change(&mut state);
        pending.reply.try_send(credentials).map_err(|_| {
            DomainError::Invalid("Network credential request ended before answer delivery".into())
        })
    }
    pub(super) fn invalidate(&self, path: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let before = state.pending.len();
        if state.wifi_paths.remove(path) {
            // A profile may select any managed radio. Until a fresh native
            // snapshot resolves the surviving target, retire interactive intent.
            state.pending.clear();
        } else {
            state.pending.retain(|_, p| p.path != path);
        }
        state.profiles.remove(path);
        if state.pending.len() != before {
            self.change(&mut state);
        }
    }
    fn remove(&self, id: NetworkCredentialRequestId) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.pending.remove(&id).is_some() {
            self.change(&mut state);
        }
    }
    fn cancel_path(&self, path: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let before = state.pending.len();
        state.pending.retain(|_, pending| pending.path != path);
        if state.pending.len() != before {
            self.change(&mut state);
        }
    }
}
#[derive(Clone)]
struct Agent {
    hub: Arc<Hub>,
    peer: String,
    alive: Arc<AtomicBool>,
}
impl Agent {
    fn authorize(&self, header: &Header<'_>) -> AgentResult<()> {
        if !self.alive.load(Ordering::Acquire)
            || header.sender().is_none_or(|s| s.as_str() != self.peer)
        {
            return Err(AgentError::PermissionDenied(
                "Network credential sender is not authorized".into(),
            ));
        }
        Ok(())
    }
    fn request(
        &self,
        settings: &Settings,
        path: &str,
        setting: &str,
        hints: &[String],
        flags: u32,
    ) -> AgentResult<NetworkCredentialRequest> {
        // No interaction for background/cache-only requests; no enterprise/VPN,
        // WPS or agent-owned vault claims. The UI supplies WPA/SAE passwords.
        if setting != SECURITY
            || flags & 1 == 0
            || flags & !7 != 0
            || hints.iter().any(|h| h != "psk")
        {
            return Err(no_secrets());
        }
        let connection = settings.get("connection").ok_or_else(no_secrets)?;
        let text = |values: &Properties, key: &str| {
            values
                .get(key)
                .and_then(|v| <&str>::try_from(v).ok())
                .map(str::to_owned)
        };
        if text(connection, "type").as_deref() != Some("802-11-wireless") {
            return Err(no_secrets());
        }
        let key = text(connection, "uuid")
            .and_then(|s| NetworkProfileKey::new(s).ok())
            .ok_or_else(no_secrets)?;
        let security = settings.get(SECURITY).ok_or_else(no_secrets)?;
        let kind = match text(security, "key-mgmt").as_deref() {
            Some("wpa-psk") => WifiSecurity::WpaPersonal,
            Some("sae") => WifiSecurity::Wpa3Personal,
            _ => return Err(no_secrets()),
        };
        let secret_flags = security
            .get("psk-flags")
            .map(u32::try_from)
            .transpose()
            .map_err(|_| no_secrets())?
            .unwrap_or(0);
        if secret_flags & !6 != 0 {
            return Err(no_secrets());
        } // AGENT_OWNED (1) requires a real vault.
        let ssid = settings
            .get("802-11-wireless")
            .and_then(|v| v.get("ssid"))
            .and_then(|v| <Vec<u8>>::try_from(v.try_clone().ok()?).ok())
            .and_then(|v| NetworkSsid::new(v).ok())
            .ok_or_else(no_secrets)?;
        let state = self.hub.state.lock().map_err(|_| no_secrets())?;
        let context = state
            .profiles
            .get(path)
            .filter(|p| p.key == key && p.ssid == ssid)
            .ok_or_else(no_secrets)?;
        if !state.online
            || !self.hub.watching.load(Ordering::Acquire)
            || state.pending.len() >= LIMIT
        {
            return Err(no_secrets());
        }
        let id = self
            .hub
            .next
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| no_secrets())?;
        Ok(NetworkCredentialRequest {
            id: NetworkCredentialRequestId::new(id).map_err(|_| no_secrets())?,
            profile: key,
            name: context.name.clone(),
            ssid,
            security: kind,
            new_secret_required: flags & 2 != 0,
        })
    }
}
/// Serialize the zeroizing secret by reference; no extra plaintext String or
/// OwnedValue is retained. zbus/native message storage has its own lifetime.
struct Reply(WifiCredentials);
impl Type for Reply {
    const SIGNATURE: &'static Signature = <Settings as Type>::SIGNATURE;
}
impl serde::Serialize for Reply {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let values = std::collections::HashMap::from([
            ("name", Value::from(SECURITY)),
            (
                "psk",
                Value::from(self.0.password().map(Secret::expose).unwrap_or("")),
            ),
        ]);
        std::collections::HashMap::from([(SECURITY, values)]).serialize(serializer)
    }
}
#[zbus::interface(name = "org.freedesktop.NetworkManager.SecretAgent")]
impl Agent {
    async fn get_secrets(
        &self,
        settings: Settings,
        connection_path: OwnedObjectPath,
        setting_name: String,
        hints: Vec<String>,
        flags: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<Reply, AgentError> {
        self.authorize(&header)?;
        let request = self.request(
            &settings,
            connection_path.as_str(),
            &setting_name,
            &hints,
            flags,
        )?;
        drop(settings); // Native incoming transport copies are not our secret vault.
        let (send, receive) = async_channel::bounded(1);
        let id = request.id;
        let deadline = Instant::now() + PROMPT_TIMEOUT;
        {
            let mut state = self.hub.state.lock().map_err(|_| no_secrets())?;
            if !state.online
                || !self.hub.watching.load(Ordering::Acquire)
                || state.pending.len() >= LIMIT
                || !state
                    .profiles
                    .get(connection_path.as_str())
                    .is_some_and(|p| p.key == request.profile && p.ssid == request.ssid)
            {
                return Err(no_secrets());
            }
            // NetworkManager promises one setting exchange, but retire an older
            // same-profile callback rather than keeping two competing prompts.
            state
                .pending
                .retain(|_, p| p.path != connection_path.as_str());
            let profile_id = state.profiles[connection_path.as_str()].id;
            if let Some(waiting) = &state.waiting
                && waiting.path == connection_path.as_str()
                && id.value() >= waiting.minimum
            {
                let mut extension = waiting.extension.lock().unwrap_or_else(|e| e.into_inner());
                if extension.is_none() {
                    *extension = Some(deadline + waiting.grace);
                }
            }
            state.pending.insert(
                id,
                Pending {
                    path: connection_path.to_string(),
                    profile_id,
                    request,
                    reply: send,
                },
            );
            self.hub.change(&mut state);
        }
        let answer =
            futures_lite::future::or(async { receive.recv().await.ok().flatten() }, async {
                async_io::Timer::at(deadline).await;
                None
            })
            .await;
        self.hub.remove(id);
        self.authorize(&header)?;
        answer.map(Reply).ok_or_else(cancelled)
    }
    fn cancel_get_secrets(
        &self,
        connection_path: OwnedObjectPath,
        setting_name: String,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<(), AgentError> {
        self.authorize(&header)?;
        if setting_name == SECURITY {
            self.hub.cancel_path(connection_path.as_str());
        }
        Ok(())
    }
    fn save_secrets(
        &self,
        _settings: Settings,
        _connection_path: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<(), AgentError> {
        self.authorize(&header)?;
        // We do not advertise or silently pretend to provide a persistent vault.
        Err(no_secrets())
    }
    fn delete_secrets(
        &self,
        _settings: Settings,
        connection_path: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<(), AgentError> {
        self.authorize(&header)?;
        self.hub.cancel_path(connection_path.as_str());
        Ok(())
    }
}
/// One admitted saved activation can wait for its first actual, matching native
/// request. The deadline is absolute and cannot be renewed by repeated prompts.
pub(super) struct Interaction {
    hub: Arc<Hub>,
    path: String,
    minimum: u64,
    extension: Arc<Mutex<Option<Instant>>>,
}
impl Interaction {
    pub(super) fn new(
        hub: Arc<Hub>,
        path: String,
        extension: Arc<Mutex<Option<Instant>>>,
        grace: Duration,
    ) -> Self {
        let minimum = hub.next.load(Ordering::Acquire);
        hub.state.lock().unwrap_or_else(|e| e.into_inner()).waiting = Some(Waiter {
            path: path.clone(),
            minimum,
            extension: extension.clone(),
            grace,
        });
        Self {
            hub,
            path,
            minimum,
            extension,
        }
    }
    pub(super) fn deadline(&self) -> Option<Instant> {
        *self.extension.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub(super) fn cancel(&self) {
        let mut state = self.hub.state.lock().unwrap_or_else(|e| e.into_inner());
        let before = state.pending.len();
        state
            .pending
            .retain(|_, p| p.path != self.path || p.request.id.value() < self.minimum);
        if state.pending.len() != before {
            self.hub.change(&mut state);
        }
    }
}
impl Drop for Interaction {
    fn drop(&mut self) {
        let mut state = self.hub.state.lock().unwrap_or_else(|e| e.into_inner());
        if state
            .waiting
            .as_ref()
            .is_some_and(|w| Arc::ptr_eq(&w.extension, &self.extension))
        {
            state.waiting = None;
        }
    }
}
pub(super) struct Session {
    agent: Agent,
}
impl Session {
    pub(super) async fn register(
        transport: &Transport,
        hub: Arc<Hub>,
        timeout: Duration,
    ) -> Result<Self> {
        let agent = Agent {
            hub,
            peer: transport.owner.clone(),
            alive: Arc::new(AtomicBool::new(true)),
        };
        let session = Self { agent };
        bounded(
            async {
                transport
                    .connection
                    .object_server()
                    .at(PATH, session.agent.clone())
                    .await
                    .map_err(bus_error)?;
                transport
                    .call(
                        MANAGER,
                        "org.freedesktop.NetworkManager.AgentManager",
                        "RegisterWithCapabilities",
                        &("org.lucent.Network", 0u32),
                    )
                    .await?
                    .body()
                    .deserialize::<()>()
                    .map_err(|_| protocol())?;
                Ok(())
            },
            Instant::now() + timeout,
        )
        .await?;
        session.agent.hub.online();
        Ok(session)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.agent.alive.store(false, Ordering::Release);
        self.agent.hub.retire();
        // Registration is connection-owned; dropping this complete native
        // lifetime unregisters it. No method replay against a replacement owner.
    }
}
