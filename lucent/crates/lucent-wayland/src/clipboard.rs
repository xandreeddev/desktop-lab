//! Core Wayland text selection. No desktop identities, shell commands or history.
use crate::State;
use calloop::{
    Mode, PostAction, RegistrationToken,
    generic::Generic,
    timer::{TimeoutAction, Timer},
};
use lucent_api::{
    self as api, Application, CLIPBOARD_TEXT_LIMIT, ClipboardAction, ClipboardError,
    ClipboardOutcome, ClipboardRequest, ClipboardTarget, ViewContext,
};
use smithay_client_toolkit::data_device_manager::{
    DataDeviceManagerState, WritePipe,
    data_device::{DataDevice, DataDeviceHandler},
    data_offer::{DataOfferHandler, DragOffer},
    data_source::{CopyPasteSource, DataSourceHandler},
};
use std::{collections::BTreeMap, fs::File, os::fd::OwnedFd, sync::Arc, time::Instant};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::DndAction,
        wl_data_source::WlDataSource, wl_seat, wl_surface,
    },
};
mod io;
const MIME: &[&str] = &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];
const MAX_TRANSFERS: usize = 8;
struct Guard {
    target: ClipboardTarget,
    epoch: u64,
    surface: wl_surface::WlSurface,
}
struct Pending<M> {
    token: RegistrationToken,
    deadline: Instant,
    transfer: io::Transfer,
    request: Option<ClipboardRequest<M>>,
    guard: Option<Guard>,
}
pub(super) struct Clipboard<M> {
    manager: Option<DataDeviceManagerState>,
    device: Option<DataDevice>,
    source: Option<(CopyPasteSource, Arc<[u8]>)>,
    pending: BTreeMap<u64, Pending<M>>,
    next: u64,
    timer: Option<RegistrationToken>,
    epoch: u64,
    focus: Option<(String, String)>,
}
impl<M> Clipboard<M> {
    pub(super) fn new(manager: Option<DataDeviceManagerState>) -> Self {
        Self {
            manager,
            device: None,
            source: None,
            pending: BTreeMap::new(),
            next: 0,
            timer: None,
            epoch: 0,
            focus: None,
        }
    }
}
impl<A: Application> State<A> {
    pub(super) fn clipboard_observe_focus(&mut self, qh: &QueueHandle<Self>) {
        if self.clipboard.manager.is_none() {
            return;
        }
        let focus = self.keyboard_surface.as_ref().and_then(|id| {
            self.surfaces.get(id).and_then(|native| {
                native
                    .interaction
                    .focus
                    .as_ref()
                    .map(|element| (id, element))
            })
        });
        if self
            .clipboard
            .focus
            .as_ref()
            .map(|(id, element)| (id, element))
            != focus
        {
            self.clipboard.focus = focus.map(|(id, element)| (id.clone(), element.clone()));
            self.clipboard_cancel(qh, false);
        }
    }
    fn clipboard_guard(&self, target: &ClipboardTarget) -> Option<Guard> {
        if self.authorize_unlock.is_some()
            || !self.app.clipboard_enabled()
            || self.keyboard_surface.as_deref() != Some(target.surface)
        {
            return None;
        }
        let native = self.surfaces.get(target.surface)?;
        if native.interaction.focus.as_deref() != Some(&target.element)
            || !self
                .app
                .surfaces()
                .iter()
                .any(|s| s.id == target.surface && s.visible && s.keyboard != api::Keyboard::None)
        {
            return None;
        }
        let tree = self.app.view(&ViewContext {
            surface: native.spec.id,
            width: native.width as f32,
            height: native.height as f32,
            now: self.epoch.elapsed().as_secs_f64(),
        });
        if !ordinary_field(&tree, &target.element) {
            return None;
        }
        Some(Guard {
            target: target.clone(),
            epoch: self.clipboard.epoch,
            surface: native.role.wl_surface().clone(),
        })
    }
    fn clipboard_complete(
        &mut self,
        request: ClipboardRequest<A::Message>,
        outcome: ClipboardOutcome,
        qh: &QueueHandle<Self>,
    ) {
        self.message(request.finish(outcome), qh);
    }
    pub(super) fn clipboard_request(
        &mut self,
        mut request: ClipboardRequest<A::Message>,
        qh: &QueueHandle<Self>,
    ) {
        self.clipboard_observe_focus(qh);
        let Some(guard) = self.clipboard_guard(&request.target) else {
            self.clipboard_complete(
                request,
                ClipboardOutcome::Error(ClipboardError::NoFocus),
                qh,
            );
            return;
        };
        let (Some(manager), Some(device)) = (&self.clipboard.manager, &self.clipboard.device)
        else {
            self.clipboard_complete(
                request,
                ClipboardOutcome::Error(ClipboardError::Unavailable),
                qh,
            );
            return;
        };
        match std::mem::replace(&mut request.action, ClipboardAction::Read) {
            ClipboardAction::Write(text) => {
                if text.len() > CLIPBOARD_TEXT_LIMIT {
                    self.clipboard_complete(
                        request,
                        ClipboardOutcome::Error(ClipboardError::TooLarge),
                        qh,
                    );
                    return;
                }
                let Some(serial) = self.input_serial else {
                    self.clipboard_complete(
                        request,
                        ClipboardOutcome::Error(ClipboardError::NoFocus),
                        qh,
                    );
                    return;
                };
                let source = manager.create_copy_paste_source(qh, MIME.iter().copied());
                source.set_selection(device, serial);
                self.clipboard.source = Some((source, Arc::from(text.into_bytes())));
                // This queues a protocol request; core Wayland provides no ownership ack.
                self.clipboard_complete(request, ClipboardOutcome::WritePosted, qh);
            }
            ClipboardAction::Read => {
                if self.clipboard.pending.len() >= MAX_TRANSFERS {
                    self.clipboard_complete(
                        request,
                        ClipboardOutcome::Error(ClipboardError::Busy),
                        qh,
                    );
                    return;
                }
                let Some(offer) = device.data().selection_offer() else {
                    self.clipboard_complete(
                        request,
                        ClipboardOutcome::Error(ClipboardError::Unsupported),
                        qh,
                    );
                    return;
                };
                let mime = offer.with_mime_types(|offered| {
                    MIME.iter()
                        .find(|mime| offered.iter().any(|m| m == **mime))
                        .copied()
                });
                let Some(mime) = mime else {
                    self.clipboard_complete(
                        request,
                        ClipboardOutcome::Error(ClipboardError::Unsupported),
                        qh,
                    );
                    return;
                };
                match offer.receive(mime.into()) {
                    Ok(pipe) => self.clipboard_transfer(
                        File::from(OwnedFd::from(pipe)),
                        io::Transfer::Read(vec![]),
                        Some(request),
                        Some(guard),
                        qh,
                    ),
                    Err(_) => self.clipboard_complete(
                        request,
                        ClipboardOutcome::Error(ClipboardError::Disconnected),
                        qh,
                    ),
                }
            }
        }
    }
    fn clipboard_transfer(
        &mut self,
        file: File,
        transfer: io::Transfer,
        request: Option<ClipboardRequest<A::Message>>,
        guard: Option<Guard>,
        qh: &QueueHandle<Self>,
    ) {
        if self.clipboard.pending.len() >= MAX_TRANSFERS || io::nonblocking(&file).is_err() {
            if let Some(request) = request {
                self.clipboard_complete(request, ClipboardOutcome::Error(ClipboardError::Busy), qh);
            }
            return;
        }
        let id = self.clipboard.next;
        self.clipboard.next = self.clipboard.next.wrapping_add(1);
        let event_qh = qh.clone();
        let interest = transfer.interest();
        let token = match self.handle.insert_source(
            Generic::new(file, interest, Mode::Level),
            move |_, file, state| {
                let Some(mut pending) = state.clipboard.pending.remove(&id) else {
                    return Ok(PostAction::Remove);
                };
                let live = pending.guard.as_ref().is_none_or(|guard| {
                    state
                        .clipboard_guard(&guard.target)
                        .is_some_and(|now| now.epoch == guard.epoch && now.surface == guard.surface)
                });
                let outcome = if live {
                    pending
                        .transfer
                        .step(file.as_ref(), pending.deadline, Instant::now())
                } else {
                    Some(ClipboardOutcome::Error(ClipboardError::Cancelled))
                };
                if let Some(outcome) = outcome {
                    if let Some(request) = pending.request {
                        state.clipboard_complete(request, outcome, &event_qh);
                    }
                    Ok(PostAction::Remove)
                } else {
                    state.clipboard.pending.insert(id, pending);
                    Ok(PostAction::Continue)
                }
            },
        ) {
            Ok(token) => token,
            Err(_) => {
                if let Some(request) = request {
                    self.clipboard_complete(
                        request,
                        ClipboardOutcome::Error(ClipboardError::Disconnected),
                        qh,
                    );
                }
                return;
            }
        };
        self.clipboard.pending.insert(
            id,
            Pending {
                token,
                deadline: Instant::now() + io::DEADLINE,
                transfer,
                request,
                guard,
            },
        );
        if self.clipboard.timer.is_none() {
            let event_qh = qh.clone();
            match self.handle.insert_source(
                Timer::from_duration(io::DEADLINE),
                move |_, _, state| {
                    let now = Instant::now();
                    let expired: Vec<_> = state
                        .clipboard
                        .pending
                        .iter()
                        .filter(|(_, p)| now >= p.deadline)
                        .map(|(id, _)| *id)
                        .collect();
                    for id in expired {
                        if let Some(p) = state.clipboard.pending.remove(&id) {
                            state.handle.remove(p.token);
                            if let Some(request) = p.request {
                                state.clipboard_complete(
                                    request,
                                    ClipboardOutcome::Error(ClipboardError::Timeout),
                                    &event_qh,
                                );
                            }
                        }
                    }
                    if let Some(next) = state.clipboard.pending.values().map(|p| p.deadline).min() {
                        TimeoutAction::ToInstant(next)
                    } else {
                        state.clipboard.timer = None;
                        TimeoutAction::Drop
                    }
                },
            ) {
                Ok(token) => self.clipboard.timer = Some(token),
                Err(_) => self.clipboard_cancel(qh, true),
            }
        }
    }
    pub(super) fn clipboard_cancel(&mut self, qh: &QueueHandle<Self>, all: bool) {
        self.clipboard.epoch = self.clipboard.epoch.wrapping_add(1);
        let ids: Vec<_> = self
            .clipboard
            .pending
            .iter()
            .filter(|(_, p)| all || p.request.is_some())
            .map(|(id, _)| *id)
            .collect();
        let mut requests = vec![];
        for id in ids {
            if let Some(p) = self.clipboard.pending.remove(&id) {
                self.handle.remove(p.token);
                if let Some(request) = p.request {
                    requests.push(request);
                }
            }
        }
        if self.clipboard.pending.is_empty()
            && let Some(timer) = self.clipboard.timer.take()
        {
            self.handle.remove(timer);
        }
        for request in requests {
            self.clipboard_complete(
                request,
                ClipboardOutcome::Error(ClipboardError::Cancelled),
                qh,
            );
        }
    }
    pub(super) fn clipboard_seat(&mut self, seat: &wl_seat::WlSeat, qh: &QueueHandle<Self>) {
        if let Some(manager) = &self.clipboard.manager {
            self.clipboard.device = Some(manager.get_data_device(qh, seat));
        }
    }
    pub(super) fn clipboard_drop(&mut self) {
        for (_, p) in std::mem::take(&mut self.clipboard.pending) {
            self.handle.remove(p.token);
        }
        if let Some(timer) = self.clipboard.timer.take() {
            self.handle.remove(timer);
        }
        self.clipboard.source = None;
        self.clipboard.device = None;
    }
}
fn ordinary_field<M>(tree: &api::Element<M>, id: &str) -> bool {
    if tree.style.opacity <= 0. {
        return false;
    }
    if tree.id == id {
        return matches!(tree.kind, api::Kind::TextInput { .. })
            && tree.text_edit.is_some()
            && tree.style.opacity > 0.;
    }
    tree.children.iter().any(|child| ordinary_field(child, id))
}
impl<A: Application> DataDeviceHandler for State<A> {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        device: &WlDataDevice,
        _: f64,
        _: f64,
        _: &wl_surface::WlSurface,
    ) {
        // Binding the data device also exposes DnD. This adapter accepts only
        // ordinary clipboard selection, so reject every drag explicitly.
        if let Some(offer) = device
            .data::<smithay_client_toolkit::data_device_manager::data_device::DataDeviceData>()
            .and_then(|data| data.drag_offer())
        {
            offer.accept_mime_type(offer.serial, None);
            offer.set_actions(DndAction::empty(), DndAction::empty());
        }
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {}
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, device: &WlDataDevice) {
        if let Some(offer) = device
            .data::<smithay_client_toolkit::data_device_manager::data_device::DataDeviceData>()
            .and_then(|data| data.drag_offer())
        {
            offer.destroy();
        }
    }
}
impl<A: Application> DataOfferHandler for State<A> {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}
impl<A: Application> DataSourceHandler for State<A> {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        _: Option<String>,
    ) {
    }
    fn send_request(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        source: &WlDataSource,
        mime: String,
        fd: WritePipe,
    ) {
        if !MIME.contains(&mime.as_str()) {
            return;
        }
        let Some(bytes) = self
            .clipboard
            .source
            .as_ref()
            .filter(|(owned, _)| owned.inner() == source)
            .map(|(_, bytes)| bytes.clone())
        else {
            return;
        };
        self.clipboard_transfer(
            File::from(OwnedFd::from(fd)),
            io::Transfer::Write { bytes, offset: 0 },
            None,
            None,
            qh,
        );
    }
    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        if self
            .clipboard
            .source
            .as_ref()
            .is_some_and(|(owned, _)| owned.inner() == source)
        {
            self.clipboard.source = None;
        }
        // An already accepted send owns bounded bytes and may finish even
        // after selection replacement. Closing it here could paste a prefix.
    }
    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_target_requires_a_live_ordinary_control_and_callback() {
        let field =
            api::Element::text_input("value", api::TextSelection::default(), "", |edit| edit)
                .id("field");
        assert!(ordinary_field(&field, "field"));
        assert!(!ordinary_field(&field, "other"));
        let hidden = api::Element::column(vec![field.clone()]).opacity(0.);
        assert!(!ordinary_field(&hidden, "field"));
        let masked = api::Element::masked_input(5, "", |_| api::TextEdit::Clear).id("field");
        assert!(!ordinary_field(&masked, "field"));
        let mut unbound = field;
        unbound.text_edit = None;
        assert!(!ordinary_field(&unbound, "field"));
    }
}
