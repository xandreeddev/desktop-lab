//! Explicit IPC decoding. File paths stay separate JSON values, never shell
//! words. Native launch policy remains behind the application domain port.
use lucent_domain::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    application: String,
    targets: Vec<String>,
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
}
