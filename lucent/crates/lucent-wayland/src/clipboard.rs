//! Core Wayland text selection. No desktop identities, shell commands or history.
use crate::State;
use calloop::{
    Mode, PostAction, RegistrationToken,
    generic::Generic,
    timer::{TimeoutAction, Timer},
};
use lucent_api::{
    self as api, Application, CLIPBOARD_TEXT_LIMIT, ClipboardAction, ClipboardError,
    ClipboardOutcome, ClipboardRequest, ClipboardTarget,
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
    focus: Option<(lucent_api::SurfaceAddress, String)>,
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
        self.input_method_observe();
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
            || self.keyboard_surface.as_ref() != Some(&target.address())
        {
            return None;
        }
        let native = self.surfaces.get(&target.address())?;
        if native.interaction.focus.as_deref() != Some(&target.element)
            || !self.app.surfaces().iter().any(|s| {
                s.address() == target.address() && s.visible && s.keyboard != api::Keyboard::None
            })
        {
            return None;
        }
        // Request admission and transfer callbacks refresh this CPU scene
        // before checking authority. Its hits include admitted popover controls
        // and exclude passive tooltip contents and rejected declarations.
        if !ordinary_field(&native.scene, &target.element) {
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
                if let Some(guard) = &pending.guard {
                    let address = guard.target.address();
                    if state.id_for(&guard.surface).as_ref() == Some(&address) {
                        for message in state.refresh_input_scene(&address, false, true) {
                            if state.id_for(&guard.surface).as_ref() != Some(&address) {
                                break;
                            }
                            state.message(message, &event_qh);
                        }
                        state.clipboard_observe_focus(&event_qh);
                    }
                }
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
fn ordinary_field<M>(scene: &lucent_ui::Scene<M>, id: &str) -> bool {
    let mut matching = scene.hits.iter().filter(|hit| hit.id == id);
    matching
        .next()
        .is_some_and(|hit| hit.text.is_some() && hit.text_edit.is_some())
        && matching.next().is_none()
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
    fn layout() -> lucent_ui::Layout {
        lucent_ui::Layout::new(Arc::new(vec![
            lucent_ui::text::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
                Default::default(),
            )
            .unwrap(),
        ]))
    }
    fn scene(
        layout: &lucent_ui::Layout,
        tree: &api::Element<api::TextEdit>,
    ) -> lucent_ui::Scene<api::TextEdit> {
        layout.build(tree, 240., 160., &lucent_ui::Interaction::default(), 0.)
    }
    #[test]
    fn clipboard_target_requires_a_live_ordinary_control_and_callback() {
        let layout = layout();
        let field =
            api::Element::text_input("value", api::TextSelection::default(), "", |edit| edit)
                .id("field");
        assert!(ordinary_field(&scene(&layout, &field), "field"));
        assert!(!ordinary_field(&scene(&layout, &field), "other"));
        let hidden = api::Element::column(vec![field.clone()]).opacity(0.);
        assert!(!ordinary_field(&scene(&layout, &hidden), "field"));
        let masked = api::Element::masked_input(5, "", |_| api::TextEdit::Clear).id("field");
        assert!(!ordinary_field(&scene(&layout, &masked), "field"));
        assert!(ordinary_field(
            &scene(&layout, &field.clone().input_only()),
            "field"
        ));
        let duplicate = api::Element::stack(vec![field.clone(), field.clone()]);
        assert!(!ordinary_field(&scene(&layout, &duplicate), "field"));
        let mut unbound = field;
        unbound.text_edit = None;
        assert!(!ordinary_field(&scene(&layout, &unbound), "field"));
    }
    #[test]
    fn clipboard_fields_follow_admitted_scopes_and_exclude_tooltip_content() {
        let layout = layout();
        let placement = api::AnchoredSpec {
            side: api::OverlaySide::Below,
            align: api::OverlayAlign::Start,
            gap: 8.,
            inset: 8.,
        };
        let field =
            api::Element::text_input("Popup value", api::TextSelection::default(), "", |edit| {
                edit
            })
            .size(140., 40.)
            .id("field");
        let anchor = api::Element::empty()
            .size(20., 20.)
            .at(20., 10.)
            .on_click(api::TextEdit::Clear)
            .id("anchor");
        let popup = api::Element::stack(vec![anchor.clone()]).popover(api::Popover {
            id: "popup".into(),
            anchor: "anchor".into(),
            placement,
            content: Box::new(field.clone()),
            dismiss: api::TextEdit::Clear,
        });
        let mut input = lucent_ui::Interaction::default();
        let prepared = layout.build(&popup, 240., 160., &input, 0.);
        input.synchronize(&prepared);
        assert_eq!(input.focus.as_deref(), Some("field"));
        assert!(ordinary_field(&prepared, "field"));
        let mut rejected = popup.clone();
        rejected.popovers[0].anchor = "missing".into();
        assert!(!ordinary_field(&scene(&layout, &rejected), "field"));
        let passive = api::Element::stack(vec![anchor.tooltip(api::Tooltip {
            content: Box::new(field),
            placement,
            max_width: 160.,
            delay: 0.,
            transition: api::Transition::default(),
        })]);
        let mut input = lucent_ui::Interaction::default();
        let initial = layout.build(&passive, 240., 160., &input, 0.);
        input.motion(&initial, 30., 20., 0.);
        let prepared = layout.build(&passive, 240., 160., &input, 1.);
        assert!(!ordinary_field(&prepared, "field"));
    }
}
