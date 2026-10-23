//! Explicit user-selected URI targets. No opening or transport policy here.
use crate::{ApplicationLaunchContext, DomainError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalUri(String);
impl ExternalUri {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let valid = value.len() <= 2048
            && !value.chars().any(|c| c.is_control() || c.is_whitespace())
            && value.split_once(':').is_some_and(|(scheme, rest)| {
                !rest.is_empty()
                    && match scheme.to_ascii_lowercase().as_str() {
                        "http" | "https" => rest.strip_prefix("//").is_some_and(|authority| {
                            !authority.is_empty() && !authority.starts_with('/')
                        }),
                        "mailto" => !rest.starts_with('/'),
                        "file" => rest.starts_with("///") || rest.starts_with("//localhost/"),
                        _ => false,
                    }
            });
        if !valid {
            return Err(DomainError::Invalid("URI target is not supported".into()));
        }
        Ok(Self(value))
    }
    pub fn value(&self) -> &str {
        &self.0
    }
}
/// Opening requires an explicit client action; a renderer never interprets URIs.
pub trait UriPort: Send + Sync {
    /// The supplied context belongs to this one explicit opening intent. An
    /// empty context must not inherit activation authority from the process or
    /// session manager; resolving the default handler remains adapter policy.
    fn open(&self, target: &ExternalUri, context: ApplicationLaunchContext) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_targets_are_bounded_and_reject_commands_and_remote_files() {
        for target in [
            "https://example.test/a",
            "HTTP://example.test/",
            "mailto:a@example.test",
            "file:///tmp/a",
            "file://localhost/tmp/a",
        ] {
            assert_eq!(ExternalUri::new(target).unwrap().value(), target);
        }
        for target in [
            "javascript:alert(1)",
            "exec:terminal",
            "https:///path",
            "https://",
            "file://server/file",
            "file:/tmp/a",
            "https://example.test/\n",
            "https://example.test/a b",
            "mailto:",
        ] {
            assert!(ExternalUri::new(target).is_err(), "{target}");
        }
        assert!(ExternalUri::new(format!("https://example.test/{}", "a".repeat(2048))).is_err());
    }
}
