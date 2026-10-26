//! Read-only observations of this client's selected `wl_keyboard` focus.
//!
//! Logical addresses can name successive native surfaces. Keep the actual Enter
//! proxy and resolve it against current natives only when inspecting. This data
//! never participates in routing, activation, rendering or focus decisions.

use lucent_api as api;
use serde_json::{Value, json};
use wayland_client::{Proxy, protocol::wl_surface::WlSurface};

#[derive(Clone, Copy)]
struct Count(Option<u64>);

impl Default for Count {
    fn default() -> Self {
        Self(Some(0))
    }
}

impl Count {
    fn advance(&mut self) {
        self.0 = self.0.and_then(|count| count.checked_add(1));
    }
}

#[derive(Default)]
pub(super) struct Observations {
    surface: Option<WlSurface>,
    sequence: Count,
    enters: Count,
    leaves: Count,
    ignored_keyboards: Count,
    capability_removals: Count,
}

pub(super) struct CurrentNative<'a> {
    pub surface: &'a WlSurface,
    pub address: &'a api::SurfaceAddress,
    pub requested: api::Keyboard,
    pub committed: api::Keyboard,
}

pub(super) struct Inspection<'a> {
    pub current_address: Option<&'a api::SurfaceAddress>,
    pub json: Value,
}

pub(super) fn policy_name(policy: api::Keyboard) -> &'static str {
    match policy {
        api::Keyboard::None => "none",
        api::Keyboard::OnDemand => "on-demand",
        api::Keyboard::Exclusive => "exclusive",
    }
}

impl Observations {
    pub fn enter(&mut self, current_keyboard: bool, surface: &WlSurface) {
        if !current_keyboard {
            self.ignored_keyboards.advance();
            return;
        }
        self.sequence.advance();
        self.enters.advance();
        // A single protocol proxy holds no Native/renderer ownership. Unknown
        // and retiring surfaces remain unresolved rather than naming a newer
        // native object which happens to use the same logical address.
        self.surface = Some(surface.clone());
    }

    pub fn leave(&mut self, current_keyboard: bool, surface: &WlSurface) {
        if !current_keyboard {
            self.ignored_keyboards.advance();
            return;
        }
        self.sequence.advance();
        self.leaves.advance();
        if self.surface.as_ref() == Some(surface) {
            self.surface = None;
        }
    }

    pub fn keyboard_removed(&mut self) {
        self.capability_removals.advance();
        self.surface = None;
    }

    pub fn inspect<'a>(
        &self,
        keyboard_available: bool,
        current: impl IntoIterator<Item = CurrentNative<'a>>,
    ) -> Inspection<'a> {
        let observed_alive = self.surface.as_ref().map(|surface| surface.is_alive());
        let matched = if keyboard_available && observed_alive == Some(true) {
            current.into_iter().find(|native| {
                self.surface.as_ref() == Some(native.surface) && native.surface.is_alive()
            })
        } else {
            None
        };
        let resolution = if !keyboard_available {
            "keyboard-unavailable"
        } else if self.surface.is_none() {
            "no-client-focus-observed"
        } else if observed_alive == Some(false) {
            "retired-proxy"
        } else if matched.is_some() {
            "current-native"
        } else {
            "unresolved-native"
        };
        let current_address = matched.as_ref().map(|native| native.address);
        let json = json!({
            "scope":"This client's selected wl_keyboard Enter/Leave observations as dispatched. An absent or unresolved current_native does not establish that the compositor has no global keyboard owner.",
            "keyboard_available":keyboard_available,
            "observed_surface_present":self.surface.is_some(),
            "observed_proxy_alive":observed_alive,
            "resolution":resolution,
            "current_native":matched.map(|native| json!({
                "id":native.address.to_string(),
                "role":native.address.role,
                "instance":native.address.instance.as_ref().map(api::SurfaceInstance::as_str),
                "requested_keyboard":policy_name(native.requested),
                "committed_keyboard":policy_name(native.committed),
            })),
            "event_sequence":self.sequence.0,
            "enter_count":self.enters.0,
            "leave_count":self.leaves.0,
            "ignored_other_keyboard_events":self.ignored_keyboards.0,
            "capability_removal_count":self.capability_removals.0,
            "counter_scope":"Sequence counts selected-keyboard Enter and Leave deliveries; ignored keyboards and capability removals have separate counts. A null count means overflow, never wraparound.",
        });
        Inspection {
            current_address,
            json,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, os::unix::net::UnixStream};
    use wayland_client::{
        Connection, EventQueue,
        protocol::{wl_compositor::WlCompositor, wl_registry::WlRegistry},
    };

    struct Peer;
    wayland_client::delegate_noop!(Peer: ignore WlRegistry);
    wayland_client::delegate_noop!(Peer: ignore WlCompositor);
    wayland_client::delegate_noop!(Peer: ignore WlSurface);

    fn socket() -> (Connection, EventQueue<Peer>, UnixStream, WlCompositor) {
        let (client, server) = UnixStream::pair().unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let queue = connection.new_event_queue::<Peer>();
        let registry = connection.display().get_registry(&queue.handle(), ());
        let compositor = registry.bind::<WlCompositor, _, _>(1, 1, &queue.handle(), ());
        (connection, queue, server, compositor)
    }

    fn native<'a>(surface: &'a WlSurface, address: &'a api::SurfaceAddress) -> CurrentNative<'a> {
        CurrentNative {
            surface,
            address,
            requested: api::Keyboard::None,
            committed: api::Keyboard::Exclusive,
        }
    }

    #[test]
    fn keyboard_inspection_resolves_exact_proxy_and_distinguishes_committed_policy() {
        let (_connection, queue, _server, compositor) = socket();
        let surface = compositor.create_surface(&queue.handle(), ());
        let address =
            api::SurfaceAddress::new("panel").instance(api::SurfaceInstance::new("one").unwrap());
        let mut observed = Observations::default();
        observed.enter(true, &surface);
        let snapshot = observed.inspect(true, [native(&surface, &address)]);
        assert_eq!(snapshot.current_address, Some(&address));
        assert_eq!(snapshot.json["resolution"], "current-native");
        assert_eq!(snapshot.json["current_native"]["id"], "panel [one]");
        assert_eq!(
            snapshot.json["current_native"]["requested_keyboard"],
            "none"
        );
        assert_eq!(
            snapshot.json["current_native"]["committed_keyboard"],
            "exclusive"
        );
        assert_eq!(snapshot.json["event_sequence"], 1);
        assert_eq!(snapshot.json["enter_count"], 1);
    }

    #[test]
    fn unknown_or_replaced_native_cannot_inherit_observed_focus_from_its_address() {
        let (_connection, queue, _server, compositor) = socket();
        let original = compositor.create_surface(&queue.handle(), ());
        let replacement = compositor.create_surface(&queue.handle(), ());
        let address = api::SurfaceAddress::new("same-panel");
        let mut observed = Observations::default();
        observed.enter(true, &original);
        let snapshot = observed.inspect(true, [native(&replacement, &address)]);
        assert!(snapshot.current_address.is_none());
        assert_eq!(snapshot.json["resolution"], "unresolved-native");
        assert!(snapshot.json["current_native"].is_null());
        observed.enter(true, &replacement);
        assert_eq!(
            observed
                .inspect(true, [native(&replacement, &address)])
                .current_address,
            Some(&address)
        );
    }

    #[test]
    fn destroyed_proxy_and_reused_protocol_id_do_not_name_a_replacement() {
        let (connection, mut queue, mut server, compositor) = socket();
        let original = compositor.create_surface(&queue.handle(), ());
        let address = api::SurfaceAddress::new("same-panel");
        let mut observed = Observations::default();
        observed.enter(true, &original);
        let old_id = original.id().protocol_id();
        original.destroy();
        let retired = observed.inspect(true, [native(&original, &address)]);
        assert!(retired.current_address.is_none());
        assert_eq!(retired.json["resolution"], "retired-proxy");
        connection.flush().unwrap();
        server.write_all(&1u32.to_ne_bytes()).unwrap();
        server
            .write_all(&((12u32 << 16) | 1).to_ne_bytes())
            .unwrap();
        server.write_all(&old_id.to_ne_bytes()).unwrap();
        connection.prepare_read().unwrap().read().unwrap();
        queue.dispatch_pending(&mut Peer).unwrap();
        let replacement = compositor.create_surface(&queue.handle(), ());
        assert_eq!(replacement.id().protocol_id(), old_id);
        assert_ne!(original, replacement);
        let snapshot = observed.inspect(true, [native(&replacement, &address)]);
        assert!(snapshot.current_address.is_none());
        assert_eq!(snapshot.json["resolution"], "retired-proxy");
        assert!(snapshot.json["current_native"].is_null());
        assert!(connection.backend().last_error().is_none());
    }

    #[test]
    fn old_leave_and_other_keyboard_events_cannot_erase_current_native_observation() {
        let (_connection, queue, _server, compositor) = socket();
        let original = compositor.create_surface(&queue.handle(), ());
        let replacement = compositor.create_surface(&queue.handle(), ());
        let address = api::SurfaceAddress::new("same-panel");
        let mut observed = Observations::default();
        observed.enter(true, &replacement);
        observed.leave(true, &original);
        observed.enter(false, &original);
        observed.leave(false, &replacement);
        let snapshot = observed.inspect(true, [native(&replacement, &address)]);
        assert_eq!(snapshot.current_address, Some(&address));
        assert_eq!(snapshot.json["event_sequence"], 2);
        assert_eq!(snapshot.json["enter_count"], 1);
        assert_eq!(snapshot.json["leave_count"], 1);
        assert_eq!(snapshot.json["ignored_other_keyboard_events"], 2);
        observed.leave(true, &replacement);
        assert!(
            observed
                .inspect(true, [native(&replacement, &address)])
                .current_address
                .is_none()
        );
    }

    #[test]
    fn removed_keyboard_clears_only_client_observation_without_claiming_global_focus() {
        let (_connection, queue, _server, compositor) = socket();
        let surface = compositor.create_surface(&queue.handle(), ());
        let address = api::SurfaceAddress::new("panel");
        let mut observed = Observations::default();
        observed.enter(true, &surface);
        assert!(
            observed
                .inspect(false, [native(&surface, &address)])
                .current_address
                .is_none()
        );
        observed.keyboard_removed();
        let snapshot = observed.inspect(false, [native(&surface, &address)]);
        assert!(snapshot.current_address.is_none());
        assert_eq!(snapshot.json["resolution"], "keyboard-unavailable");
        assert_eq!(snapshot.json["capability_removal_count"], 1);
        assert!(snapshot.json["current_native"].is_null());
        assert!(
            snapshot.json["scope"]
                .as_str()
                .unwrap()
                .contains("does not establish")
        );
    }

    #[test]
    fn focus_event_counts_become_unavailable_at_overflow_without_wrapping() {
        let (_connection, queue, _server, compositor) = socket();
        let surface = compositor.create_surface(&queue.handle(), ());
        let address = api::SurfaceAddress::new("panel");
        let mut observed = Observations {
            sequence: Count(Some(u64::MAX)),
            enters: Count(Some(u64::MAX)),
            leaves: Count(Some(u64::MAX)),
            ignored_keyboards: Count(Some(u64::MAX)),
            capability_removals: Count(Some(u64::MAX)),
            ..Default::default()
        };
        observed.enter(true, &surface);
        observed.leave(true, &surface);
        observed.enter(false, &surface);
        observed.keyboard_removed();
        // Counter exhaustion cannot prevent a later valid native observation.
        observed.enter(true, &surface);
        let snapshot = observed.inspect(true, [native(&surface, &address)]);
        assert_eq!(snapshot.current_address, Some(&address));
        for field in [
            "event_sequence",
            "enter_count",
            "leave_count",
            "ignored_other_keyboard_events",
            "capability_removal_count",
        ] {
            assert!(snapshot.json[field].is_null());
        }
    }
}
