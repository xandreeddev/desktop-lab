//! xdg-activation tokens belong to one actual input dispatch and one request.
//! No retained serial, inherited environment token, synchronous roundtrip or
//! application-launch policy belongs here.
use super::*;
use wayland_client::{Dispatch, globals::GlobalList};
use wayland_protocols::xdg::activation::v1::client::{
    xdg_activation_token_v1::{self, XdgActivationTokenV1},
    xdg_activation_v1::{self, XdgActivationV1},
};

pub(super) struct Input {
    pub surface: wl_surface::WlSurface,
    pub seat: wl_seat::WlSeat,
    pub serial: u32,
}
struct Pending<M> {
    proxy: XdgActivationTokenV1,
    request: api::ActivationRequest<M>,
    timer: RegistrationToken,
}
fn valid_target_id(app_id: Option<&str>) -> bool {
    app_id.is_none_or(|value| {
        !value.is_empty() && value.len() <= 255 && !value.chars().any(char::is_control)
    })
}

fn configure_token(proxy: &XdgActivationTokenV1, input: &Input, app_id: Option<&str>) {
    proxy.set_serial(input.serial, &input.seat);
    proxy.set_surface(&input.surface);
    // xdg-activation-v1 makes the target app_id optional. Default-handler
    // resolution belongs to the client adapter, not the Wayland runtime.
    if let Some(app_id) = app_id {
        proxy.set_app_id(app_id.to_owned());
    }
}
pub(super) struct Activation<M> {
    manager: Option<XdgActivationV1>,
    pub input: Option<Input>,
    pending: BTreeMap<u64, Pending<M>>,
    next: u64,
}
impl<M> Activation<M> {
    pub fn new<A: Application>(
        globals: &GlobalList,
        qh: &QueueHandle<State<A>>,
        enabled: bool,
    ) -> Self {
        Self {
            manager: enabled.then(|| globals.bind(qh, 1..=1, ()).ok()).flatten(),
            input: None,
            pending: BTreeMap::new(),
            next: 0,
        }
    }
}
impl<M> Drop for Activation<M> {
    fn drop(&mut self) {
        for pending in self.pending.values() {
            pending.proxy.destroy();
        }
        if let Some(manager) = self.manager.take() {
            manager.destroy();
        }
    }
}
impl<A: Application> State<A> {
    pub(super) fn activation_request(
        &mut self,
        request: api::ActivationRequest<A::Message>,
        qh: &QueueHandle<Self>,
    ) {
        use api::ActivationOutcome as Outcome;
        let failure = if !valid_target_id(request.app_id.as_deref()) {
            Some(Outcome::Invalid)
        } else if self.authorize_unlock.is_some() || self.activation.manager.is_none() {
            Some(Outcome::Unavailable)
        } else if self.activation.pending.len() >= 8 {
            Some(Outcome::Busy)
        } else if self.activation.input.is_none() {
            Some(Outcome::NoUserInput)
        } else {
            None
        };
        if let Some(outcome) = failure {
            self.message(request.finish(outcome), qh);
            return;
        }
        let input = self
            .activation
            .input
            .take()
            .expect("validated current input");
        if self.id_for(&input.surface).is_none() {
            self.message(request.finish(Outcome::NoUserInput), qh);
            return;
        }
        self.activation.next = self
            .activation
            .next
            .checked_add(1)
            .expect("activation request counter exhausted");
        let id = self.activation.next;
        let proxy = self
            .activation
            .manager
            .as_ref()
            .unwrap()
            .get_activation_token(qh, id);
        configure_token(&proxy, &input, request.app_id.as_deref());
        let timeout_qh = qh.clone();
        let timer = match self.handle.insert_source(
            Timer::from_duration(Duration::from_secs(2)),
            move |_, _, state| {
                // The timer source removes itself; completion must not remove it twice.
                state.activation_finish(id, Outcome::Timeout, &timeout_qh, true);
                TimeoutAction::Drop
            },
        ) {
            Ok(timer) => timer,
            Err(_) => {
                proxy.destroy();
                self.message(request.finish(Outcome::Unavailable), qh);
                return;
            }
        };
        self.activation.pending.insert(
            id,
            Pending {
                proxy: proxy.clone(),
                request,
                timer,
            },
        );
        proxy.commit();
    }
    fn activation_finish(
        &mut self,
        id: u64,
        outcome: api::ActivationOutcome,
        qh: &QueueHandle<Self>,
        timer_fired: bool,
    ) {
        let Some(pending) = self.activation.pending.remove(&id) else {
            return;
        };
        if !timer_fired {
            self.handle.remove(pending.timer);
        }
        pending.proxy.destroy();
        self.message(pending.request.finish(outcome), qh);
    }
}
impl<A: Application> Dispatch<XdgActivationV1, ()> for State<A> {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: xdg_activation_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl<A: Application> Dispatch<XdgActivationTokenV1, u64> for State<A> {
    fn event(
        state: &mut Self,
        proxy: &XdgActivationTokenV1,
        event: xdg_activation_token_v1::Event,
        id: &u64,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if !state
            .activation
            .pending
            .get(id)
            .is_some_and(|pending| pending.proxy == *proxy)
        {
            return;
        }
        if let xdg_activation_token_v1::Event::Done { token } = event {
            let outcome = api::ActivationToken::new(token)
                .map(api::ActivationOutcome::Token)
                .unwrap_or(api::ActivationOutcome::Invalid);
            state.activation_finish(*id, outcome, qh, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    struct Peer;
    wayland_client::delegate_noop!(Peer: ignore wayland_client::protocol::wl_registry::WlRegistry);
    wayland_client::delegate_noop!(Peer: ignore wl_seat::WlSeat);
    wayland_client::delegate_noop!(Peer: ignore wayland_client::protocol::wl_compositor::WlCompositor);
    wayland_client::delegate_noop!(Peer: ignore wl_surface::WlSurface);
    wayland_client::delegate_noop!(Peer: ignore XdgActivationV1);
    wayland_client::delegate_noop!(Peer: ignore XdgActivationTokenV1);

    #[test]
    fn optional_target_keeps_existing_known_id_validation() {
        assert!(valid_target_id(None));
        assert!(valid_target_id(Some("org.example.App")));
        assert!(valid_target_id(Some(&"x".repeat(255))));
        for value in [String::new(), "a\nb".into(), "x".repeat(256)] {
            assert!(!valid_target_id(Some(&value)));
        }
    }

    #[test]
    fn unknown_handler_omits_app_id_but_sends_the_actual_input_authority() {
        // Inspect actual client wire requests on an owned socket pair. There is
        // no compositor/GPU connection, live input serial or external desktop.
        for target in [None, Some("org.example.App")] {
            let (client, mut server) = std::os::unix::net::UnixStream::pair().unwrap();
            server
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let connection = Connection::from_socket(client).unwrap();
            let queue = connection.new_event_queue::<Peer>();
            let qh = queue.handle();
            let registry = connection.display().get_registry(&qh, ());
            let seat = registry.bind::<wl_seat::WlSeat, _, _>(1, 1, &qh, ());
            let compositor = registry
                .bind::<wayland_client::protocol::wl_compositor::WlCompositor, _, _>(2, 1, &qh, ());
            let surface = compositor.create_surface(&qh, ());
            let manager = registry.bind::<XdgActivationV1, _, _>(3, 1, &qh, ());
            let token = manager.get_activation_token(&qh, ());
            let input = Input {
                surface: surface.clone(),
                seat: seat.clone(),
                serial: 73,
            };
            configure_token(&token, &input, target);
            token.commit();
            connection.flush().unwrap();

            let mut requests = vec![];
            for _ in 0..32 {
                let mut header = [0u8; 8];
                server.read_exact(&mut header).unwrap();
                let object = u32::from_ne_bytes(header[..4].try_into().unwrap());
                let word = u32::from_ne_bytes(header[4..].try_into().unwrap());
                let size = (word >> 16) as usize;
                let opcode = word as u16;
                assert!((8..=4096).contains(&size));
                let mut payload = vec![0; size - 8];
                server.read_exact(&mut payload).unwrap();
                if object == token.id().protocol_id() {
                    requests.push((opcode, payload));
                    if opcode == 3 {
                        break;
                    }
                }
            }
            assert_eq!(
                requests
                    .iter()
                    .map(|(opcode, _)| *opcode)
                    .collect::<Vec<_>>(),
                if target.is_some() {
                    vec![0, 2, 1, 3]
                } else {
                    vec![0, 2, 3]
                }
            );
            assert_eq!(&requests[0].1[..4], &73u32.to_ne_bytes());
            assert_eq!(&requests[0].1[4..], &seat.id().protocol_id().to_ne_bytes());
            assert_eq!(requests[1].1, surface.id().protocol_id().to_ne_bytes());
            if let Some(target) = target {
                let payload = &requests[2].1;
                let len = u32::from_ne_bytes(payload[..4].try_into().unwrap()) as usize;
                assert_eq!(&payload[4..4 + len - 1], target.as_bytes());
                assert_eq!(payload[4 + len - 1], 0);
            }
        }
    }
}
