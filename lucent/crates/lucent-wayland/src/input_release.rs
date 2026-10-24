//! A bounded acknowledgement of a caller-requested layer input release.
//! Tagged frame callbacks share the normal buffer commit; no extra commit,
//! geometry mutation, synchronous roundtrip or destination focus belongs here.
use super::*;
use wayland_client::{Dispatch, protocol::wl_callback::WlCallback};

const CAPACITY: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(2);
struct Pending<M> {
    request: api::InputReleaseRequest<M>,
    surface: wl_surface::WlSurface,
    frame: Option<u64>,
    timer: Option<RegistrationToken>,
    deadline: Instant,
}
struct Frame {
    surface: wl_surface::WlSurface,
    callback: WlCallback,
    requests: Vec<u64>,
    presented: bool,
}
#[derive(Clone)]
pub(super) struct FrameData {
    id: u64,
    surface: wl_surface::WlSurface,
}
pub(super) struct Releases<M> {
    requests: BTreeMap<u64, Pending<M>>,
    // Timed-out/cancelled callbacks remain counted until their server Done.
    // wl_callback has no client destructor request; never admit unbounded ghosts.
    frames: BTreeMap<u64, Frame>,
    next_request: u64,
    next_frame: u64,
}
impl<M> Default for Releases<M> {
    fn default() -> Self {
        Self {
            requests: BTreeMap::new(),
            frames: BTreeMap::new(),
            next_request: 0,
            next_frame: 0,
        }
    }
}
fn released(spec: &api::SurfaceSpec) -> bool {
    spec.keyboard == api::Keyboard::None && spec.pointer_input == api::PointerInput::None
}
fn admission(secure: bool, native: Option<(bool, bool)>) -> Option<api::InputReleaseOutcome> {
    use api::InputReleaseOutcome as Outcome;
    match (secure, native) {
        (true, _) | (false, Some((false, _))) => Some(Outcome::Unavailable),
        (false, None) => Some(Outcome::Retired),
        (false, Some((true, false))) => Some(Outcome::Superseded),
        (false, Some((true, true))) => None,
    }
}
fn current_policy<A: Application>(
    state: &State<A>,
    address: &api::SurfaceAddress,
    original: &wl_surface::WlSurface,
) -> Option<api::InputReleaseOutcome> {
    let Some(native) = state
        .surfaces
        .get(address)
        .filter(|native| native.role.wl_surface() == original)
    else {
        return Some(api::InputReleaseOutcome::Retired);
    };
    (!released(&native.spec)).then_some(api::InputReleaseOutcome::Superseded)
}
impl<M: 'static> Releases<M> {
    fn insert(
        &mut self,
        request: api::InputReleaseRequest<M>,
        surface: wl_surface::WlSurface,
    ) -> Result<u64, api::InputReleaseRequest<M>> {
        let Some(id) = self.next_request.checked_add(1) else {
            return Err(request);
        };
        if self.requests.len() >= CAPACITY
            || self.frames.len() >= CAPACITY
            || self.next_frame == u64::MAX
        {
            return Err(request);
        }
        self.next_request = id;
        self.requests.insert(
            id,
            Pending {
                request,
                surface,
                frame: None,
                timer: None,
                deadline: Instant::now() + TIMEOUT,
            },
        );
        Ok(id)
    }
    /// Called only in the existing before-present transaction for None/None.
    /// Admission precedes this new callback; older callbacks have different IDs.
    pub(super) fn frame<D: Dispatch<WlCallback, FrameData> + 'static>(
        &mut self,
        surface: &wl_surface::WlSurface,
        none: bool,
        qh: &QueueHandle<D>,
    ) -> Option<u64> {
        if !none || self.frames.len() >= CAPACITY {
            return None;
        }
        let requests: Vec<_> = self
            .requests
            .iter()
            .filter_map(|(id, pending)| {
                (pending.surface == *surface
                    && pending.frame.is_none()
                    && Instant::now() < pending.deadline
                    && !pending.request.cancellation.cancelled())
                .then_some(*id)
            })
            .collect();
        if requests.is_empty() {
            return None;
        }
        let id = self.next_frame.checked_add(1)?;
        self.next_frame = id;
        let callback = surface.frame(
            qh,
            FrameData {
                id,
                surface: surface.clone(),
            },
        );
        for request in &requests {
            self.requests.get_mut(request).unwrap().frame = Some(id);
        }
        self.frames.insert(
            id,
            Frame {
                surface: surface.clone(),
                callback,
                requests,
                presented: false,
            },
        );
        Some(id)
    }
    pub(super) fn submitted(&mut self, id: Option<u64>, successful: bool) {
        let Some(id) = id else { return };
        let Some(frame) = self.frames.get_mut(&id) else {
            return;
        };
        frame.presented = successful;
        if !successful {
            // Preserve the abandoned callback's distinct identity until Done;
            // a retry can bind a fresh ID, never this failed attempt's callback.
            for request in &frame.requests {
                if let Some(pending) = self
                    .requests
                    .get_mut(request)
                    .filter(|pending| pending.frame == Some(id))
                {
                    pending.frame = None;
                }
            }
        }
    }
    fn take_frame(&mut self, proxy: &WlCallback, data: &FrameData) -> Option<Frame> {
        if !self
            .frames
            .get(&data.id)
            .is_some_and(|frame| frame.callback == *proxy && frame.surface == data.surface)
        {
            return None;
        }
        self.frames.remove(&data.id)
    }
    fn outcomes(
        &self,
        id: u64,
        frame: &Frame,
        mut validate: impl FnMut(
            &api::SurfaceAddress,
            &wl_surface::WlSurface,
        ) -> Option<api::InputReleaseOutcome>,
        mut now: impl FnMut() -> Instant,
    ) -> Vec<(u64, api::InputReleaseOutcome)> {
        frame
            .requests
            .iter()
            .filter_map(|id_request| {
                let pending = self.requests.get(id_request).filter(|pending| {
                    pending.frame == Some(id) && pending.surface == frame.surface
                })?;
                let checked = validate(&pending.request.surface, &pending.surface);
                // Sample after current lifetime/policy validation. A delayed
                // callback cannot beat a due timer by dispatch order alone.
                let outcome = if now() >= pending.deadline {
                    api::InputReleaseOutcome::Timeout
                } else if !frame.presented {
                    api::InputReleaseOutcome::Unavailable
                } else {
                    checked.unwrap_or(api::InputReleaseOutcome::Released)
                };
                Some((*id_request, outcome))
            })
            .collect()
    }
}
impl<A: Application> State<A> {
    fn input_release_finish(
        &mut self,
        id: u64,
        outcome: api::InputReleaseOutcome,
        qh: &QueueHandle<Self>,
        timer_fired: bool,
    ) {
        let Some(pending) = self.input_release.requests.remove(&id) else {
            return;
        };
        if !timer_fired && let Some(timer) = pending.timer {
            self.handle.remove(timer);
        }
        if let Some(message) = pending.request.finish(outcome) {
            self.message(message, qh);
        }
    }
    pub(super) fn input_release_request(
        &mut self,
        request: api::InputReleaseRequest<A::Message>,
        qh: &QueueHandle<Self>,
    ) {
        use api::InputReleaseOutcome as Outcome;
        if request.cancellation.cancelled() {
            return;
        }
        let failure = admission(
            self.authorize_unlock.is_some() || self.session_lock.is_some(),
            self.surfaces.get(&request.surface).map(|native| {
                (
                    matches!(&native.role, SurfaceRole::Layer(_)),
                    released(&native.spec),
                )
            }),
        );
        if let Some(outcome) = failure {
            if let Some(message) = request.finish(outcome) {
                self.message(message, qh);
            }
            return;
        }
        let address = request.surface.clone();
        let native = self.surfaces.get_mut(&address).unwrap();
        let surface = native.role.wl_surface().clone();
        let id = match self.input_release.insert(request, surface) {
            Ok(id) => id,
            Err(request) => {
                if let Some(message) = request.finish(Outcome::Busy) {
                    self.message(message, qh);
                }
                return;
            }
        };
        // Exactly one coalesced ordinary redraw, not a polling/repaint loop.
        self.surfaces.get_mut(&address).unwrap().demand.invalidate();
        let timer_qh = qh.clone();
        match self
            .handle
            .insert_source(Timer::from_duration(TIMEOUT), move |_, _, state| {
                state.input_release_finish(id, Outcome::Timeout, &timer_qh, true);
                TimeoutAction::Drop
            }) {
            Ok(timer) => self.input_release.requests.get_mut(&id).unwrap().timer = Some(timer),
            Err(_) => self.input_release_finish(id, Outcome::Unavailable, qh, false),
        }
    }
    pub(super) fn input_release_revalidate(&mut self, qh: &QueueHandle<Self>) {
        let completed: Vec<_> = self
            .input_release
            .requests
            .iter()
            .filter_map(|(id, pending)| {
                let reason = if pending.request.cancellation.cancelled() {
                    Some(api::InputReleaseOutcome::Retired) // finish suppresses cancelled delivery.
                } else if Instant::now() >= pending.deadline {
                    Some(api::InputReleaseOutcome::Timeout)
                } else {
                    current_policy(self, &pending.request.surface, &pending.surface)
                };
                reason.map(|reason| (*id, reason))
            })
            .collect();
        for (id, reason) in completed {
            self.input_release_finish(id, reason, qh, false);
        }
    }
}
impl<A: Application> Dispatch<WlCallback, FrameData> for State<A> {
    fn event(
        state: &mut Self,
        proxy: &WlCallback,
        _: wayland_client::protocol::wl_callback::Event,
        data: &FrameData,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let Some(frame) = state.input_release.take_frame(proxy, data) else {
            return;
        };
        let completed = state.input_release.outcomes(
            data.id,
            &frame,
            |address, surface| {
                current_policy(state, address, surface).or_else(|| {
                    (!state
                        .surfaces
                        .get(address)
                        .is_some_and(|native| released(&native.committed_spec)))
                    .then_some(api::InputReleaseOutcome::Superseded)
                })
            },
            Instant::now,
        );
        for (id, outcome) in completed {
            state.input_release_finish(id, outcome, qh, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay_client_toolkit::reexports::protocols_wlr::layer_shell::v1::client::{
        zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
        zwlr_layer_surface_v1::{KeyboardInteractivity, ZwlrLayerSurfaceV1},
    };
    use std::io::{Read, Write};
    use wayland_client::protocol::{
        wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::WlRegistry,
    };

    #[derive(Default)]
    struct Peer {
        done: Vec<(WlCallback, FrameData)>,
    }
    wayland_client::delegate_noop!(Peer: ignore WlRegistry);
    wayland_client::delegate_noop!(Peer: ignore WlCompositor);
    wayland_client::delegate_noop!(Peer: ignore WlRegion);
    wayland_client::delegate_noop!(Peer: ignore wl_surface::WlSurface);
    wayland_client::delegate_noop!(Peer: ignore ZwlrLayerShellV1);
    wayland_client::delegate_noop!(Peer: ignore ZwlrLayerSurfaceV1);
    impl Dispatch<WlCallback, FrameData> for Peer {
        fn event(
            state: &mut Self,
            proxy: &WlCallback,
            _: wayland_client::protocol::wl_callback::Event,
            data: &FrameData,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            state.done.push((proxy.clone(), data.clone()));
        }
    }
    fn spec(none: bool) -> api::SurfaceSpec {
        api::SurfaceSpec {
            id: "panel",
            instance: None,
            output: None,
            layer: api::Layer::Overlay,
            anchor: api::Anchor::Fill,
            margins: Default::default(),
            redraw_group: None,
            width: 0,
            height: 0,
            exclusive_zone: -1,
            keyboard: if none {
                api::Keyboard::None
            } else {
                api::Keyboard::Exclusive
            },
            visible: true,
            pointer_input: if none {
                api::PointerInput::None
            } else {
                api::PointerInput::All
            },
        }
    }
    fn request(cancel: api::Cancellation) -> api::InputReleaseRequest<api::InputReleaseOutcome> {
        api::InputReleaseRequest::new(
            api::SurfaceAddress::new("panel"),
            cancel,
            std::convert::identity,
        )
    }
    #[test]
    fn secure_missing_unsupported_or_interactive_targets_cannot_be_released() {
        use api::InputReleaseOutcome as Outcome;
        assert_eq!(
            admission(true, Some((true, true))),
            Some(Outcome::Unavailable)
        );
        assert_eq!(admission(false, None), Some(Outcome::Retired));
        assert_eq!(
            admission(false, Some((false, true))),
            Some(Outcome::Unavailable)
        );
        assert_eq!(
            admission(false, Some((true, false))),
            Some(Outcome::Superseded)
        );
        assert_eq!(admission(false, Some((true, true))), None);
        let mut one_policy = spec(true);
        one_policy.pointer_input = api::PointerInput::Content;
        assert!(!released(&one_policy));
    }
    fn socket() -> (
        Connection,
        wayland_client::EventQueue<Peer>,
        std::os::unix::net::UnixStream,
        wl_surface::WlSurface,
    ) {
        let (client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let queue = connection.new_event_queue::<Peer>();
        let qh = queue.handle();
        let registry = connection.display().get_registry(&qh, ());
        let compositor = registry.bind::<WlCompositor, _, _>(1, 1, &qh, ());
        let surface = compositor.create_surface(&qh, ());
        (connection, queue, server, surface)
    }
    fn receive_done(
        connection: &Connection,
        queue: &mut wayland_client::EventQueue<Peer>,
        server: &mut std::os::unix::net::UnixStream,
        frame: &Frame,
    ) -> (WlCallback, FrameData) {
        connection.flush().unwrap();
        server
            .write_all(&frame.callback.id().protocol_id().to_ne_bytes())
            .unwrap();
        server.write_all(&(12u32 << 16).to_ne_bytes()).unwrap();
        server.write_all(&1u32.to_ne_bytes()).unwrap();
        let mut peer = Peer::default();
        queue.blocking_dispatch(&mut peer).unwrap();
        assert_eq!(peer.done.len(), 1);
        peer.done.pop().unwrap()
    }

    #[test]
    fn real_wire_input_release_callback_is_in_same_commit_after_input_policy_requests() {
        let (connection, mut queue, mut server, surface) = socket();
        let qh = queue.handle();
        let registry = connection.display().get_registry(&qh, ());
        let compositor = registry.bind::<WlCompositor, _, _>(2, 1, &qh, ());
        let shell = registry.bind::<ZwlrLayerShellV1, _, _>(3, 1, &qh, ());
        let layer =
            shell.get_layer_surface(&surface, None, Layer::Overlay, "owned".into(), &qh, ());
        let empty = compositor.create_region(&qh, ());
        let mut releases = Releases::default();
        let id = releases
            .insert(request(api::Cancellation::default()), surface.clone())
            .ok()
            .unwrap();
        assert!(
            releases
                .frame(&surface, released(&spec(false)), &qh)
                .is_none()
        );
        assert!(releases.requests[&id].frame.is_none());
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        surface.set_input_region(Some(&empty));
        let frame = releases
            .frame(&surface, released(&spec(true)), &qh)
            .unwrap();
        surface.commit(); // Owned protocol peer stands in for the WSI commit.
        releases.submitted(Some(frame), true);
        connection.flush().unwrap();
        let mut observed = Vec::new();
        for _ in 0..32 {
            let mut header = [0u8; 8];
            server.read_exact(&mut header).unwrap();
            let object = u32::from_ne_bytes(header[..4].try_into().unwrap());
            let word = u32::from_ne_bytes(header[4..].try_into().unwrap());
            let size = (word >> 16) as usize;
            assert!((8..=4096).contains(&size));
            let opcode = word as u16;
            let mut payload = vec![0; size - 8];
            server.read_exact(&mut payload).unwrap();
            if object == layer.id().protocol_id() {
                observed.push(("layer", opcode, payload));
            } else if object == surface.id().protocol_id() {
                observed.push(("surface", opcode, payload));
                if opcode == 6 {
                    break;
                }
            }
        }
        assert_eq!(
            observed
                .iter()
                .map(|(kind, op, _)| (*kind, *op))
                .collect::<Vec<_>>(),
            [("layer", 4), ("surface", 5), ("surface", 3), ("surface", 6)]
        );
        assert_eq!(observed[0].2, 0u32.to_ne_bytes());
        assert_eq!(observed[1].2, empty.id().protocol_id().to_ne_bytes());
        assert_eq!(
            observed[2].2,
            releases.frames[&frame]
                .callback
                .id()
                .protocol_id()
                .to_ne_bytes()
        );
        // Local submission alone has not completed the pending request.
        assert!(releases.requests.contains_key(&id));
        let (proxy, data) = receive_done(
            &connection,
            &mut queue,
            &mut server,
            &releases.frames[&frame],
        );
        let replacement = compositor.create_surface(&qh, ());
        assert!(
            releases
                .take_frame(
                    &proxy,
                    &FrameData {
                        id: data.id,
                        surface: replacement
                    }
                )
                .is_none()
        );
        assert!(
            releases.frames.contains_key(&frame),
            "wrong native data cannot consume the real callback"
        );
        let acknowledged = releases.take_frame(&proxy, &data).unwrap();
        assert_eq!(
            releases.outcomes(
                data.id,
                &acknowledged,
                |_, _| None,
                || releases
                    .requests
                    .values()
                    .next()
                    .map(|pending| pending.deadline - Duration::from_secs(1))
                    .unwrap_or_else(Instant::now)
            ),
            [(id, api::InputReleaseOutcome::Released)]
        );
    }

    #[test]
    fn failed_or_older_callback_cannot_acknowledge_new_successful_binding() {
        let (connection, mut queue, mut server, surface) = socket();
        let qh = queue.handle();
        let mut releases = Releases::default();
        let id = releases
            .insert(request(api::Cancellation::default()), surface.clone())
            .ok()
            .unwrap();
        let failed = releases.frame(&surface, true, &qh).unwrap();
        releases.submitted(Some(failed), false);
        assert!(releases.requests[&id].frame.is_none());
        let valid = releases.frame(&surface, true, &qh).unwrap();
        assert_ne!(failed, valid);
        releases.submitted(Some(valid), true);
        surface.commit();
        let (proxy, data) = receive_done(
            &connection,
            &mut queue,
            &mut server,
            &releases.frames[&failed],
        );
        let old = releases.take_frame(&proxy, &data).unwrap();
        assert!(
            releases
                .outcomes(
                    data.id,
                    &old,
                    |_, _| None,
                    || releases
                        .requests
                        .values()
                        .next()
                        .map(|pending| pending.deadline - Duration::from_secs(1))
                        .unwrap_or_else(Instant::now)
                )
                .is_empty()
        );
        let (proxy, data) = receive_done(
            &connection,
            &mut queue,
            &mut server,
            &releases.frames[&valid],
        );
        let current = releases.take_frame(&proxy, &data).unwrap();
        assert_eq!(
            releases.outcomes(
                data.id,
                &current,
                |_, _| None,
                || releases
                    .requests
                    .values()
                    .next()
                    .map(|pending| pending.deadline - Duration::from_secs(1))
                    .unwrap_or_else(Instant::now)
            ),
            [(id, api::InputReleaseOutcome::Released)]
        );
    }

    #[test]
    fn earlier_requests_wrong_native_retirement_and_interactive_supersession_do_not_release() {
        let (connection, mut queue, mut server, surface) = socket();
        let qh = queue.handle();
        let mut releases = Releases::default();
        let old = releases
            .insert(request(api::Cancellation::default()), surface.clone())
            .ok()
            .unwrap();
        let frame = releases.frame(&surface, true, &qh).unwrap();
        releases.submitted(Some(frame), true);
        releases.requests.remove(&old);
        let new = releases
            .insert(request(api::Cancellation::default()), surface.clone())
            .ok()
            .unwrap();
        assert_ne!(new, old);
        surface.commit();
        let (proxy, data) = receive_done(
            &connection,
            &mut queue,
            &mut server,
            &releases.frames[&frame],
        );
        let old = releases.take_frame(&proxy, &data).unwrap();
        assert!(
            releases
                .outcomes(
                    data.id,
                    &old,
                    |_, _| None,
                    || releases
                        .requests
                        .values()
                        .next()
                        .map(|pending| pending.deadline - Duration::from_secs(1))
                        .unwrap_or_else(Instant::now)
                )
                .is_empty()
        );
        let frame = releases.frame(&surface, true, &qh).unwrap();
        releases.submitted(Some(frame), true);
        surface.commit();
        let (proxy, data) = receive_done(
            &connection,
            &mut queue,
            &mut server,
            &releases.frames[&frame],
        );
        let current = releases.take_frame(&proxy, &data).unwrap();
        for reason in [
            api::InputReleaseOutcome::Retired,
            api::InputReleaseOutcome::Superseded,
        ] {
            assert_eq!(
                releases.outcomes(
                    data.id,
                    &current,
                    |_, _| Some(reason),
                    || releases
                        .requests
                        .values()
                        .next()
                        .map(|pending| pending.deadline - Duration::from_secs(1))
                        .unwrap_or_else(Instant::now)
                ),
                [(new, reason)]
            );
        }
        let wrong = FrameData {
            id: data.id,
            surface: surface.clone(),
        };
        assert!(
            releases.take_frame(&proxy, &wrong).is_none(),
            "replayed callback has no record"
        );
    }

    #[test]
    fn cancelled_timed_out_and_abandoned_frames_remain_bounded_without_identity_reuse() {
        let (_connection, queue, _server, surface) = socket();
        let qh = queue.handle();
        let mut releases = Releases::default();
        for _ in 0..CAPACITY {
            let cancel = api::Cancellation::default();
            let id = releases
                .insert(request(cancel.clone()), surface.clone())
                .ok()
                .unwrap();
            let frame = releases.frame(&surface, true, &qh).unwrap();
            releases.submitted(Some(frame), true);
            let pending = releases.requests.remove(&id).unwrap();
            cancel.cancel();
            assert!(
                pending
                    .request
                    .finish(api::InputReleaseOutcome::Timeout)
                    .is_none()
            );
        }
        assert_eq!(releases.frames.len(), CAPACITY);
        assert!(
            releases
                .insert(request(api::Cancellation::default()), surface.clone())
                .is_err()
        );
        assert_eq!(TIMEOUT, Duration::from_secs(2));
        let mut timed = Releases::default();
        let timeout = timed
            .insert(request(api::Cancellation::default()), surface.clone())
            .ok()
            .unwrap();
        assert_eq!(
            timed
                .requests
                .remove(&timeout)
                .unwrap()
                .request
                .finish(api::InputReleaseOutcome::Timeout),
            Some(api::InputReleaseOutcome::Timeout)
        );
        let mut exhausted = Releases::default();
        exhausted.next_request = u64::MAX;
        assert!(
            exhausted
                .insert(request(api::Cancellation::default()), surface)
                .is_err()
        );
        assert_eq!(exhausted.next_request, u64::MAX);
    }

    #[test]
    fn overdue_callback_cannot_release_even_if_timer_dispatch_is_later() {
        use std::cell::Cell;
        let (connection, mut queue, mut server, surface) = socket();
        let qh = queue.handle();
        let mut releases = Releases::default();
        let id = releases
            .insert(request(api::Cancellation::default()), surface.clone())
            .ok()
            .unwrap();
        let frame = releases.frame(&surface, true, &qh).unwrap();
        releases.submitted(Some(frame), true);
        surface.commit();
        let (proxy, data) = receive_done(
            &connection,
            &mut queue,
            &mut server,
            &releases.frames[&frame],
        );
        let frame = releases.take_frame(&proxy, &data).unwrap();
        let deadline = releases.requests[&id].deadline;
        let observed = Cell::new(deadline - Duration::from_millis(1));
        let outcomes = releases.outcomes(
            data.id,
            &frame,
            |_, _| {
                observed.set(deadline + Duration::from_millis(1));
                None
            },
            || observed.get(),
        );
        assert_eq!(outcomes, [(id, api::InputReleaseOutcome::Timeout)]);
        assert!(
            releases.requests.contains_key(&id),
            "request still awaited its delayed timer until callback completion"
        );
    }
}
