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
        let invalid = request.app_id.is_empty()
            || request.app_id.len() > 255
            || request.app_id.chars().any(char::is_control);
        let failure = if invalid {
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
        proxy.set_serial(input.serial, &input.seat);
        proxy.set_surface(&input.surface);
        proxy.set_app_id(request.app_id.clone());
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
        event: xdg_activation_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {}
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
