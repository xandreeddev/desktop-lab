//! Open only through the explicitly captured application, using its current
//! descriptor and template. No default-handler substitution or shell grammar.
use super::*;
pub(super) fn open(
    source: &XdgApplications,
    app: &Application,
    request: &ApplicationOpenRequest,
    context: &ApplicationLaunchContext,
) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let (current, path, text) = catalog::resolve(source, &app.id)?;
    if current != *app {
        return Err(DomainError::Invalid(
            "Application changed; choose it again".into(),
        ));
    }
    if matches!(current.launch, ApplicationLaunch::DesktopService { .. }) {
        let uris = request
            .targets()
            .iter()
            .map(ApplicationOpenTarget::as_uri)
            .collect::<Vec<_>>();
        match activation::request_context(
            &current.id,
            activation::Operation::Open(&uris),
            context.activation_token(),
            None,
            deadline.saturating_duration_since(std::time::Instant::now()),
        ) {
            Ok(()) => return Ok(()),
            Err(activation::ActivationError::Unsupported) => (),
            Err(activation::ActivationError::Failed) => {
                return Err(DomainError::Failed(
                    "Application opening failed without starting another instance".into(),
                ));
            }
        }
    }
    let commands = commands(&current, &path.to_string_lossy(), &text, request)?;
    for (index, command) in commands.into_iter().enumerate() {
        // A token represents this one user intent, not fresh authority for every
        // window spawned by a single-file template.
        let empty = ApplicationLaunchContext::default();
        launch::executable_before(
            &command,
            if index == 0 { context } else { &empty },
            deadline,
        )?;
    }
    Ok(())
}
fn commands(
    app: &Application,
    path: &str,
    text: &str,
    request: &ApplicationOpenRequest,
) -> Result<Vec<LaunchCommand>> {
    let fallback = app.launch.command().ok_or_else(|| {
        DomainError::Unavailable("Application has no file-opening command".into())
    })?;
    let mut main = false;
    let mut exec = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            main = line == "[Desktop Entry]";
            continue;
        }
        if main && let Some(("Exec", value)) = line.split_once('=') {
            exec = Some(unescape(value.trim()));
        }
    }
    let exec = exec.ok_or_else(|| {
        DomainError::Unavailable("Application has no file-opening command".into())
    })?;
    exec::expand(&exec, &app.name, path, &app.icon, Some(request))?
        .into_iter()
        .map(|argv| {
            Ok(LaunchCommand {
                program: argv[0].clone(),
                args: argv[1..].to_vec(),
                directory: fallback.directory.clone(),
                terminal: fallback.terminal,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn open_reexpands_the_current_template_and_retains_terminal_and_working_directory() {
        let text = "[Desktop Entry]\nType=Application\nName=Files\nExec=/fixture %F\nPath=/tmp/a b\nTerminal=true\n";
        let app = parse_entry("example.desktop", "/entry", text, "", "")
            .unwrap()
            .unwrap();
        let request = ApplicationOpenRequest::new(vec![
            ApplicationOpenTarget::file("/tmp/a b").unwrap(),
            ApplicationOpenTarget::file("/tmp/%U").unwrap(),
        ])
        .unwrap();
        let rows = commands(&app, "/entry", text, &request).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].args, ["/tmp/a b", "/tmp/%U"]);
        assert_eq!(rows[0].directory.as_deref(), Some("/tmp/a b"));
        assert!(rows[0].terminal);
    }
}
