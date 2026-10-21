//! text-input-v3 adapter. Only current ordinary fields grant input authority.
use super::*;
use wayland_client::{Dispatch, globals::GlobalList};
use wayland_protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    zwp_text_input_v3::{self as protocol, ZwpTextInputV3},
};
#[derive(Clone, PartialEq)]
struct Target {
    address: api::SurfaceAddress,
    element: String,
    surface: wl_surface::WlSurface,
    value: Arc<str>,
    selection: api::TextSelection,
    cursor: api::Rect,
    multiline: bool,
}
#[derive(Default)]
struct Pending {
    preedit: Option<(String, i32, i32)>,
    commit: Option<String>,
    delete: (u32, u32),
    invalid: bool,
}
struct CompletedBatch {
    preedit: Option<(String, i32, i32)>,
    edit: Option<api::TextEdit>,
    update_protocol: bool,
}
impl Pending {
    fn complete(
        self,
        serial: u32,
        current: u32,
        available_before: usize,
        available_after: usize,
    ) -> Option<CompletedBatch> {
        if self.invalid
            || self.delete.0 as usize > available_before
            || self.delete.1 as usize > available_after
        {
            return None;
        }
        let edit =
            (self.commit.is_some() || self.delete != (0, 0)).then(|| api::TextEdit::InputMethod {
                before: self.delete.0 as usize,
                after: self.delete.1 as usize,
                commit: self.commit.unwrap_or_default(),
            });
        Some(CompletedBatch {
            preedit: self.preedit,
            edit,
            update_protocol: serial == current,
        })
    }
}
pub(super) struct InputMethod {
    manager: Option<ZwpTextInputManagerV3>,
    input: Option<ZwpTextInputV3>,
    entered: Option<wl_surface::WlSurface>,
    target: Option<Target>,
    serial: u32,
    pending: Pending,
    force: bool,
    deferred: bool,
    external_reset: bool,
}
impl InputMethod {
    pub fn bind<A: Application>(
        globals: &GlobalList,
        qh: &QueueHandle<State<A>>,
        secure: bool,
    ) -> Self {
        Self {
            manager: if secure {
                None
            } else {
                globals.bind(qh, 1..=1, ()).ok()
            },
            input: None,
            entered: None,
            target: None,
            serial: 0,
            pending: Pending::default(),
            force: false,
            deferred: false,
            external_reset: false,
        }
    }
    pub fn seat<D: Dispatch<ZwpTextInputV3, ()> + 'static>(
        &mut self,
        seat: &wl_seat::WlSeat,
        qh: &QueueHandle<D>,
    ) {
        if let Some(manager) = &self.manager {
            self.input = Some(manager.get_text_input(seat, qh, ()));
            self.serial = 0;
        }
    }
    fn commit(&mut self) {
        if let Some(input) = &self.input {
            input.commit();
            self.serial = self.serial.wrapping_add(1);
        }
    }
    fn disable(&mut self) {
        if self.target.take().is_some() {
            if let Some(input) = &self.input {
                input.disable();
            }
            self.commit();
        }
        self.pending = Pending::default();
    }
    pub fn shutdown(&mut self) {
        self.remove_seat(|_| {});
        if let Some(manager) = self.manager.take() {
            manager.destroy();
        }
    }
    fn remove_seat(&mut self, clear_preedit: impl FnOnce(&Target)) {
        // Presentation belongs to the native interaction, not this protocol
        // object. Retire it while the old target still identifies that native.
        if let Some(target) = &self.target {
            clear_preedit(target);
        }
        self.disable();
        if let Some(input) = self.input.take() {
            input.destroy();
        }
        self.entered = None;
        self.serial = 0;
        self.force = false;
        self.deferred = false;
        self.external_reset = false;
    }
    fn accepts(&self, proxy: &ZwpTextInputV3) -> bool {
        self.input.as_ref() == Some(proxy)
    }
}
impl Drop for InputMethod {
    fn drop(&mut self) {
        self.shutdown();
    }
}
/// Protocol's bounded UTF-8 window. Selection too wide for one message is omitted.
fn surrounding(value: &str, selection: api::TextSelection) -> (String, i32, i32) {
    let selection = selection.reconciled(value);
    let low = selection.caret().min(selection.anchor());
    let high = selection.caret().max(selection.anchor());
    if high - low > 4000 {
        return (String::new(), 0, 0);
    }
    let spare = 4000 - (high - low);
    let mut start = low.saturating_sub(spare / 2);
    while !value.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (start + 4000).min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (
        value[start..end].to_owned(),
        (selection.caret() - start) as i32,
        (selection.anchor() - start) as i32,
    )
}
fn retire_preedit<M: Clone>(
    interaction: &mut lucent_ui::Interaction<M>,
    scene: &lucent_ui::Scene<M>,
    demand: &mut FrameDemand,
) {
    if interaction.set_preedit(scene, None) {
        demand.invalidate();
    }
}
impl<A: Application> State<A> {
    pub(super) fn input_method_remove_seat(&mut self, qh: &QueueHandle<Self>) {
        let surfaces = &mut self.surfaces;
        let mut retired = None;
        self.input_method.remove_seat(|target| {
            if let Some(native) = surfaces.get_mut(&target.address) {
                retire_preedit(&mut native.interaction, &native.scene, &mut native.demand);
                retired = Some(target.address.clone());
            }
        });
        // Rebuild CPU hit/caret geometry even while a previous GPU submission
        // waits for its frame callback. A recreated seat cannot see ghost preedit.
        if let Some(address) = retired {
            for message in self.refresh_input_scene(&address, false, InputRefresh::Current) {
                self.message(message, qh);
            }
        }
    }
    fn input_method_target(&self) -> Option<Target> {
        let address = self.keyboard_surface.as_ref()?;
        let native = self.surfaces.get(address)?;
        let surface = native.role.wl_surface();
        if self.input_method.entered.as_ref() != Some(surface)
            || self.authorize_unlock.is_some()
            || !self.app.input_method_enabled()
        {
            return None;
        }
        let element = native.interaction.focus.as_ref()?;
        let hit = native
            .scene
            .hits
            .iter()
            .find(|hit| &hit.id == element && hit.text_edit.is_some())?;
        let text = hit.text.as_ref()?;
        if !self.app.surfaces().iter().any(|spec| {
            spec.address() == *address && spec.visible && spec.keyboard != api::Keyboard::None
        }) {
            return None;
        }
        Some(Target {
            address: address.clone(),
            element: element.clone(),
            surface: surface.clone(),
            value: text.value.clone(),
            selection: text.selection,
            cursor: text.caret_rect(),
            multiline: text.is_multiline(),
        })
    }
    pub(super) fn input_method_observe(&mut self) {
        if self.input_method.input.is_none() {
            return;
        }
        let target = self.input_method_target();
        if self.input_method.deferred {
            if self
                .input_method
                .target
                .as_ref()
                .zip(target.as_ref())
                .is_some_and(|(a, b)| {
                    a.address == b.address && a.element == b.element && a.surface == b.surface
                })
            {
                return;
            }
            self.input_method.deferred = false;
        }
        if target == self.input_method.target && !self.input_method.force {
            return;
        }
        let from_input_method = self.input_method.force;
        self.input_method.force = false;
        let lifetime_same = self
            .input_method
            .target
            .as_ref()
            .zip(target.as_ref())
            .is_some_and(|(a, b)| {
                a.address == b.address && a.element == b.element && a.surface == b.surface
            });
        if !lifetime_same {
            if let Some(old) = &self.input_method.target
                && let Some(native) = self.surfaces.get_mut(&old.address)
                && native.interaction.set_preedit(&native.scene, None)
            {
                native.demand.invalidate();
            }
            self.input_method.disable();
        } else if self
            .input_method
            .target
            .as_ref()
            .zip(target.as_ref())
            .is_some_and(|(a, b)| a.value != b.value || a.selection != b.selection)
        {
            if let Some(native) = target
                .as_ref()
                .and_then(|target| self.surfaces.get_mut(&target.address))
                && native.interaction.set_preedit(&native.scene, None)
            {
                native.demand.invalidate();
            }
            self.input_method.pending = Pending::default();
        }
        let Some(target) = target else {
            return;
        };
        let input = self.input_method.input.as_ref().expect("bound input");
        if !lifetime_same {
            input.enable();
        }
        let (text, cursor, anchor) = surrounding(&target.value, target.selection);
        input.set_surrounding_text(text, cursor, anchor);
        let externally_changed = self.input_method.external_reset
            || !from_input_method
                && self.input_method.target.as_ref().is_none_or(|old| {
                    old.value != target.value || old.selection != target.selection
                });
        self.input_method.external_reset = false;
        input.set_text_change_cause(if externally_changed {
            protocol::ChangeCause::Other
        } else {
            protocol::ChangeCause::InputMethod
        });
        input.set_content_type(
            if target.multiline {
                protocol::ContentHint::Multiline
            } else {
                protocol::ContentHint::None
            },
            protocol::ContentPurpose::Normal,
        );
        let rect = target.cursor;
        input.set_cursor_rectangle(
            rect.x.floor() as i32,
            rect.y.floor() as i32,
            ((rect.x + rect.w).ceil() - rect.x.floor()).max(1.) as i32,
            ((rect.y + rect.h).ceil() - rect.y.floor()).max(1.) as i32,
        );
        self.input_method.target = Some(target);
        self.input_method.commit();
    }
    pub(super) fn input_method_cancel_preedit(&mut self, qh: &QueueHandle<Self>) {
        let Some(target) = self.input_method.target.clone() else {
            return;
        };
        let changed = self
            .surfaces
            .get_mut(&target.address)
            .is_some_and(|native| {
                let changed = native.interaction.set_preedit(&native.scene, None);
                if changed {
                    native.demand.invalidate();
                }
                changed
            });
        if !changed {
            return;
        }
        self.input_method.pending = Pending::default();
        self.input_method.deferred = false;
        self.input_method.external_reset = true;
        self.input_method.force = true;
        for message in self.refresh_input_scene(&target.address, false, InputRefresh::Current) {
            self.message(message, qh);
        }
        self.input_method_observe();
    }
    fn input_method_done(&mut self, serial: u32, qh: &QueueHandle<Self>) {
        let pending = std::mem::take(&mut self.input_method.pending);
        if pending.invalid {
            return;
        }
        let matches_serial = serial == self.input_method.serial;
        self.input_method.deferred = true;
        let Some(target) = self.input_method.target.clone() else {
            self.input_method.deferred = !matches_serial;
            return;
        };
        // Resolve current CPU geometry before accepting a delayed native batch.
        for message in self.refresh_input_scene(&target.address, false, InputRefresh::Current) {
            self.message(message, qh);
        }
        if !self.input_method_target().as_ref().is_some_and(|current| {
            current.address == target.address
                && current.element == target.element
                && current.surface == target.surface
        }) {
            self.input_method.deferred = false;
            self.input_method_observe();
            return;
        }
        let window = surrounding(&target.value, target.selection);
        // Never let a compositor edit outside the surrounding window it saw.
        let Some(completed) = pending.complete(
            serial,
            self.input_method.serial,
            window.1.min(window.2) as usize,
            window.0.len() - window.1.max(window.2) as usize,
        ) else {
            self.input_method.deferred = !matches_serial;
            return;
        };
        let message = completed.edit.and_then(|edit| {
            self.surfaces
                .get(&target.address)
                .and_then(|native| {
                    native
                        .scene
                        .hits
                        .iter()
                        .find(|hit| hit.id == target.element)
                })
                .and_then(|hit| hit.text_edit.as_ref())
                .map(|callback| callback(edit))
        });
        if let Some(native) = self.surfaces.get_mut(&target.address)
            && native.interaction.set_preedit(&native.scene, None)
        {
            native.demand.invalidate();
        }
        if let Some(message) = message {
            self.message(message, qh);
        }
        // Updating the client can close/recreate/refocus the field. Preedit never crosses it.
        for message in self.refresh_input_scene(&target.address, false, InputRefresh::Current) {
            self.message(message, qh);
        }
        self.input_method.force = true;
        self.input_method.deferred = !completed.update_protocol;
        self.input_method_observe();
        let current = self.input_method_target();
        if current.as_ref().is_some_and(|current| {
            current.address == target.address
                && current.element == target.element
                && current.surface == target.surface
        }) && let Some(native) = self.surfaces.get_mut(&target.address)
            && native
                .interaction
                .set_preedit(&native.scene, completed.preedit)
        {
            native.demand.invalidate();
        }
    }
}
impl<A: Application> Dispatch<ZwpTextInputV3, ()> for State<A> {
    fn event(
        state: &mut Self,
        proxy: &ZwpTextInputV3,
        event: protocol::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if !state.input_method.accepts(proxy) {
            return;
        }
        match event {
            protocol::Event::Enter { surface } => {
                state.input_method.entered = Some(surface);
                state.input_method_observe();
            }
            protocol::Event::Leave { surface } => {
                if state.input_method.entered.as_ref() == Some(&surface) {
                    state.input_method.entered = None;
                    state.input_method.deferred = false;
                    state.input_method_observe();
                }
            }
            protocol::Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                let text = text.unwrap_or_default();
                if text.len() > 4096 {
                    state.input_method.pending.invalid = true;
                } else {
                    state.input_method.pending.preedit = Some((text, cursor_begin, cursor_end));
                }
            }
            protocol::Event::CommitString { text } => {
                let text = text.unwrap_or_default();
                if text.len() > 64 * 1024 {
                    state.input_method.pending.invalid = true;
                } else {
                    state.input_method.pending.commit = Some(text);
                }
            }
            protocol::Event::DeleteSurroundingText {
                before_length,
                after_length,
            } => state.input_method.pending.delete = (before_length, after_length),
            protocol::Event::Done { serial } => state.input_method_done(serial, qh),
            _ => {}
        }
    }
}
wayland_client::delegate_noop!(@<A:Application> State<A>:ZwpTextInputManagerV3);
#[cfg(test)]
mod tests {
    use super::*;
    struct Peer;
    wayland_client::delegate_noop!(Peer: ignore wayland_client::protocol::wl_registry::WlRegistry);
    wayland_client::delegate_noop!(Peer: ignore wl_seat::WlSeat);
    wayland_client::delegate_noop!(Peer: ignore wayland_client::protocol::wl_compositor::WlCompositor);
    wayland_client::delegate_noop!(Peer: ignore wl_surface::WlSurface);
    wayland_client::delegate_noop!(Peer: ignore ZwpTextInputManagerV3);
    wayland_client::delegate_noop!(Peer: ignore ZwpTextInputV3);

    #[test]
    fn seat_retirement_clears_visible_preedit_before_dropping_target_and_denies_old_proxy() {
        // Client-owned local socket only: no compositor, GPU, host display or IME.
        let (socket, _server) = std::os::unix::net::UnixStream::pair().unwrap();
        let connection = Connection::from_socket(socket).unwrap();
        let queue = connection.new_event_queue::<Peer>();
        let qh = queue.handle();
        let registry = connection.display().get_registry(&qh, ());
        let manager = registry.bind::<ZwpTextInputManagerV3, _, _>(1, 1, &qh, ());
        let seat = registry.bind::<wl_seat::WlSeat, _, _>(2, 1, &qh, ());
        let compositor = registry
            .bind::<wayland_client::protocol::wl_compositor::WlCompositor, _, _>(3, 1, &qh, ());
        let surface = compositor.create_surface(&qh, ());
        let layout = lucent_ui::Layout::new(Arc::new(vec![
            lucent_ui::text::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf").as_slice(),
                Default::default(),
            )
            .unwrap(),
        ]));
        for multiline in [false, true] {
            let value = "ab";
            let selection = api::TextSelection::at_end(value);
            let tree = if multiline {
                api::Element::text_area(
                    value,
                    selection,
                    "",
                    api::ScrollSpec::new(0., 24.).unwrap(),
                    |edit| edit,
                    |_| api::TextEdit::Clear,
                )
            } else {
                api::Element::text_input(value, selection, "", |edit| edit)
            }
            .id("field")
            .autofocus()
            .size(320., 100.)
            .font(20.);
            let mut interaction = lucent_ui::Interaction::default();
            let scene = layout.build(&tree, 320., 100., &interaction, 0.);
            interaction.synchronize(&scene);
            let scene = layout.build(&tree, 320., 100., &interaction, 0.);
            let original_cursor = scene.hits[0].text.as_ref().unwrap().caret_rect();
            assert!(interaction.set_preedit(&scene, Some(("候補".into(), 6, 6))));
            let projected = layout.build(&tree, 320., 100., &interaction, 0.);
            assert!(projected.paint.iter().any(|paint| matches!(paint,
                lucent_ui::Paint::Text { text, .. } if text == "ab候補"
            )));
            let mut method = InputMethod {
                manager: Some(manager.clone()),
                input: None,
                entered: None,
                target: None,
                serial: 0,
                pending: Pending::default(),
                force: false,
                deferred: false,
                external_reset: false,
            };
            method.seat(&seat, &qh);
            let old_proxy = method.input.as_ref().unwrap().clone();
            method.entered = Some(surface.clone());
            method.target = Some(Target {
                address: api::SurfaceAddress::new("editor"),
                element: "field".into(),
                surface: surface.clone(),
                value: value.into(),
                selection,
                cursor: original_cursor,
                multiline,
            });
            method.pending.preedit = Some(("stale".into(), 5, 5));
            method.pending.commit = Some("stale".into());
            method.pending.delete = (1, 1);
            method.serial = 9;
            method.force = true;
            method.deferred = true;
            method.external_reset = true;
            let mut demand = FrameDemand::default();
            let mut cleared = false;
            method.remove_seat(|target| {
                assert_eq!(target.element, "field");
                retire_preedit(&mut interaction, &projected, &mut demand);
                cleared = true;
            });
            assert!(
                cleared && demand.begin(),
                "retirement must request a repaint"
            );
            assert!(method.input.is_none() && method.target.is_none() && method.entered.is_none());
            assert!(!method.accepts(&old_proxy));
            assert_eq!(method.serial, 0);
            assert!(!method.force && !method.deferred && !method.external_reset);
            assert!(
                method.pending.preedit.is_none()
                    && method.pending.commit.is_none()
                    && method.pending.delete == (0, 0)
            );
            let retired = layout.build(&tree, 320., 100., &interaction, 0.);
            interaction.synchronize(&retired);
            assert_eq!(interaction.focus.as_deref(), Some("field"));
            assert_eq!(retired.hits[0].text.as_ref().unwrap().value.as_ref(), value);
            assert_eq!(
                retired.hits[0].text.as_ref().unwrap().caret_rect(),
                original_cursor
            );
            assert!(retired.paint.iter().any(|paint| matches!(paint,
                lucent_ui::Paint::Text { text, .. } if text == value
            )));
            assert!(!retired.paint.iter().any(|paint| matches!(paint,
                lucent_ui::Paint::Text { text, .. } if text.contains("候補")
            )));
            method.seat(&seat, &qh);
            assert!(method.accepts(method.input.as_ref().unwrap()));
            assert!(
                !method.accepts(&old_proxy),
                "old Done/Commit/Preedit/Leave cannot reach the replacement seat"
            );
            assert!(method.target.is_none() && method.entered.is_none());
            method.remove_seat(|_| {});
            // This fixture shares one manager across both independent field cases.
            method.manager = None;
        }
    }

    #[test]
    fn stale_done_applies_the_atomic_edit_but_defers_protocol_state_updates() {
        for (serial, expected_updates) in [(4, false), (5, true)] {
            let mut pending = Pending {
                preedit: Some(("候補".into(), 6, 6)),
                commit: Some("語".into()),
                delete: (1, 0),
                invalid: false,
            };
            let completed = std::mem::take(&mut pending)
                .complete(serial, 5, 1, 0)
                .unwrap();
            assert_eq!(completed.update_protocol, expected_updates);
            let mut text = "ab".to_owned();
            let mut selection = api::TextSelection::at_end(&text);
            selection
                .apply(&mut text, completed.edit.unwrap(), 32)
                .unwrap();
            assert_eq!(text, "a語");
            assert_eq!(completed.preedit.as_ref().unwrap().0, "候補");
            assert!(
                pending.commit.is_none() && pending.preedit.is_none() && pending.delete == (0, 0)
            );
        }
    }
    #[test]
    fn invalid_or_out_of_window_batches_cannot_apply_partial_commit_or_preedit() {
        for (invalid, delete) in [(true, (0, 0)), (false, (4, 0)), (false, (0, 4))] {
            let pending = Pending {
                preedit: Some(("候補".into(), 6, 6)),
                commit: Some("語".into()),
                delete,
                invalid,
            };
            assert!(pending.complete(1, 1, 3, 3).is_none());
        }
    }
    #[test]
    fn utf8_surrounding_is_bounded_and_anchors_remain_byte_offsets() {
        let value = "日".repeat(4000);
        let mut selected = api::TextSelection::default();
        let mut owned = value.clone();
        selected
            .apply(
                &mut owned,
                api::TextEdit::Select {
                    anchor: 6000,
                    caret: 6006,
                },
                usize::MAX,
            )
            .unwrap();
        let (text, cursor, anchor) = surrounding(&value, selected);
        assert!(text.len() <= 4000);
        assert!(text.is_char_boundary(cursor as usize));
        assert!(text.is_char_boundary(anchor as usize));
        assert_eq!(cursor - anchor, 6);
        selected
            .apply(&mut owned, api::TextEdit::SelectAll, usize::MAX)
            .unwrap();
        assert_eq!(surrounding(&value, selected), (String::new(), 0, 0));
    }
}
