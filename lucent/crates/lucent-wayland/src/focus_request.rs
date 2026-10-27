//! Optional one-shot native keyboard acquisition. Client intent is generic;
//! the compositor-specific grab exists only until a real Enter or termination.
use super::*;
use smithay_client_toolkit::registry::RegistryHandler;
use wayland_client::{
    Dispatch,
    backend::WaylandError,
    globals::GlobalList,
    protocol::wl_callback::{self, WlCallback},
};

mod wire {
    #![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
    #![allow(non_upper_case_globals, non_snake_case, unused_imports)]
    #![allow(missing_docs, clippy::all)]
    use wayland_client;
    use wayland_client::protocol::*;
    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/hyprland-focus-grab-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("protocols/hyprland-focus-grab-v1.xml");
}
use wire::{
    hyprland_focus_grab_manager_v1::HyprlandFocusGrabManagerV1,
    hyprland_focus_grab_v1::{self, HyprlandFocusGrabV1},
};

const TIMEOUT: Duration = Duration::from_secs(2);
const INTERFACE: &str = "hyprland_focus_grab_manager_v1";

/// Try once after an entire ordered control transaction. Backpressure belongs
/// to the normal Wayland source's next flush; it is neither an acknowledgement
/// nor a reason to wait, retry here, or claim keyboard ownership.
fn flush_control(flush: impl FnOnce() -> Result<(), WaylandError>) -> Result<(), WaylandError> {
    match flush() {
        Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
        result => result,
    }
}

pub(super) struct Origin {
    pub address: api::SurfaceAddress,
    pub surface: wl_surface::WlSurface,
    pub native: api::NativeSurfaceToken,
    pub seat: wl_seat::WlSeat,
}

struct Pending<M> {
    id: u64,
    request: api::KeyboardFocusRequest<M>,
    surface: wl_surface::WlSurface,
    native: api::NativeSurfaceToken,
    phase: Phase,
    timer: RegistrationToken,
    deadline: Instant,
}
impl<M> Pending<M> {
    fn matches_sync(&self, proxy: &WlCallback, data: &SyncData) -> bool {
        self.id == data.id
            && self.native == data.native
            && matches!(&self.phase, Phase::Retaining(sync) if &sync.0 == proxy)
    }
    fn matches_grab(&self, proxy: &HyprlandFocusGrabV1, data: &GrabData) -> bool {
        self.id == data.id
            && self.native == data.native
            && matches!(&self.phase, Phase::Acquiring(grab) if grab == proxy)
    }
}

enum Phase {
    Acquiring(HyprlandFocusGrabV1),
    Retaining(OwnedSync),
}

/// wl_callback has no destructor request. Explicit local teardown prevents a
/// cancelled queued Done from owning another acquisition or leaking a proxy.
struct OwnedSync(WlCallback);
impl Drop for OwnedSync {
    fn drop(&mut self) {
        if let Some(backend) = self.0.backend().upgrade() {
            let _ = backend.destroy_object(&self.0.id());
        }
    }
}

fn begin_retention<D: Dispatch<WlCallback, SyncData> + 'static>(
    connection: &Connection,
    qh: &QueueHandle<D>,
    grab: HyprlandFocusGrabV1,
    data: SyncData,
) -> OwnedSync {
    grab.destroy();
    OwnedSync(connection.display().sync(qh, data))
}

fn deliver_focused<M: 'static>(
    request: api::KeyboardFocusRequest<M>,
    native: api::NativeSurfaceToken,
    mut deliver: impl FnMut(M),
) {
    if let Some(message) = request.finish(api::KeyboardFocusOutcome::Focused(native)) {
        deliver(message);
    }
}

#[derive(Clone)]
pub(super) struct SyncData {
    id: u64,
    native: api::NativeSurfaceToken,
}

#[derive(Clone)]
pub(super) struct GrabData {
    id: u64,
    native: api::NativeSurfaceToken,
}

pub(super) enum Notice<M> {
    Completion(api::KeyboardFocusRequest<M>, api::KeyboardFocusOutcome),
    Lifecycle(
        api::SurfaceSpec,
        api::NativeSurfaceToken,
        api::KeyboardFocusChange,
    ),
}

pub(super) struct Requests<M> {
    manager: Option<HyprlandFocusGrabManagerV1>,
    global: Option<u32>,
    pending: Option<Pending<M>>,
    next: u64,
    pub origin: Option<Origin>,
    pub notices: Vec<Notice<M>>,
}

impl<M> Requests<M> {
    pub fn bind<A: Application>(
        globals: &GlobalList,
        qh: &QueueHandle<State<A>>,
        enabled: bool,
    ) -> Self {
        let manager: Option<HyprlandFocusGrabManagerV1> =
            enabled.then(|| globals.bind(qh, 1..=1, ()).ok()).flatten();
        let global = manager.as_ref().and_then(|_| {
            globals.contents().with_list(|list| {
                list.iter()
                    .find(|entry| entry.interface == INTERFACE)
                    .map(|entry| entry.name)
            })
        });
        Self {
            manager,
            global,
            pending: None,
            next: 0,
            origin: None,
            notices: vec![],
        }
    }

    fn id(&mut self) -> Option<u64> {
        let next = self.next.checked_add(1)?;
        self.next = next;
        Some(next)
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({
            "supported": self.manager.is_some(),
            "pending": self.pending.as_ref().map(|pending| serde_json::json!({
                "request": pending.id,
                "surface": pending.request.surface.to_string(),
                "phase": match &pending.phase {
                    Phase::Acquiring(_) => "awaiting-enter",
                    Phase::Retaining(_) => "verifying-retention",
                },
            })),
            "scope": "One-shot request; success requires exact native focus retained through post-grab teardown sync. Later native Leave remains authoritative.",
        })
    }
}

impl<M> Drop for Requests<M> {
    fn drop(&mut self) {
        // State first withdraws eligibility while its natives still exist.
        if let Some(pending) = self.pending.take()
            && let Phase::Acquiring(grab) = pending.phase
        {
            grab.destroy();
        }
        if let Some(manager) = self.manager.take() {
            manager.destroy();
        }
    }
}

/// Registry dispatch has no application policy and never automatically rearms.
pub(super) struct Registry;
impl<A: Application> RegistryHandler<State<A>> for Registry {
    fn remove_global(
        state: &mut State<A>,
        _: &Connection,
        qh: &QueueHandle<State<A>>,
        name: u32,
        _: &str,
    ) {
        if state.focus_requests.global != Some(name) {
            return;
        }
        state.focus_requests.global = None;
        state.focus_request_end(api::KeyboardFocusOutcome::Unavailable, false);
        if let Some(manager) = state.focus_requests.manager.take() {
            manager.destroy();
        }
        state.focus_request_flush(qh);
    }
}

#[derive(Clone, Copy)]
struct Admission {
    secure: bool,
    supported: bool,
    keyboard: bool,
    current_origin: bool,
    same_seat: bool,
    on_demand: bool,
    presented: bool,
}
fn admission(value: Admission) -> Option<api::KeyboardFocusOutcome> {
    use api::KeyboardFocusOutcome as Outcome;
    if value.secure || !value.supported || !value.keyboard {
        Some(Outcome::Unavailable)
    } else if !value.current_origin || !value.same_seat || !value.on_demand {
        Some(Outcome::Rejected)
    } else if !value.presented {
        Some(Outcome::Retired)
    } else {
        None
    }
}
fn already_owned(
    actual_enter: bool,
    requested: api::Keyboard,
    committed: api::Keyboard,
    pending_none: bool,
) -> bool {
    actual_enter
        && requested == api::Keyboard::OnDemand
        && committed == api::Keyboard::OnDemand
        && !pending_none
}

impl<A: Application> State<A> {
    fn flush_focus_control(&mut self) {
        if let Err(error) = flush_control(|| self.connection.flush()) {
            // Invalidate pending receipts before another already-queued native
            // event can complete them on a failed connection. The existing
            // runtime failure path still owns shutdown and surface teardown.
            self.focus_request_end(api::KeyboardFocusOutcome::Unavailable, false);
            self.error = Some(error.to_string());
            self.exit = true;
        }
    }

    pub(super) fn focus_requests_pending_for(&self, address: &api::SurfaceAddress) -> bool {
        self.focus_requests
            .pending
            .as_ref()
            .is_some_and(|pending| &pending.request.surface == address)
    }
    pub(super) fn focus_request_acquiring_for(&self, address: &api::SurfaceAddress) -> bool {
        self.focus_requests.pending.as_ref().is_some_and(|pending| {
            &pending.request.surface == address && matches!(&pending.phase, Phase::Acquiring(_))
        })
    }
    /// No attach, damage, frame callback, presentation request or history advance
    /// belongs in this commit. Restore all staged state against the old image.
    fn keyboard_control_commit(
        &mut self,
        address: &api::SurfaceAddress,
        original: &wl_surface::WlSurface,
        keyboard: api::Keyboard,
    ) -> Result<bool, Error> {
        let Some(native) = self
            .surfaces
            .get_mut(address)
            .filter(|native| native.role.wl_surface() == original)
        else {
            return Ok(false);
        };
        if native.committed_spec.keyboard == keyboard {
            return Ok(true);
        }
        let SurfaceRole::Layer(layer) = &native.role else {
            return Ok(false);
        };
        let Some(plan) =
            keyboard_control::plan(&native.committed_spec, native.presented.as_ref(), keyboard)
        else {
            return Ok(false);
        };
        // Allocate before queueing any restoration, so allocation failure does
        // not itself leave a partially staged control transaction.
        let region = Region::new(&self.compositor)?;
        for clip in &plan.presented.regions {
            add_region(&region, clip);
        }
        if let Some(fractional) = &mut native.fractional {
            fractional.destination(plan.presented.extent.0, plan.presented.extent.1)?;
        }
        configure_layer(layer, &plan.spec);
        layer
            .wl_surface()
            .set_buffer_scale(plan.presented.buffer_scale);
        layer
            .wl_surface()
            .set_input_region(Some(region.wl_region()));
        layer.commit();
        if keyboard == api::Keyboard::None {
            native.pending_keyboard_none = true;
        }
        plan.record_commit(&mut native.committed_spec, &mut native.regions);
        Ok(true)
    }

    pub(super) fn focus_request(
        &mut self,
        request: api::KeyboardFocusRequest<A::Message>,
        qh: &QueueHandle<Self>,
    ) {
        use api::KeyboardFocusOutcome as Outcome;
        if request.cancellation.cancelled() {
            return;
        }
        let origin = self.focus_requests.origin.take();
        let native = self.surfaces.get(&request.surface);
        let current_origin = origin.as_ref().is_some_and(|origin| {
            origin.address == request.surface
                && native.is_some_and(|native| {
                    native.role.wl_surface() == &origin.surface && native.identity == origin.native
                })
        });
        let check = Admission {
            secure: self.authorize_unlock.is_some() || self.session_lock.is_some(),
            supported: self.focus_requests.manager.is_some(),
            keyboard: self.keyboard.is_some(),
            current_origin,
            same_seat: origin
                .as_ref()
                .is_some_and(|origin| self.keyboard_seat.as_ref() == Some(&origin.seat)),
            on_demand: native.is_some_and(|native| {
                native.spec.keyboard == api::Keyboard::OnDemand
                    && matches!(&native.role, SurfaceRole::Layer(_))
            }),
            presented: native.is_some_and(|native| native.configured && native.presented.is_some()),
        };
        if let Some(outcome) = admission(check) {
            self.focus_requests
                .notices
                .push(Notice::Completion(request, outcome));
            return;
        }
        let origin = origin.expect("admitted exact prepared press");
        // A newer real press supersedes the old acquisition before its own
        // eligibility is committed. Neither scope survives as a pointer grab.
        self.focus_request_end(Outcome::Superseded, false);
        let Some(id) = self.focus_requests.id() else {
            self.focus_requests
                .notices
                .push(Notice::Completion(request, Outcome::Unavailable));
            return;
        };
        if let Some(native) = self.surfaces.get_mut(&request.surface) {
            native.keyboard_claimed = true;
        }
        let already_entered = self.surfaces.get(&request.surface).is_some_and(|native| {
            already_owned(
                self.keyboard_surface.as_ref() == Some(&request.surface),
                native.spec.keyboard,
                native.committed_spec.keyboard,
                native.pending_keyboard_none,
            )
        });
        match self.keyboard_control_commit(
            &request.surface,
            &origin.surface,
            api::Keyboard::OnDemand,
        ) {
            Ok(true) => {}
            Ok(false) => {
                self.focus_requests
                    .notices
                    .push(Notice::Completion(request, Outcome::Retired));
                return;
            }
            Err(error) => {
                self.error = Some(error.to_string());
                self.exit = true;
                self.focus_requests
                    .notices
                    .push(Notice::Completion(request, Outcome::Unavailable));
                return;
            }
        }
        if already_entered {
            // A stable already-owned native needs no grab or new synthetic
            // Enter. A None->OnDemand promotion cannot take this shortcut.
            deliver_focused(request, origin.native, |message| self.message(message, qh));
            return;
        }
        let data = GrabData {
            id,
            native: origin.native.clone(),
        };
        let grab = self
            .focus_requests
            .manager
            .as_ref()
            .expect("admitted global")
            .create_grab(qh, data);
        let timeout_qh = qh.clone();
        let timer =
            match self
                .handle
                .insert_source(Timer::from_duration(TIMEOUT), move |_, _, state| {
                    if state
                        .focus_requests
                        .pending
                        .as_ref()
                        .is_some_and(|pending| pending.id == id)
                    {
                        state.focus_request_end(Outcome::Timeout, true);
                        state.focus_request_flush(&timeout_qh);
                    }
                    TimeoutAction::Drop
                }) {
                Ok(timer) => timer,
                Err(_) => {
                    if let Err(error) = self.keyboard_control_commit(
                        &request.surface,
                        &origin.surface,
                        api::Keyboard::None,
                    ) {
                        self.error = Some(error.to_string());
                        self.exit = true;
                    }
                    grab.destroy();
                    self.focus_requests
                        .notices
                        .push(Notice::Completion(request, Outcome::Unavailable));
                    return;
                }
            };
        grab.add_surface(&origin.surface);
        self.focus_requests.pending = Some(Pending {
            id,
            request,
            surface: origin.surface,
            native: origin.native,
            phase: Phase::Acquiring(grab.clone()),
            timer,
            deadline: Instant::now() + TIMEOUT,
        });
        // The keyboard-only role commit above precedes this request on the same
        // Wayland connection; acquiring never depends on an own frame callback.
        grab.commit();
        // Do not leave this already-complete transaction behind unrelated
        // callbacks/layout work while WaylandSource drains its pending queue.
        self.flush_focus_control();
    }

    pub(super) fn focus_request_end(
        &mut self,
        outcome: api::KeyboardFocusOutcome,
        timer_fired: bool,
    ) {
        let Some(pending) = self.focus_requests.pending.take() else {
            return;
        };
        if !timer_fired {
            self.handle.remove(pending.timer);
        }
        // Failure/cancellation withdraws eligibility BEFORE destroying the
        // grab, so ordinary compositor refocus cannot select this old demand.
        if let Err(error) = self.keyboard_control_commit(
            &pending.request.surface,
            &pending.surface,
            api::Keyboard::None,
        ) {
            self.error = Some(error.to_string());
            self.exit = true;
        }
        if let Phase::Acquiring(grab) = pending.phase {
            grab.destroy();
        }
        self.focus_requests
            .notices
            .push(Notice::Completion(pending.request, outcome));
    }

    pub(super) fn focus_request_revalidate(&mut self) {
        use api::KeyboardFocusOutcome as Outcome;
        // A completed one-shot has no retained protocol object. Its client can
        // still withdraw the acknowledged ordinary OnDemand intent without
        // waiting for another GPU frame or exposing staged image geometry.
        let withdrawn: Vec<_> = self
            .surfaces
            .iter()
            .filter(|(_, native)| {
                native.keyboard_claimed && native.spec.keyboard == api::Keyboard::None
            })
            .map(|(address, native)| (address.clone(), native.role.wl_surface().clone()))
            .collect();
        for (address, surface) in withdrawn {
            match self.keyboard_control_commit(&address, &surface, api::Keyboard::None) {
                Ok(true) => {
                    if let Some(native) = self.surfaces.get_mut(&address) {
                        native.keyboard_claimed = false;
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.exit = true;
                }
            }
        }
        let outcome = self.focus_requests.pending.as_ref().and_then(|pending| {
            if pending.request.cancellation.cancelled() {
                return Some(Outcome::Superseded);
            }
            if self.keyboard.is_none() {
                return Some(Outcome::Unavailable);
            }
            if Instant::now() >= pending.deadline {
                return Some(Outcome::Timeout);
            }
            let Some(native) = self
                .surfaces
                .get(&pending.request.surface)
                .filter(|native| {
                    native.role.wl_surface() == &pending.surface
                        && native.identity == pending.native
                })
            else {
                return Some(Outcome::Retired);
            };
            (native.spec.keyboard != api::Keyboard::OnDemand).then_some(Outcome::Superseded)
        });
        if let Some(outcome) = outcome {
            self.focus_request_end(outcome, false);
        }
    }

    pub(super) fn focus_request_enter(
        &mut self,
        surface: &wl_surface::WlSurface,
        qh: &QueueHandle<Self>,
    ) {
        self.focus_request_revalidate();
        let valid = self.focus_requests.pending.as_ref().is_some_and(|pending| {
            &pending.surface == surface
                && self
                    .surfaces
                    .get(&pending.request.surface)
                    .is_some_and(|native| {
                        native.role.wl_surface() == surface
                            && native.identity == pending.native
                            && native.committed_spec.keyboard == api::Keyboard::OnDemand
                    })
                && self.keyboard_surface.as_ref() == Some(&pending.request.surface)
                && matches!(&pending.phase, Phase::Acquiring(_))
        });
        if !valid {
            return;
        }
        let pending = self.focus_requests.pending.take().expect("validated Enter");
        let Phase::Acquiring(grab) = pending.phase else {
            unreachable!()
        };
        // Successful acquisition is ONE SHOT. Keep ordinary OnDemand policy;
        // remove the pointer grab immediately, without fabricating a Leave.
        // A separate bounded sync verifies that this teardown did not itself
        // refocus another native before reporting successful retention.
        let callback = begin_retention(
            &self.connection,
            qh,
            grab,
            SyncData {
                id: pending.id,
                native: pending.native.clone(),
            },
        );
        self.focus_requests.pending = Some(Pending {
            phase: Phase::Retaining(callback),
            ..pending
        });
        // Destroy and the following sync must travel together, after the
        // receipt is installed. Flush success itself never reports Focused.
        self.flush_focus_control();
    }

    pub(super) fn focus_request_lifecycle(
        &mut self,
        spec: api::SurfaceSpec,
        native: api::NativeSurfaceToken,
        change: api::KeyboardFocusChange,
    ) {
        self.focus_requests
            .notices
            .push(Notice::Lifecycle(spec, native, change));
    }

    pub(super) fn focus_request_flush(&mut self, qh: &QueueHandle<Self>) {
        for notice in std::mem::take(&mut self.focus_requests.notices) {
            let message = match notice {
                Notice::Completion(request, outcome) => request.finish(outcome),
                Notice::Lifecycle(spec, native, change) => {
                    self.app.event(spec.scope_event(api::Event::KeyboardFocus {
                        surface: spec.id,
                        native,
                        change,
                    }))
                }
            };
            if let Some(message) = message {
                self.message(message, qh);
            }
        }
    }
}

impl<A: Application> Dispatch<HyprlandFocusGrabV1, GrabData> for State<A> {
    fn event(
        state: &mut Self,
        proxy: &HyprlandFocusGrabV1,
        event: hyprland_focus_grab_v1::Event,
        data: &GrabData,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if matches!(event, hyprland_focus_grab_v1::Event::Cleared)
            && state
                .focus_requests
                .pending
                .as_ref()
                .is_some_and(|pending| pending.matches_grab(proxy, data))
        {
            state.focus_request_end(api::KeyboardFocusOutcome::Cleared, false);
            state.focus_request_flush(qh);
        }
    }
}
wayland_client::delegate_noop!(@<A:Application> State<A>:HyprlandFocusGrabManagerV1);

impl<A: Application> Dispatch<WlCallback, SyncData> for State<A> {
    fn event(
        state: &mut Self,
        proxy: &WlCallback,
        event: wl_callback::Event,
        data: &SyncData,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if !matches!(event, wl_callback::Event::Done { .. }) {
            return;
        }
        // Done commonly makes the proxy dead before this dispatch. Exact owned
        // identity, not is_alive(), decides whether queued valid Done belongs.
        let valid = state
            .focus_requests
            .pending
            .as_ref()
            .is_some_and(|pending| pending.matches_sync(proxy, data));
        if !valid {
            return;
        }
        state.focus_request_revalidate();
        let Some(pending) = state.focus_requests.pending.take() else {
            state.focus_request_flush(qh);
            return;
        };
        state.handle.remove(pending.timer);
        let retained = state.keyboard_surface.as_ref() == Some(&pending.request.surface)
            && state
                .surfaces
                .get(&pending.request.surface)
                .is_some_and(|native| {
                    native.role.wl_surface() == &pending.surface
                        && native.identity == pending.native
                        && native.spec.keyboard == api::Keyboard::OnDemand
                        && native.committed_spec.keyboard == api::Keyboard::OnDemand
                });
        drop(pending.phase);
        if retained {
            if let Some(native) = state.surfaces.get_mut(&pending.request.surface) {
                native.pending_keyboard_none = false;
            }
            // Complete synchronously before any later native loss delivery.
            deliver_focused(pending.request, pending.native, |message| {
                state.message(message, qh)
            });
        } else {
            if let Err(error) = state.keyboard_control_commit(
                &pending.request.surface,
                &pending.surface,
                api::Keyboard::None,
            ) {
                state.error = Some(error.to_string());
                state.exit = true;
            }
            state.focus_requests.notices.push(Notice::Completion(
                pending.request,
                api::KeyboardFocusOutcome::NotRetained,
            ));
        }
        state.focus_request_flush(qh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
    };
    use wayland_client::{
        EventQueue,
        protocol::{wl_compositor::WlCompositor, wl_registry::WlRegistry},
    };
    #[test]
    fn control_flush_attempts_success_once_without_an_ownership_receipt() {
        let calls = std::cell::Cell::new(0);
        assert!(
            flush_control(|| {
                calls.set(calls.get() + 1);
                Ok(())
            })
            .is_ok()
        );
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn control_flush_defers_would_block_without_retrying_or_waiting() {
        let calls = std::cell::Cell::new(0);
        assert!(
            flush_control(|| {
                calls.set(calls.get() + 1);
                Err(WaylandError::Io(std::io::ErrorKind::WouldBlock.into()))
            })
            .is_ok()
        );
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn control_flush_preserves_fatal_io_errors() {
        let result = flush_control(|| Err(WaylandError::Io(std::io::ErrorKind::BrokenPipe.into())));
        assert!(matches!(result, Err(WaylandError::Io(error))
            if error.kind() == std::io::ErrorKind::BrokenPipe));
    }
    #[test]
    fn control_flush_preserves_protocol_failure() {
        let result = flush_control(|| {
            Err(WaylandError::Protocol(
                wayland_client::backend::protocol::ProtocolError {
                    code: 1,
                    object_id: 1,
                    object_interface: "wl_display".into(),
                    message: "test refusal".into(),
                },
            ))
        });
        assert!(matches!(result, Err(WaylandError::Protocol(error))
            if error.code == 1 && error.object_interface == "wl_display"));
    }
    fn valid() -> Admission {
        Admission {
            secure: false,
            supported: true,
            keyboard: true,
            current_origin: true,
            same_seat: true,
            on_demand: true,
            presented: true,
        }
    }
    #[test]
    fn only_current_prepared_press_with_keyboard_capability_can_admit() {
        assert_eq!(admission(valid()), None);
        assert_eq!(
            admission(Admission {
                current_origin: false,
                ..valid()
            }),
            Some(api::KeyboardFocusOutcome::Rejected)
        );
        assert_eq!(
            admission(Admission {
                same_seat: false,
                ..valid()
            }),
            Some(api::KeyboardFocusOutcome::Rejected)
        );
        assert_eq!(
            admission(Admission {
                on_demand: false,
                ..valid()
            }),
            Some(api::KeyboardFocusOutcome::Rejected)
        );
    }
    #[test]
    fn secure_unsupported_missing_keyboard_and_unpresented_targets_do_not_acquire() {
        for check in [
            Admission {
                secure: true,
                ..valid()
            },
            Admission {
                supported: false,
                ..valid()
            },
            Admission {
                keyboard: false,
                ..valid()
            },
        ] {
            assert_eq!(
                admission(check),
                Some(api::KeyboardFocusOutcome::Unavailable)
            );
        }
        assert_eq!(
            admission(Admission {
                presented: false,
                ..valid()
            }),
            Some(api::KeyboardFocusOutcome::Retired)
        );
    }
    #[test]
    fn request_identifiers_never_wrap_or_reuse() {
        let mut requests: Requests<()> = Requests {
            manager: None,
            global: None,
            pending: None,
            next: u64::MAX - 1,
            origin: None,
            notices: vec![],
        };
        assert_eq!(requests.id(), Some(u64::MAX));
        assert_eq!(requests.id(), None);
        assert_eq!(requests.id(), None);
    }
    #[test]
    fn already_owned_noop_excludes_control_promotion_and_pending_none() {
        use api::Keyboard::{None, OnDemand};
        assert!(already_owned(true, OnDemand, OnDemand, false));
        assert!(!already_owned(false, OnDemand, OnDemand, false));
        assert!(!already_owned(true, OnDemand, None, false));
        assert!(!already_owned(true, None, OnDemand, false));
        assert!(!already_owned(true, OnDemand, OnDemand, true));
    }

    // This wire peer exercises real queue ordering and the same receipt matcher
    // and synchronous completion function as the native driver, without a GPU.
    struct Peer {
        pending: Option<Pending<api::KeyboardFocusOutcome>>,
        current: Option<wl_surface::WlSurface>,
        native: api::NativeSurfaceToken,
        client_pending: bool,
        client_owner: Option<api::NativeSurfaceToken>,
        trace: Vec<&'static str>,
    }
    impl Peer {
        fn accept(&mut self, outcome: api::KeyboardFocusOutcome) {
            self.client_pending = false;
            match outcome {
                api::KeyboardFocusOutcome::Focused(native) => {
                    self.client_owner = Some(native);
                    self.trace.push("focused");
                }
                api::KeyboardFocusOutcome::NotRetained => {
                    self.client_owner = None;
                    self.trace.push("not-retained");
                }
                _ => self.trace.push("other-outcome"),
            }
        }
    }
    wayland_client::delegate_noop!(Peer: ignore WlRegistry);
    wayland_client::delegate_noop!(Peer: ignore WlCompositor);
    wayland_client::delegate_noop!(Peer: ignore wl_surface::WlSurface);
    wayland_client::delegate_noop!(Peer: ignore wl_seat::WlSeat);
    wayland_client::delegate_noop!(Peer: ignore HyprlandFocusGrabManagerV1);
    impl Dispatch<HyprlandFocusGrabV1, GrabData> for Peer {
        fn event(
            _: &mut Self,
            _: &HyprlandFocusGrabV1,
            _: hyprland_focus_grab_v1::Event,
            _: &GrabData,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<wl_keyboard::WlKeyboard, ()> for Peer {
        fn event(
            state: &mut Self,
            _: &wl_keyboard::WlKeyboard,
            event: wl_keyboard::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                wl_keyboard::Event::Enter { surface, .. } => {
                    state.current = Some(surface);
                    state.trace.push("enter");
                }
                wl_keyboard::Event::Leave { surface, .. } => {
                    if state.current.as_ref() == Some(&surface) {
                        state.current = None;
                    }
                    state.trace.push("leave");
                    // Like the client, pending intent cannot infer ownership
                    // from a Leave that predates its acquisition receipt.
                    if !state.client_pending && state.client_owner.as_ref() == Some(&state.native) {
                        state.client_owner = None;
                        state.trace.push("loss");
                    }
                }
                _ => {}
            }
        }
    }
    impl Dispatch<WlCallback, SyncData> for Peer {
        fn event(
            state: &mut Self,
            proxy: &WlCallback,
            event: wl_callback::Event,
            data: &SyncData,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if !matches!(event, wl_callback::Event::Done { .. })
                || !state
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.matches_sync(proxy, data))
            {
                return;
            }
            let pending = state.pending.take().unwrap();
            let retained =
                state.current.as_ref() == Some(&pending.surface) && state.native == pending.native;
            drop(pending.phase);
            if retained {
                deliver_focused(pending.request, pending.native, |message| {
                    state.accept(message)
                });
            } else if let Some(message) = pending
                .request
                .finish(api::KeyboardFocusOutcome::NotRetained)
            {
                state.accept(message);
            }
            state.trace.push("sync");
        }
    }
    fn wire_socket() -> (
        Connection,
        EventQueue<Peer>,
        UnixStream,
        WlCompositor,
        wl_keyboard::WlKeyboard,
    ) {
        let (client, server) = UnixStream::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let queue = connection.new_event_queue::<Peer>();
        let qh = queue.handle();
        let registry = connection.display().get_registry(&qh, ());
        let compositor = registry.bind::<WlCompositor, _, _>(1, 1, &qh, ());
        let seat = registry.bind::<wl_seat::WlSeat, _, _>(2, 5, &qh, ());
        let keyboard = seat.get_keyboard(&qh, ());
        (connection, queue, server, compositor, keyboard)
    }
    fn event(bytes: &mut Vec<u8>, object: u32, opcode: u16, payload: &[u32]) {
        bytes.extend_from_slice(&object.to_ne_bytes());
        bytes.extend_from_slice(
            &((((payload.len() * 4 + 8) as u32) << 16) | u32::from(opcode)).to_ne_bytes(),
        );
        for word in payload {
            bytes.extend_from_slice(&word.to_ne_bytes());
        }
    }
    struct Fixture {
        connection: Connection,
        queue: EventQueue<Peer>,
        server: UnixStream,
        compositor: WlCompositor,
        keyboard: wl_keyboard::WlKeyboard,
        surface: wl_surface::WlSurface,
        callback: WlCallback,
        data: SyncData,
        peer: Peer,
        _event_loop: EventLoop<'static, Peer>,
    }
    fn fixture(cancel: api::Cancellation) -> Fixture {
        let (connection, queue, server, compositor, keyboard) = wire_socket();
        let surface = compositor.create_surface(&queue.handle(), ());
        let native = api::NativeSurfaceToken::new();
        let data = SyncData {
            id: 1,
            native: native.clone(),
        };
        let callback = connection.display().sync(&queue.handle(), data.clone());
        let event_loop = EventLoop::<Peer>::try_new().unwrap();
        let timer = event_loop
            .handle()
            .insert_source(Timer::from_duration(TIMEOUT), |_, _, _| TimeoutAction::Drop)
            .unwrap();
        let peer = Peer {
            pending: Some(Pending {
                id: 1,
                request: api::KeyboardFocusRequest::new(
                    api::SurfaceAddress::new("notes"),
                    cancel,
                    std::convert::identity,
                ),
                surface: surface.clone(),
                native: native.clone(),
                phase: Phase::Retaining(OwnedSync(callback.clone())),
                timer,
                deadline: Instant::now() + TIMEOUT,
            }),
            current: None,
            native,
            client_pending: true,
            client_owner: None,
            trace: vec![],
        };
        Fixture {
            connection,
            queue,
            server,
            compositor,
            keyboard,
            surface,
            callback,
            data,
            peer,
            _event_loop: event_loop,
        }
    }
    fn dispatch(fixture: &mut Fixture, bytes: &[u8]) {
        fixture.connection.flush().unwrap();
        fixture.server.write_all(bytes).unwrap();
        fixture.connection.prepare_read().unwrap().read().unwrap();
        fixture.queue.dispatch_pending(&mut fixture.peer).unwrap();
    }
    #[test]
    fn queued_sync_done_completes_before_a_following_real_leave_in_one_dispatch_batch() {
        let mut f = fixture(api::Cancellation::default());
        let mut bytes = vec![];
        event(
            &mut bytes,
            f.keyboard.id().protocol_id(),
            2,
            &[1, f.surface.id().protocol_id()],
        );
        event(
            &mut bytes,
            f.keyboard.id().protocol_id(),
            1,
            &[2, f.surface.id().protocol_id(), 0],
        );
        event(&mut bytes, f.callback.id().protocol_id(), 0, &[3]);
        event(&mut bytes, 1, 1, &[f.callback.id().protocol_id()]);
        event(
            &mut bytes,
            f.keyboard.id().protocol_id(),
            2,
            &[4, f.surface.id().protocol_id()],
        );
        dispatch(&mut f, &bytes);
        assert_eq!(
            f.peer.trace,
            ["leave", "enter", "focused", "sync", "leave", "loss"]
        );
        assert!(!f.peer.client_pending);
        assert!(f.peer.client_owner.is_none());
        assert!(f.peer.pending.is_none());
    }
    #[test]
    fn a_leave_before_sync_is_not_retained_and_cannot_arm_late() {
        let mut f = fixture(api::Cancellation::default());
        let mut bytes = vec![];
        event(
            &mut bytes,
            f.keyboard.id().protocol_id(),
            1,
            &[1, f.surface.id().protocol_id(), 0],
        );
        event(
            &mut bytes,
            f.keyboard.id().protocol_id(),
            2,
            &[2, f.surface.id().protocol_id()],
        );
        event(&mut bytes, f.callback.id().protocol_id(), 0, &[3]);
        event(&mut bytes, 1, 1, &[f.callback.id().protocol_id()]);
        dispatch(&mut f, &bytes);
        assert_eq!(f.peer.trace, ["enter", "leave", "not-retained", "sync"]);
        assert!(f.peer.client_owner.is_none());
    }
    #[test]
    fn another_native_enter_before_sync_cannot_retain_the_previous_routing_address() {
        let mut f = fixture(api::Cancellation::default());
        let retired_or_other = f.compositor.create_surface(&f.queue.handle(), ());
        let mut bytes = vec![];
        event(
            &mut bytes,
            f.keyboard.id().protocol_id(),
            1,
            &[1, f.surface.id().protocol_id(), 0],
        );
        event(
            &mut bytes,
            f.keyboard.id().protocol_id(),
            1,
            &[2, retired_or_other.id().protocol_id(), 0],
        );
        event(&mut bytes, f.callback.id().protocol_id(), 0, &[3]);
        event(&mut bytes, 1, 1, &[f.callback.id().protocol_id()]);
        dispatch(&mut f, &bytes);
        assert_eq!(f.peer.trace, ["enter", "enter", "not-retained", "sync"]);
        assert!(f.peer.client_owner.is_none());
    }
    #[test]
    fn queued_valid_done_survives_proxy_death_but_reused_ids_and_native_replacements_do_not_match()
    {
        let mut f = fixture(api::Cancellation::default());
        f.connection.flush().unwrap();
        let mut bytes = vec![];
        event(&mut bytes, f.callback.id().protocol_id(), 0, &[1]);
        event(&mut bytes, 1, 1, &[f.callback.id().protocol_id()]);
        f.server.write_all(&bytes).unwrap();
        f.connection.prepare_read().unwrap().read().unwrap();
        assert!(!f.callback.is_alive());
        assert!(
            f.peer
                .pending
                .as_ref()
                .unwrap()
                .matches_sync(&f.callback, &f.data)
        );
        let replacement = f
            .connection
            .display()
            .sync(&f.queue.handle(), f.data.clone());
        assert_eq!(
            replacement.id().protocol_id(),
            f.callback.id().protocol_id()
        );
        assert_ne!(replacement, f.callback);
        let pending = f.peer.pending.as_ref().unwrap();
        assert!(!pending.matches_sync(&replacement, &f.data));
        assert!(!pending.matches_sync(
            &f.callback,
            &SyncData {
                native: api::NativeSurfaceToken::new(),
                ..f.data.clone()
            }
        ));
        assert!(!pending.matches_sync(
            &f.callback,
            &SyncData {
                id: 2,
                ..f.data.clone()
            }
        ));
        // Retiring the old owner cannot destroy an object which reused its
        // protocol number, and its already-queued Done cannot finish another
        // request after that owner was removed.
        drop(f.peer.pending.take());
        assert!(replacement.is_alive());
        f.queue.dispatch_pending(&mut f.peer).unwrap();
        assert!(f.peer.trace.is_empty());
        assert!(replacement.is_alive());
        // Explicit local teardown retires only the replacement callback.
        drop(OwnedSync(replacement));
    }
    #[test]
    fn cancellation_before_sync_dispatch_suppresses_completion() {
        let cancellation = api::Cancellation::default();
        let mut f = fixture(cancellation.clone());
        let mut bytes = vec![];
        event(
            &mut bytes,
            f.keyboard.id().protocol_id(),
            1,
            &[1, f.surface.id().protocol_id(), 0],
        );
        event(&mut bytes, f.callback.id().protocol_id(), 0, &[2]);
        event(&mut bytes, 1, 1, &[f.callback.id().protocol_id()]);
        cancellation.cancel();
        dispatch(&mut f, &bytes);
        assert_eq!(f.peer.trace, ["enter", "sync"]);
        assert!(f.peer.client_owner.is_none());
    }
    #[test]
    fn internal_grab_destroy_precedes_sync_and_adds_no_surface_or_frame_request() {
        let (connection, queue, mut server, compositor, _) = wire_socket();
        let qh = queue.handle();
        let registry = connection.display().get_registry(&qh, ());
        let manager = registry.bind::<HyprlandFocusGrabManagerV1, _, _>(3, 1, &qh, ());
        let native = api::NativeSurfaceToken::new();
        let surface = compositor.create_surface(&qh, ());
        let grab = manager.create_grab(
            &qh,
            GrabData {
                id: 1,
                native: native.clone(),
            },
        );
        grab.add_surface(&surface);
        grab.commit();
        let grab_id = grab.id().protocol_id();
        let _sync = begin_retention(&connection, &qh, grab, SyncData { id: 1, native });
        // Use the production control flush, without running an event source or
        // dispatching a callback. The server must already see teardown + sync.
        flush_control(|| connection.flush()).unwrap();
        let mut observed = vec![];
        loop {
            let mut header = [0u8; 8];
            server.read_exact(&mut header).unwrap();
            let object = u32::from_ne_bytes(header[..4].try_into().unwrap());
            let word = u32::from_ne_bytes(header[4..].try_into().unwrap());
            let opcode = word as u16;
            let mut payload = vec![0; (word >> 16) as usize - 8];
            server.read_exact(&mut payload).unwrap();
            assert_ne!(
                object,
                surface.id().protocol_id(),
                "No surface attach/damage/frame/commit belongs to grab teardown"
            );
            if object == grab_id {
                observed.push(("grab", opcode));
            }
            if object == 1 && opcode == 0 {
                observed.push(("display", opcode));
                break;
            }
        }
        assert_eq!(
            observed,
            [("grab", 0), ("grab", 2), ("grab", 3), ("display", 0)]
        );
    }
    #[test]
    fn control_flush_sends_complete_acquisition_before_peer_dispatch() {
        let (connection, queue, mut server, compositor, _) = wire_socket();
        let qh = queue.handle();
        let registry = connection.display().get_registry(&qh, ());
        let manager = registry.bind::<HyprlandFocusGrabManagerV1, _, _>(3, 1, &qh, ());
        let surface = compositor.create_surface(&qh, ());
        let grab = manager.create_grab(
            &qh,
            GrabData {
                id: 1,
                native: api::NativeSurfaceToken::new(),
            },
        );
        surface.commit();
        grab.add_surface(&surface);
        grab.commit();
        flush_control(|| connection.flush()).unwrap();
        let mut observed = vec![];
        loop {
            let mut header = [0u8; 8];
            server.read_exact(&mut header).unwrap();
            let object = u32::from_ne_bytes(header[..4].try_into().unwrap());
            let word = u32::from_ne_bytes(header[4..].try_into().unwrap());
            let opcode = word as u16;
            let mut payload = vec![0; (word >> 16) as usize - 8];
            server.read_exact(&mut payload).unwrap();
            if object == surface.id().protocol_id() {
                assert_eq!(opcode, 6, "Only the queued surface commit is allowed");
                observed.push(("surface", opcode));
            }
            if object == grab.id().protocol_id() {
                observed.push(("grab", opcode));
                if opcode == 2 {
                    break;
                }
            }
        }
        assert_eq!(observed, [("surface", 6), ("grab", 0), ("grab", 2)]);
        // No Enter/Done was injected or dispatched: flushing is transport only.
    }
    #[test]
    fn owned_sync_teardown_retires_proxy_and_accepts_later_delete_id() {
        let mut f = fixture(api::Cancellation::default());
        let callback_id = f.callback.id().protocol_id();
        drop(f.peer.pending.take());
        assert!(!f.callback.is_alive());
        let mut bytes = vec![];
        event(&mut bytes, callback_id, 0, &[1]);
        event(&mut bytes, 1, 1, &[callback_id]);
        dispatch(&mut f, &bytes);
        assert!(f.connection.backend().last_error().is_none());
        assert!(f.peer.trace.is_empty());
        let replacement = f.compositor.create_surface(&f.queue.handle(), ());
        assert_eq!(replacement.id().protocol_id(), callback_id);
    }
}
