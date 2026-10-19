//! One explicit user-intent token request. The native runtime supplies authority;
//! components neither see input serials nor read inherited process tokens.
use crate::Effects;
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct ActivationToken(String);
impl ActivationToken {
    pub fn new(value: String) -> Option<Self> {
        (!value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control))
            .then_some(Self(value))
    }
    pub fn into_value(self) -> String {
        self.0
    }
}
impl fmt::Debug for ActivationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ActivationToken(..)")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivationOutcome {
    Token(ActivationToken),
    Unavailable,
    NoUserInput,
    Busy,
    Timeout,
    Invalid,
}
pub struct ActivationRequest<M> {
    pub app_id: String,
    complete: Box<dyn FnOnce(ActivationOutcome) -> M + Send>,
}
impl<M: 'static> ActivationRequest<M> {
    pub fn new(
        app_id: impl Into<String>,
        complete: impl FnOnce(ActivationOutcome) -> M + Send + 'static,
    ) -> Self {
        Self {
            app_id: app_id.into(),
            complete: Box::new(complete),
        }
    }
    pub fn finish(self, outcome: ActivationOutcome) -> M {
        (self.complete)(outcome)
    }
    pub fn map<N: 'static>(
        self,
        map: impl FnOnce(M) -> N + Send + 'static,
    ) -> ActivationRequest<N> {
        ActivationRequest {
            app_id: self.app_id,
            complete: Box::new(move |outcome| map((self.complete)(outcome))),
        }
    }
}
impl<M: 'static> Effects<M> {
    /// Request a fresh token for the current physical input dispatch. An absent
    /// protocol or input returns an explicit outcome; it never borrows old input.
    /// Delivery does not imply the compositor will focus the target application.
    pub fn activation_token(
        &mut self,
        app_id: impl Into<String>,
        complete: impl FnOnce(ActivationOutcome) -> M + Send + 'static,
    ) {
        self.activation
            .push(ActivationRequest::new(app_id, complete));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delegated_request_maps_one_completion_without_exposing_the_token() {
        let mut parent = Effects::<(u8, ActivationOutcome)>::default();
        parent.delegate(
            |child| child.activation_token("org.example.App", |outcome| outcome),
            |outcome| (7, outcome),
        );
        let request = parent.activation.pop().unwrap();
        assert_eq!(request.app_id, "org.example.App");
        let token = ActivationToken::new("owned-test-token".into()).unwrap();
        assert_eq!(format!("{token:?}"), "ActivationToken(..)");
        assert_eq!(
            request.finish(ActivationOutcome::Token(token)),
            (
                7,
                ActivationOutcome::Token(ActivationToken::new("owned-test-token".into()).unwrap())
            )
        );
        assert!(parent.activation.is_empty());
    }
    #[test]
    fn native_token_values_are_bounded() {
        for value in [String::new(), "\0".into(), "x".repeat(4097)] {
            assert!(ActivationToken::new(value).is_none());
        }
    }
}
