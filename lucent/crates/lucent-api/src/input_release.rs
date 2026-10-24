//! Acknowledgement of a client's own native input-policy release.
use crate::{Cancellation, Effects, SurfaceAddress};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputReleaseOutcome {
    /// The compositor callback for a post-admission None/None submission arrived.
    /// This acknowledges commit processing, not scanout or destination focus.
    Released,
    Unavailable,
    Retired,
    Superseded,
    Busy,
    Timeout,
}
pub struct InputReleaseRequest<M> {
    pub surface: SurfaceAddress,
    pub cancellation: Cancellation,
    complete: Box<dyn FnOnce(InputReleaseOutcome) -> M + Send>,
}
impl<M: 'static> InputReleaseRequest<M> {
    pub fn new(
        surface: SurfaceAddress,
        cancellation: Cancellation,
        complete: impl FnOnce(InputReleaseOutcome) -> M + Send + 'static,
    ) -> Self {
        Self {
            surface,
            cancellation,
            complete: Box::new(complete),
        }
    }
    pub fn finish(self, outcome: InputReleaseOutcome) -> Option<M> {
        (!self.cancellation.cancelled()).then(|| (self.complete)(outcome))
    }
    pub fn map<N: 'static>(
        self,
        map: impl FnOnce(M) -> N + Send + 'static,
    ) -> InputReleaseRequest<N> {
        InputReleaseRequest {
            surface: self.surface,
            cancellation: self.cancellation,
            complete: Box::new(move |outcome| map((self.complete)(outcome))),
        }
    }
}
impl<M: 'static> Effects<M> {
    /// Await a compositor frame callback for a successful post-admission
    /// submission on this exact native layer lifetime. The client must already
    /// request Keyboard::None and PointerInput::None. This never overrides an
    /// interactive policy or commits pending geometry early. Secure/unsupported
    /// roles reject; cancellation suppresses delivery. No scanout/focus promise.
    pub fn after_input_release(
        &mut self,
        surface: SurfaceAddress,
        cancellation: Cancellation,
        complete: impl FnOnce(InputReleaseOutcome) -> M + Send + 'static,
    ) {
        self.input_release
            .push(InputReleaseRequest::new(surface, cancellation, complete));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delegation_preserves_target_cancellation_and_one_typed_completion() {
        let cancel = Cancellation::default();
        let mut parent = Effects::<(u8, InputReleaseOutcome)>::default();
        parent.delegate(
            |child| {
                child.after_input_release(
                    SurfaceAddress::new("panel"),
                    cancel.clone(),
                    std::convert::identity,
                )
            },
            |outcome| (7, outcome),
        );
        let request = parent.input_release.pop().unwrap();
        assert_eq!(request.surface, SurfaceAddress::new("panel"));
        assert_eq!(
            request.finish(InputReleaseOutcome::Released),
            Some((7, InputReleaseOutcome::Released))
        );
        parent.after_input_release(SurfaceAddress::new("panel"), cancel.clone(), |outcome| {
            (9, outcome)
        });
        cancel.cancel();
        assert!(
            parent
                .input_release
                .pop()
                .unwrap()
                .finish(InputReleaseOutcome::Timeout)
                .is_none()
        );
    }
}
