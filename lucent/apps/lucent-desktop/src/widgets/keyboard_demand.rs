//! Transient user intent, independent of the catalog's control capabilities.
use super::Message;
use lucent_api::{
    Cancellation, Effects, KeyboardFocusChange, KeyboardFocusOutcome, NativeSurfaceToken,
    SurfaceAddress,
};
use std::{fmt, sync::Arc};

/// Identity of a single user request, including across Host replacement. A
/// queued completion cannot establish a later request's native ownership.
#[derive(Clone)]
pub struct RequestId(Arc<()>);
impl PartialEq for RequestId {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for RequestId {}
impl fmt::Debug for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KeyboardRequest(..)")
    }
}
struct Owner {
    request: RequestId,
    member: &'static str,
    target: SurfaceAddress,
    cancellation: Cancellation,
    native: Option<NativeSurfaceToken>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
#[derive(Default)]
pub(super) struct Demand(Option<Owner>);
impl Demand {
    pub fn present(&self) -> bool {
        self.0.is_some()
    }
    pub fn clear(&mut self) -> bool {
        self.0.take().is_some()
    }
    pub fn begin(
        &mut self,
        member: &'static str,
        target: SurfaceAddress,
        effects: &mut Effects<Message>,
    ) {
        self.clear();
        let request = RequestId(Arc::new(()));
        let cancellation = Cancellation::default();
        let callback_id = request.clone();
        effects.request_keyboard_focus(target.clone(), cancellation.clone(), move |outcome| {
            Message::KeyboardResolved(callback_id, outcome)
        });
        self.0 = Some(Owner {
            request,
            member,
            target,
            cancellation,
            native: None,
        });
    }
    pub fn matches(&self, target: &SurfaceAddress, members: &[&str]) -> bool {
        self.0
            .as_ref()
            .is_some_and(|owner| &owner.target == target && members.contains(&owner.member))
    }
    pub fn resolve(
        &mut self,
        request: &RequestId,
        outcome: KeyboardFocusOutcome,
    ) -> Option<&'static str> {
        let owner = self.0.as_mut().filter(|owner| owner.request == *request)?;
        match outcome {
            KeyboardFocusOutcome::Focused(native) => {
                owner.native = Some(native);
                None
            }
            outcome => {
                self.clear();
                match outcome {
                    KeyboardFocusOutcome::Unavailable => {
                        Some("This compositor cannot focus widget controls")
                    }
                    KeyboardFocusOutcome::NotRetained => {
                        Some("The compositor did not retain widget keyboard focus")
                    }
                    KeyboardFocusOutcome::Rejected => Some("The widget focus request was rejected"),
                    KeyboardFocusOutcome::Timeout => {
                        Some("The widget did not receive keyboard focus")
                    }
                    _ => None,
                }
            }
        }
    }
    pub fn observe(&mut self, native: &NativeSurfaceToken, change: KeyboardFocusChange) -> bool {
        // Enter is not a new user request. A pending request must not be
        // cancelled by a queued Leave from the previous scope before its Enter.
        if change == KeyboardFocusChange::Entered
            || !self
                .0
                .as_ref()
                .is_some_and(|owner| owner.native.as_ref() == Some(native))
        {
            return false;
        }
        self.clear()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn start(demand: &mut Demand) -> (RequestId, Cancellation) {
        let mut effects = Effects::default();
        demand.begin("notes", SurfaceAddress::new("clock"), &mut effects);
        let request = effects.keyboard_focus.pop().unwrap();
        let cancellation = request.cancellation.clone();
        let Message::KeyboardResolved(id, _) =
            request.finish(KeyboardFocusOutcome::Unavailable).unwrap()
        else {
            panic!("typed completion")
        };
        (id, cancellation)
    }
    #[test]
    fn capability_and_foreground_close_do_not_create_keyboard_demand() {
        let mut demand = Demand::default();
        assert!(!demand.matches(&SurfaceAddress::new("clock"), &["clock", "notes"]));
        let (_, cancel) = start(&mut demand);
        assert!(demand.matches(&SurfaceAddress::new("clock"), &["clock", "notes"]));
        assert!(!demand.matches(&SurfaceAddress::new("clock"), &["clock", "media"]));
        assert!(demand.clear());
        assert!(cancel.cancelled());
        assert!(!demand.matches(&SurfaceAddress::new("clock"), &["clock", "notes"]));
    }
    #[test]
    fn stale_acknowledgement_and_unrelated_native_loss_cannot_change_new_demand() {
        let mut demand = Demand::default();
        let (old, old_cancel) = start(&mut demand);
        let (current, _) = start(&mut demand);
        assert!(old_cancel.cancelled());
        let old_native = NativeSurfaceToken::new();
        assert!(
            demand
                .resolve(&old, KeyboardFocusOutcome::Focused(old_native.clone()))
                .is_none()
        );
        assert!(!demand.observe(&old_native, KeyboardFocusChange::Left));
        let current_native = NativeSurfaceToken::new();
        demand.resolve(
            &current,
            KeyboardFocusOutcome::Focused(current_native.clone()),
        );
        assert!(!demand.observe(&old_native, KeyboardFocusChange::Retired));
        assert!(demand.observe(&current_native, KeyboardFocusChange::Left));
    }
    #[test]
    fn pending_demand_ignores_prior_leave_and_waits_for_exact_focus_ack() {
        let mut demand = Demand::default();
        let (request, _) = start(&mut demand);
        let native = NativeSurfaceToken::new();
        assert!(!demand.observe(&native, KeyboardFocusChange::Left));
        assert!(!demand.observe(&native, KeyboardFocusChange::Entered));
        demand.resolve(&request, KeyboardFocusOutcome::Focused(native.clone()));
        assert!(demand.observe(&native, KeyboardFocusChange::Left));
    }
    #[test]
    fn every_request_failure_and_host_drop_cancel_owned_request() {
        for outcome in [
            KeyboardFocusOutcome::Unavailable,
            KeyboardFocusOutcome::NotRetained,
            KeyboardFocusOutcome::Rejected,
            KeyboardFocusOutcome::Retired,
            KeyboardFocusOutcome::Superseded,
            KeyboardFocusOutcome::Cleared,
            KeyboardFocusOutcome::Timeout,
        ] {
            let mut demand = Demand::default();
            let (request, cancel) = start(&mut demand);
            demand.resolve(&request, outcome);
            assert!(cancel.cancelled());
            assert!(!demand.matches(&SurfaceAddress::new("clock"), &["notes"]));
        }
        let mut demand = Demand::default();
        let (_, cancel) = start(&mut demand);
        drop(demand);
        assert!(cancel.cancelled());
    }
    #[test]
    fn demand_is_scoped_to_exact_output_and_member_group() {
        let mut demand = Demand::default();
        let output = lucent_api::SurfaceInstance::new("DP-2").unwrap();
        demand.begin(
            "notes",
            SurfaceAddress::new("clock").instance(output.clone()),
            &mut Effects::default(),
        );
        assert!(!demand.matches(&SurfaceAddress::new("clock"), &["clock", "notes"]));
        assert!(!demand.matches(
            &SurfaceAddress::new("notes").instance(output.clone()),
            &["notes"]
        ));
        assert!(demand.matches(
            &SurfaceAddress::new("clock").instance(output),
            &["clock", "notes"]
        ));
    }
}
