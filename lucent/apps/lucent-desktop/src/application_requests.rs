//! Explicit application intent. File paths stay separate JSON values, never
//! shell words. SDK authority is translated once into the opaque domain context;
//! native launch policy remains behind application and URI ports.
use lucent_domain::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    application: String,
    targets: Vec<String>,
}
pub fn launch_context(outcome: lucent_api::ActivationOutcome) -> ApplicationLaunchContext {
    match outcome {
        lucent_api::ActivationOutcome::Token(token) => {
            ApplicationLaunchContext::with_activation_token(token.into_value())
                .expect("SDK validated activation token")
        }
        _ => ApplicationLaunchContext::default(),
    }
}
pub fn parse_open(json: &str) -> Result<(AppId, ApplicationOpenRequest)> {
    if json.len() > 3900 {
        return Err(DomainError::Invalid(
            "Application open command is too large".into(),
        ));
    }
    let request: Open = serde_json::from_str(json).map_err(|_| {
        DomainError::Invalid("Use an application ID and explicit file/URI targets".into())
    })?;
    let id = AppId::new(request.application)?;
    let targets = request
        .targets
        .into_iter()
        .map(|value| {
            if value.starts_with('/') {
                ApplicationOpenTarget::file(value)
            } else {
                ApplicationOpenTarget::uri_value(value)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((id, ApplicationOpenRequest::new(targets)?))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn open_arguments_keep_spaces_and_reject_ambient_authority_or_shell_grammar() {
        let (_, request) = parse_open(r#"{"application":"example.desktop","targets":["/tmp/a b$(literal)","https://example.test/a"]}"#).unwrap();
        assert_eq!(
            request.targets()[0].local_file(),
            Some("/tmp/a b$(literal)")
        );
        assert!(parse_open(r#"{"application":"example.desktop","targets":["relative"]}"#).is_err());
        assert!(
            parse_open(r#"{"application":"example.desktop","targets":[],"token":"old"}"#).is_err()
        );
    }

    #[test]
    fn launch_authority_is_only_the_explicit_fresh_outcome() {
        use lucent_api::{ActivationOutcome, ActivationToken};
        let context = launch_context(ActivationOutcome::Token(
            ActivationToken::new("owned-intent".into()).unwrap(),
        ));
        assert_eq!(context.activation_token(), Some("owned-intent"));
        for outcome in [
            ActivationOutcome::Unavailable,
            ActivationOutcome::NoUserInput,
            ActivationOutcome::Busy,
            ActivationOutcome::Timeout,
            ActivationOutcome::Invalid,
        ] {
            assert!(launch_context(outcome).activation_token().is_none());
        }
    }
}
