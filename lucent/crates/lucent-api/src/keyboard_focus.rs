//! Explicit, one-shot keyboard focus requests for ordinary native surfaces.
use crate::{Cancellation, Effects, SurfaceAddress};
use std::{fmt, sync::Arc};

/// Opaque identity of one native surface lifetime. A clone keeps only an
/// identity alive, never a renderer or protocol object. Logical surface names
/// and profiling generations cannot substitute for this identity.
#[derive(Clone, Default)]
pub struct NativeSurfaceToken(Arc<()>);
impl NativeSurfaceToken {
    pub fn new() -> Self {
        Self::default()
    }
}
impl PartialEq for NativeSurfaceToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for NativeSurfaceToken {}
impl fmt::Debug for NativeSurfaceToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeSurfaceToken(..)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardFocusChange {
    /// A dispatched Enter on this client's selected native keyboard.
    Entered,
    /// A dispatched Leave on this client's selected native keyboard.
    Left,
    /// The exact native lifetime has been removed or replaced.
    Retired,
    /// The selected native keyboard or its seat has become unavailable.
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyboardFocusOutcome {
    /// Actual Enter was observed for the exact admitted native lifetime. This
    /// does not establish global compositor focus after later events.
    Focused(NativeSurfaceToken),
    Unavailable,
    Rejected,
    Retired,
    Superseded,
    Cleared,
    Timeout,
}
pub struct KeyboardFocusRequest<M> {
    pub surface: SurfaceAddress,
    pub cancellation: Cancellation,
    complete: Box<dyn FnOnce(KeyboardFocusOutcome) -> M + Send>,
}
impl<M: 'static> KeyboardFocusRequest<M> {
    pub fn new(
        surface: SurfaceAddress,
        cancellation: Cancellation,
        complete: impl FnOnce(KeyboardFocusOutcome) -> M + Send + 'static,
    ) -> Self {
        Self {
            surface,
            cancellation,
            complete: Box::new(complete),
        }
    }
    pub fn finish(self, outcome: KeyboardFocusOutcome) -> Option<M> {
        (!self.cancellation.cancelled()).then(|| (self.complete)(outcome))
    }
    pub fn map<N: 'static>(
        self,
        map: impl FnOnce(M) -> N + Send + 'static,
    ) -> KeyboardFocusRequest<N> {
        KeyboardFocusRequest {
            surface: self.surface,
            cancellation: self.cancellation,
            complete: Box::new(move |outcome| map((self.complete)(outcome))),
        }
    }
}
impl<M: 'static> Effects<M> {
    /// Request keyboard focus from the current genuine primary press on a
    /// keyboard-focusable prepared hit. This grants no synthetic input authority:
    /// unsupported backends, secure roles, other surfaces and stale lifetimes
    /// reject. The target must request OnDemand. The runtime preserves presented
    /// geometry when committing eligibility; it never promotes a layer or pulses
    /// Exclusive. An optional native adapter may briefly acquire a focus scope,
    /// but must release it when Enter is observed, preserving ordinary inline UI
    /// behavior rather than holding a persistent pointer/keyboard restriction.
    ///
    /// Cancellation applies to the pending request only. Focused acknowledges
    /// the exact native Enter, never merely a protocol commit, and does not
    /// promise continued ownership: normal KeyboardFocus loss events remain
    /// authoritative. The client requests None when its intent ends. Immediate
    /// first-key and outside-click delivery require compositor integration tests;
    /// this API promises no zero wire latency or global compositor ownership.
    pub fn request_keyboard_focus(
        &mut self,
        surface: SurfaceAddress,
        cancellation: Cancellation,
        complete: impl FnOnce(KeyboardFocusOutcome) -> M + Send + 'static,
    ) {
        self.keyboard_focus
            .push(KeyboardFocusRequest::new(surface, cancellation, complete));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_tokens_identify_lifetimes_without_aliasing_replacements() {
        let original = NativeSurfaceToken::new();
        assert_eq!(original, original.clone());
        assert_ne!(original, NativeSurfaceToken::new());
        assert_eq!(format!("{original:?}"), "NativeSurfaceToken(..)");
    }
    #[test]
    fn delegated_focus_request_retains_target_token_and_cancellation() {
        let cancel = Cancellation::default();
        let token = NativeSurfaceToken::new();
        let mut parent = Effects::<(u8, KeyboardFocusOutcome)>::default();
        parent.delegate(
            |child| {
                child.request_keyboard_focus(
                    SurfaceAddress::new("editor"),
                    cancel.clone(),
                    std::convert::identity,
                )
            },
            |outcome| (7, outcome),
        );
        let request = parent.keyboard_focus.pop().unwrap();
        assert_eq!(request.surface, SurfaceAddress::new("editor"));
        assert_eq!(
            request.finish(KeyboardFocusOutcome::Focused(token.clone())),
            Some((7, KeyboardFocusOutcome::Focused(token)))
        );
        parent.request_keyboard_focus(SurfaceAddress::new("editor"), cancel.clone(), |outcome| {
            (9, outcome)
        });
        cancel.cancel();
        assert!(
            parent
                .keyboard_focus
                .pop()
                .unwrap()
                .finish(KeyboardFocusOutcome::Timeout)
                .is_none()
        );
    }
}
