//! Authentication is an interactive conversation with a trusted system backend.
//! Secrets never implement Debug/Clone and never belong in a rendered element.
use crate::Result;
use zeroize::{Zeroize, Zeroizing};
pub struct Secret(Zeroizing<String>);
impl Default for Secret {
    fn default() -> Self {
        Self(Zeroizing::new(String::new()))
    }
}
impl Secret {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn push(&mut self, text: &str) {
        if self.0.len() + text.len() <= 4096 {
            self.0.push_str(text);
        }
    }
    pub fn pop(&mut self) {
        self.0.pop();
    }
    pub fn clear(&mut self) {
        self.0.zeroize();
    }
    pub fn len(&self) -> usize {
        self.0.chars().count()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Visible,
    Secret,
    Information,
    Error,
}
#[derive(Clone, Debug)]
pub struct AuthPrompt {
    pub kind: PromptKind,
    pub text: String,
}
pub trait AuthConversation {
    fn prompt(&mut self, prompt: AuthPrompt) -> Result<Secret>;
}
pub trait AuthenticationPort: Send + Sync {
    /// Returns success only after authentication and account checks (and, for a
    /// login adapter, acceptance of the session-start request).
    fn authenticate(&self, identity: &str, conversation: &mut dyn AuthConversation) -> Result<()>;
}
