//! Explicit application-opening intent and opaque launch authority. These
//! values contain no native serial, D-Bus path, executable grammar or IO.
use crate::{DomainError, ExternalUri, Result};
use std::fmt;

#[derive(Default)]
pub struct ApplicationLaunchContext {
    activation_token: Option<String>,
}
impl ApplicationLaunchContext {
    pub fn with_activation_token(token: String) -> Result<Self> {
        if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
            return Err(DomainError::Invalid(
                "Invalid application activation token".into(),
            ));
        }
        Ok(Self {
            activation_token: Some(token),
        })
    }
    pub fn activation_token(&self) -> Option<&str> {
        self.activation_token.as_deref()
    }
}
impl fmt::Debug for ApplicationLaunchContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApplicationLaunchContext(..)")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    File(String),
    Uri(ExternalUri),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationOpenTarget(Target);
impl ApplicationOpenTarget {
    pub fn file(path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        if !path.starts_with('/') || path.len() > 4096 || path.chars().any(char::is_control) {
            return Err(DomainError::Invalid(
                "An absolute local file path is required".into(),
            ));
        }
        Ok(Self(Target::File(path)))
    }
    pub fn uri(uri: ExternalUri) -> Self {
        Self(Target::Uri(uri))
    }
    pub fn local_file(&self) -> Option<&str> {
        match &self.0 {
            Target::File(path) => Some(path),
            _ => None,
        }
    }
    pub fn as_uri(&self) -> String {
        match &self.0 {
            Target::Uri(uri) => uri.value().to_owned(),
            Target::File(path) => {
                let mut out = String::from("file://");
                for byte in path.bytes() {
                    if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
                        out.push(char::from(byte));
                    } else {
                        use std::fmt::Write;
                        write!(out, "%{byte:02X}").expect("write to String");
                    }
                }
                out
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationOpenRequest(Vec<ApplicationOpenTarget>);
impl ApplicationOpenRequest {
    pub fn new(targets: Vec<ApplicationOpenTarget>) -> Result<Self> {
        if targets.is_empty()
            || targets.len() > 64
            || targets
                .iter()
                .map(|target| target.as_uri().len())
                .sum::<usize>()
                > 128 * 1024
        {
            return Err(DomainError::Invalid(
                "Open requests require 1..64 bounded file or URI targets".into(),
            ));
        }
        Ok(Self(targets))
    }
    pub fn targets(&self) -> &[ApplicationOpenTarget] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_file_targets_are_literal_and_uris_encode_once() {
        let target = ApplicationOpenTarget::file("/tmp/a b%雪$(literal)").unwrap();
        assert_eq!(target.local_file(), Some("/tmp/a b%雪$(literal)"));
        assert_eq!(
            target.as_uri(),
            "file:///tmp/a%20b%25%E9%9B%AA%24%28literal%29"
        );
        assert!(ApplicationOpenTarget::file("relative").is_err());
        assert!(ApplicationOpenRequest::new(vec![]).is_err());
        assert!(ApplicationOpenRequest::new(vec![target; 65]).is_err());
    }
    #[test]
    fn launch_authority_is_checked_and_not_debug_printed() {
        assert!(ApplicationLaunchContext::with_activation_token("\0".into()).is_err());
        let context =
            ApplicationLaunchContext::with_activation_token("private-token".into()).unwrap();
        assert_eq!(context.activation_token(), Some("private-token"));
        assert!(!format!("{context:?}").contains("private-token"));
    }
}
