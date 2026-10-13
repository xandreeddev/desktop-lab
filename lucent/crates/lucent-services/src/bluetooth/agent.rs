//! Agent1 callbacks are private transport operations. User decisions return to
//! the connection owner as bounded proposals, never through a rendering call.
use super::{native::*, *};
use std::sync::atomic::AtomicU64;
use zbus::{
    message::Header,
    object_server::ResponseDispatchNotifier,
    zvariant::{OwnedObjectPath, Signature, Type},
};

pub(super) const AGENT_PATH: &str = "/org/lucent/BluetoothAgent";
pub(super) const INTERACTIVE_TIMEOUT: Duration = Duration::from_secs(90);
pub(super) const PROPOSAL_LIMIT: usize = 8;

#[derive(zbus::DBusError, Debug)]
#[zbus(prefix = "org.bluez.Error")]
pub(super) enum AgentError {
    Rejected(String),
    Canceled(String),
}
type AgentResult<T> = std::result::Result<T, AgentError>;
fn rejected() -> AgentError {
    AgentError::Rejected("Bluetooth request rejected".into())
}
fn cancelled() -> AgentError {
    AgentError::Canceled("Bluetooth request cancelled".into())
}

/// Owns zeroizing PIN storage through serialization; no intermediate String.
pub(super) struct PinReply(BluetoothPin);
impl Type for PinReply {
    const SIGNATURE: &'static Signature = &Signature::Str;
}
impl serde::Serialize for PinReply {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.expose())
    }
}
pub(super) enum Answer {
    Accepted,
    Rejected,
    Pin(BluetoothPin),
    Passkey(BluetoothPasskeyInput),
    Cancelled,
}
pub(super) struct Proposal {
    pub path: OwnedObjectPath,
    pub prompt: BluetoothPairingPrompt,
    pub deadline: Instant,
    pub epoch: u64,
    pub reply: Option<async_channel::Sender<Answer>>,
}
pub(super) enum Event {
    Proposal(Proposal),
    Changed,
}
pub(super) struct Shared {
    peer: String,
    events: async_channel::Sender<Event>,
    epoch: AtomicU64,
    released: AtomicBool,
    release_reply_dispatched: AtomicBool,
}
impl Shared {
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    pub fn end_requests(&self, release: bool) {
        if release {
            self.released.store(true, Ordering::Release);
        }
        self.epoch.fetch_add(1, Ordering::AcqRel);
        // A full queue already wakes the owner; the epoch makes this lossless.
        let _ = self.events.try_send(Event::Changed);
    }
    fn authorize(&self, header: &Header<'_>) -> AgentResult<()> {
        if header
            .sender()
            .is_none_or(|sender| sender.as_str() != self.peer)
            || self.released.load(Ordering::Acquire)
        {
            return Err(rejected());
        }
        Ok(())
    }
    async fn ask(
        &self,
        header: &Header<'_>,
        path: OwnedObjectPath,
        prompt: BluetoothPairingPrompt,
    ) -> AgentResult<Answer> {
        self.authorize(header)?;
        let (reply, receive) = async_channel::bounded(1);
        let epoch = self.epoch();
        let deadline = Instant::now() + INTERACTIVE_TIMEOUT;
        self.events
            .try_send(Event::Proposal(Proposal {
                path,
                prompt,
                deadline,
                epoch,
                reply: Some(reply),
            }))
            .map_err(|_| rejected())?;
        let answer = futures_lite::future::or(
            async { receive.recv().await.map_err(|_| cancelled()) },
            async {
                loop {
                    if self.epoch() != epoch || Instant::now() >= deadline {
                        return Err(cancelled());
                    }
                    async_io::Timer::after(Duration::from_millis(100)).await;
                }
            },
        )
        .await?;
        if self.epoch() != epoch || matches!(answer, Answer::Cancelled) {
            return Err(cancelled());
        }
        Ok(answer)
    }
    fn display(
        &self,
        header: &Header<'_>,
        path: OwnedObjectPath,
        prompt: BluetoothPairingPrompt,
    ) -> AgentResult<()> {
        self.authorize(header)?;
        self.events
            .try_send(Event::Proposal(Proposal {
                path,
                prompt,
                deadline: Instant::now() + INTERACTIVE_TIMEOUT,
                epoch: self.epoch(),
                reply: None,
            }))
            .map_err(|_| rejected())
    }
}
#[derive(Clone)]
pub(super) struct Agent(Arc<Shared>);
#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    async fn release(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> std::result::Result<ResponseDispatchNotifier<()>, AgentError> {
        self.0.authorize(&header)?;
        self.0
            .release_reply_dispatched
            .store(false, Ordering::Release);
        self.0.end_requests(true);
        let (response, dispatched) = ResponseDispatchNotifier::new(());
        let shared = self.0.clone();
        // Reject callbacks and retire requests immediately, but let the native
        // object server dispatch this reply before the owner closes its bus.
        connection
            .executor()
            .spawn(
                async move {
                    dispatched.await;
                    shared
                        .release_reply_dispatched
                        .store(true, Ordering::Release);
                    let _ = shared.events.try_send(Event::Changed);
                },
                "bluetooth-agent-release-reply",
            )
            .detach();
        Ok(response)
    }
    async fn cancel(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<(), AgentError> {
        self.0.authorize(&header)?;
        self.0.end_requests(false);
        Ok(())
    }
    async fn request_pin_code(
        &self,
        device: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<PinReply, AgentError> {
        match self
            .0
            .ask(&header, device, BluetoothPairingPrompt::PinCode)
            .await?
        {
            Answer::Pin(pin) => Ok(PinReply(pin)),
            _ => Err(rejected()),
        }
    }
    async fn request_passkey(
        &self,
        device: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<u32, AgentError> {
        match self
            .0
            .ask(&header, device, BluetoothPairingPrompt::Passkey)
            .await?
        {
            Answer::Passkey(value) => Ok(value.number()),
            _ => Err(rejected()),
        }
    }
    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<(), AgentError> {
        let code = BluetoothPairingCode::new(passkey).map_err(|_| rejected())?;
        match self
            .0
            .ask(
                &header,
                device,
                BluetoothPairingPrompt::ConfirmPasskey(code),
            )
            .await?
        {
            Answer::Accepted => Ok(()),
            _ => Err(rejected()),
        }
    }
    async fn request_authorization(
        &self,
        device: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<(), AgentError> {
        match self
            .0
            .ask(&header, device, BluetoothPairingPrompt::AuthorizePairing)
            .await?
        {
            Answer::Accepted => Ok(()),
            _ => Err(rejected()),
        }
    }
    async fn authorize_service(
        &self,
        device: OwnedObjectPath,
        uuid: String,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<(), AgentError> {
        let service = BluetoothServiceId::new(uuid).map_err(|_| rejected())?;
        match self
            .0
            .ask(
                &header,
                device,
                BluetoothPairingPrompt::AuthorizeService(service),
            )
            .await?
        {
            Answer::Accepted => Ok(()),
            _ => Err(rejected()),
        }
    }
    async fn display_pin_code(
        &self,
        device: OwnedObjectPath,
        pincode: String,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<(), AgentError> {
        if pincode.len() != 6 || !pincode.bytes().all(|c| c.is_ascii_digit()) {
            return Err(rejected());
        }
        let code = BluetoothPairingCode::new(pincode.parse().map_err(|_| rejected())?)
            .map_err(|_| rejected())?;
        self.0.display(
            &header,
            device,
            BluetoothPairingPrompt::DisplayPinCode(code),
        )
    }
    async fn display_passkey(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
        entered: u16,
        #[zbus(header)] header: Header<'_>,
    ) -> std::result::Result<(), AgentError> {
        if entered > 6 {
            return Err(rejected());
        }
        let code = BluetoothPairingCode::new(passkey).map_err(|_| rejected())?;
        self.0.display(
            &header,
            device,
            BluetoothPairingPrompt::DisplayPasskey { code, entered },
        )
    }
}
pub(super) struct Host {
    pub shared: Arc<Shared>,
    pub events: async_channel::Receiver<Event>,
    pub registered: bool,
    pub incoming: bool,
}
impl Host {
    pub fn new(peer: String) -> Self {
        let (send, events) = async_channel::bounded(PROPOSAL_LIMIT);
        Self {
            shared: Arc::new(Shared {
                peer,
                events: send,
                epoch: AtomicU64::new(0),
                released: AtomicBool::new(false),
                release_reply_dispatched: AtomicBool::new(true),
            }),
            events,
            registered: false,
            incoming: false,
        }
    }
    pub async fn register(&mut self, transport: &Transport) -> Result<()> {
        if self.released() && !self.release_reply_dispatched() {
            return Err(DomainError::Invalid(
                "Bluetooth agent release is still finishing".into(),
            ));
        }
        if self.registered && !self.shared.released.load(Ordering::Acquire) {
            return Ok(());
        }
        self.shared.released.store(false, Ordering::Release);
        transport
            .connection
            .object_server()
            .at(AGENT_PATH, Agent(self.shared.clone()))
            .await
            .map_err(|_| unavailable("Bluetooth agent registration failed"))?;
        transport
            .call(
                "/org/bluez",
                "org.bluez.AgentManager1",
                "RegisterAgent",
                &(
                    zbus::zvariant::ObjectPath::try_from(AGENT_PATH).unwrap(),
                    "KeyboardDisplay",
                ),
            )
            .await?;
        self.registered = true;
        Ok(())
    }
    pub fn released(&self) -> bool {
        self.shared.released.load(Ordering::Acquire)
    }
    pub fn release_reply_dispatched(&self) -> bool {
        self.shared.release_reply_dispatched.load(Ordering::Acquire)
    }
    pub async fn set_incoming(&mut self, transport: &Transport, enabled: bool) -> Result<()> {
        if enabled {
            self.register(transport).await?;
            transport
                .call(
                    "/org/bluez",
                    "org.bluez.AgentManager1",
                    "RequestDefaultAgent",
                    &(zbus::zvariant::ObjectPath::try_from(AGENT_PATH).unwrap(),),
                )
                .await?;
            self.incoming = true;
        } else if self.registered {
            // There is no dedicated relinquish-default API; unregister our
            // agent, cancelling its requests, and register lazily on next use.
            transport
                .call(
                    "/org/bluez",
                    "org.bluez.AgentManager1",
                    "UnregisterAgent",
                    &(zbus::zvariant::ObjectPath::try_from(AGENT_PATH).unwrap(),),
                )
                .await?;
            self.shared.end_requests(false);
            self.registered = false;
            self.incoming = false;
        }
        Ok(())
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.shared.end_requests(true);
    }
}
