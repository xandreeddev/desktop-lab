//! Desktop-entry argument grammar and one-pass field expansion. No shell and
//! no IO. One-file fields produce separate invocations; list fields retain argv.
use lucent_domain::{ApplicationOpenRequest, ApplicationOpenTarget, DomainError, Result};
const MAX_BYTES: usize = 1024 * 1024;
const MAX_ARGUMENTS: usize = 4096;
fn invalid() -> DomainError {
    DomainError::Invalid("Invalid desktop entry Exec field".into())
}
struct Token {
    value: String,
    quoted_field: bool,
}
fn tokens(exec: &str) -> Result<Vec<Token>> {
    if exec.len() > MAX_BYTES || exec.contains('\0') {
        return Err(invalid());
    }
    let mut tokens = vec![];
    let (mut value, mut quoted, mut started, mut quoted_field) =
        (String::new(), false, false, false);
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' => {
                let next = chars.next().ok_or_else(invalid)?;
                if quoted && !matches!(next, '"' | '`' | '$' | '\\') {
                    value.push('\\');
                }
                value.push(next);
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    if tokens.len() == MAX_ARGUMENTS {
                        return Err(invalid());
                    }
                    tokens.push(Token {
                        value: std::mem::take(&mut value),
                        quoted_field,
                    });
                    started = false;
                    quoted_field = false;
                }
            }
            c => {
                // A field code in quotes is undefined by the specification;
                // reject it rather than guessing a multiple-argument expansion.
                if quoted && c == '%' && chars.peek() != Some(&'%') {
                    quoted_field = true;
                }
                value.push(c);
                started = true;
                if c == '%' && chars.peek() == Some(&'%') {
                    value.push(chars.next().unwrap());
                }
            }
        }
    }
    if quoted {
        return Err(invalid());
    }
    if started {
        tokens.push(Token {
            value,
            quoted_field,
        });
    }
    if tokens.len() > MAX_ARGUMENTS || tokens.iter().any(|token| token.quoted_field) {
        return Err(invalid());
    }
    Ok(tokens)
}
fn field(tokens: &[Token]) -> Result<Option<char>> {
    let mut field = None;
    for token in tokens {
        let mut chars = token.value.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                continue;
            }
            match chars.next() {
                Some(code @ ('f' | 'F' | 'u' | 'U')) => {
                    if field.replace(code).is_some() {
                        return Err(invalid());
                    }
                    if matches!(code, 'F' | 'U') && token.value != format!("%{code}") {
                        return Err(invalid());
                    }
                }
                Some('i') if token.value != "%i" => return Err(invalid()),
                Some('%' | 'c' | 'k' | 'i' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm') => (),
                _ => return Err(invalid()),
            }
        }
    }
    Ok(field)
}
fn local(target: &ApplicationOpenTarget) -> Result<String> {
    if let Some(path) = target.local_file() {
        return Ok(path.to_owned());
    }
    let uri = target.as_uri();
    let path = uri
        .split_once(':')
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("file"))
        .and_then(|(_, rest)| {
            let path = rest.split(['?', '#']).next()?;
            if let Some(authority) = path.strip_prefix("//") {
                let (host, tail) = authority.split_once('/')?;
                (host.is_empty() || host.eq_ignore_ascii_case("localhost"))
                    .then(|| format!("/{tail}"))
            } else {
                path.starts_with('/').then(|| path.to_owned())
            }
        })
        .ok_or_else(|| {
            DomainError::Unavailable("This application accepts local files only".into())
        })?;
    let mut bytes = vec![];
    let mut input = path.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let a = input
                .next()
                .and_then(|v| char::from(v).to_digit(16))
                .ok_or_else(invalid)?;
            let b = input
                .next()
                .and_then(|v| char::from(v).to_digit(16))
                .ok_or_else(invalid)?;
            bytes.push((a * 16 + b) as u8);
        } else {
            bytes.push(byte);
        }
    }
    let path = String::from_utf8(bytes).map_err(|_| invalid())?;
    ApplicationOpenTarget::file(path.clone())?;
    Ok(path)
}
pub(super) fn expand(
    exec: &str,
    name: &str,
    path: &str,
    icon: &str,
    request: Option<&ApplicationOpenRequest>,
) -> Result<Vec<Vec<String>>> {
    let tokens = tokens(exec)?;
    let field = field(&tokens)?;
    let targets = request.map(ApplicationOpenRequest::targets).unwrap_or(&[]);
    if !targets.is_empty() && field.is_none() {
        return Err(DomainError::Unavailable(
            "This application declares no file or URI arguments".into(),
        ));
    }
    let values = targets
        .iter()
        .map(|target| {
            if matches!(field, Some('f' | 'F')) {
                local(target)
            } else {
                Ok(target.as_uri())
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let count = if matches!(field, Some('f' | 'u')) {
        values.len().max(1)
    } else {
        1
    };
    let mut invocations = Vec::with_capacity(count);
    let mut total_bytes = 0usize;
    for index in 0..count {
        let mut out = vec![];
        let mut bytes = 0usize;
        let mut push = |value: String| -> Result<()> {
            if out.len() >= MAX_ARGUMENTS || value.contains('\0') || value.len() > MAX_BYTES - bytes
            {
                return Err(invalid());
            }
            bytes += value.len();
            out.push(value);
            Ok(())
        };
        for token in &tokens {
            match token.value.as_str() {
                "%F" | "%U" => {
                    for value in &values {
                        push(value.clone())?;
                    }
                    continue;
                }
                "%f" | "%u" if values.is_empty() => continue,
                "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => continue,
                "%i" => {
                    if !icon.is_empty() {
                        push("--icon".into())?;
                        push(icon.into())?;
                    }
                    continue;
                }
                _ => (),
            }
            let mut expanded = String::new();
            let mut chars = token.value.chars();
            while let Some(c) = chars.next() {
                if c != '%' {
                    expanded.push(c);
                } else {
                    match chars.next().ok_or_else(invalid)? {
                        '%' => expanded.push('%'),
                        'c' => expanded.push_str(name),
                        'k' => expanded.push_str(path),
                        'f' | 'u' => {
                            if let Some(value) = values.get(index) {
                                expanded.push_str(value);
                            }
                        }
                        'd' | 'D' | 'n' | 'N' | 'v' | 'm' => (),
                        _ => return Err(invalid()),
                    }
                }
                if expanded.len() > MAX_BYTES {
                    return Err(invalid());
                }
            }
            push(expanded)?;
        }
        if out
            .first()
            .is_none_or(|program| program.is_empty() || program.contains('='))
        {
            return Err(invalid());
        }
        total_bytes = total_bytes.checked_add(bytes).ok_or_else(invalid)?;
        if total_bytes > 4 * MAX_BYTES {
            return Err(invalid());
        }
        invocations.push(out);
    }
    Ok(invocations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_domain::ExternalUri;
    fn request() -> ApplicationOpenRequest {
        ApplicationOpenRequest::new(vec![
            ApplicationOpenTarget::file("/tmp/a b%U").unwrap(),
            ApplicationOpenTarget::file("/tmp/雪").unwrap(),
        ])
        .unwrap()
    }
    #[test]
    fn single_and_plural_fields_preserve_literal_args_and_expand_once() {
        let request = request();
        assert_eq!(
            expand("app %f %% %c", "Name %U", "/entry", "", Some(&request)).unwrap(),
            vec![
                vec!["app", "/tmp/a b%U", "%", "Name %U"],
                vec!["app", "/tmp/雪", "%", "Name %U"]
            ]
        );
        assert_eq!(
            expand("app %F", "", "", "", Some(&request)).unwrap(),
            vec![vec!["app", "/tmp/a b%U", "/tmp/雪"]]
        );
        assert_eq!(
            expand("app %U", "", "", "", Some(&request)).unwrap(),
            vec![vec![
                "app",
                "file:///tmp/a%20b%25U",
                "file:///tmp/%E9%9B%AA"
            ]]
        );
    }
    #[test]
    fn unsupported_remote_files_and_ambiguous_fields_fail_instead_of_losing_targets() {
        let request = ApplicationOpenRequest::new(vec![ApplicationOpenTarget::uri(
            ExternalUri::new("https://example.test/a").unwrap(),
        )])
        .unwrap();
        assert!(expand("app %f", "", "", "", Some(&request)).is_err());
        assert!(expand("app --flag", "", "", "", Some(&request)).is_err());
        for exec in [
            "app %f %U",
            "app prefix%F",
            "app \"%u\"",
            "app %z",
            "app prefix%i",
        ] {
            assert!(expand(exec, "", "", "", None).is_err(), "{exec}");
        }
        assert_eq!(
            expand("app \"\" %U", "", "", "", None).unwrap(),
            vec![vec!["app", ""]]
        );
    }
    #[test]
    fn local_file_uris_decode_once_without_becoming_field_codes() {
        let request = ApplicationOpenRequest::new(vec![ApplicationOpenTarget::uri(
            ExternalUri::new("file:///tmp/a%20b%25f").unwrap(),
        )])
        .unwrap();
        assert_eq!(
            expand("app %f", "", "", "", Some(&request)).unwrap(),
            vec![vec!["app", "/tmp/a b%f"]]
        );
        for uri in ["FILE:/tmp/a%20b#page", "file://LOCALHOST/tmp/a%20b?ignored"] {
            let request =
                ApplicationOpenRequest::new(vec![ApplicationOpenTarget::uri_value(uri).unwrap()])
                    .unwrap();
            assert_eq!(
                expand("app %f", "", "", "", Some(&request)).unwrap(),
                vec![vec!["app", "/tmp/a b"]]
            );
        }
    }
}
