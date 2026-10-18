//! XDG desktop entry discovery and isolated argument-array launching.
mod actions;
mod activation;
mod catalog;
mod launch;
mod watch;
use lucent_domain::*;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
// Presentation metadata limits are adapter resource policy, not format limits.
const MAX_LABEL_BYTES: usize = 1024;
const MAX_DESCRIPTION_BYTES: usize = 4096;

fn usable_label(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_LABEL_BYTES
        && !value.chars().any(char::is_control)
}

pub struct XdgApplications {
    data_roots: Vec<PathBuf>,
    desktop: String,
    locale: String,
    program_dirs: Vec<PathBuf>,
}
impl Default for XdgApplications {
    fn default() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute());
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.map(|p| p.join(".local/share")));
        let mut roots = data.into_iter().collect::<Vec<_>>();
        roots.extend(
            std::env::var("XDG_DATA_DIRS")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/usr/local/share:/usr/share".into())
                .split(':')
                .filter(|s| s.starts_with('/'))
                .map(PathBuf::from),
        );
        Self {
            data_roots: roots,
            desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "Hyprland".into()),
            locale: std::env::var("LC_ALL")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| std::env::var("LC_MESSAGES").ok().filter(|s| !s.is_empty()))
                .or_else(|| std::env::var("LANG").ok())
                .unwrap_or_default(),
            program_dirs: std::env::var_os("PATH")
                .map(|p| {
                    std::env::split_paths(&p)
                        .filter(|p| p.is_absolute())
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}
impl XdgApplications {
    /// Explicit isolated catalog; no mutation of global process environment.
    pub fn with_source(data_roots: Vec<PathBuf>, desktop: String, locale: String) -> Result<Self> {
        if data_roots.is_empty()
            || data_roots.len() > 64
            || data_roots.iter().any(|p| !p.is_absolute())
        {
            return Err(DomainError::Invalid(
                "Absolute application catalog roots required".into(),
            ));
        }
        Ok(Self {
            data_roots,
            desktop,
            locale,
            program_dirs: vec![],
        })
    }
}
impl ApplicationPort for XdgApplications {
    fn discover(&self) -> Result<Vec<Application>> {
        catalog::discover(self)
    }
    fn watch(&self, emit: &mut dyn FnMut(Result<Vec<Application>>), stop: &dyn StopSignal) {
        watch::observe(self, emit, stop);
    }
    fn launch(&self, app: &Application) -> Result<()> {
        launch::application(app)
    }
    fn launch_action(&self, app: &Application, action: &ApplicationActionId) -> Result<()> {
        lucent_usecases::applications::application_action(app, action)?;
        let current = self
            .discover()?
            .into_iter()
            .find(|current| current.id == app.id);
        if current.as_ref() != Some(app) {
            return Err(DomainError::Invalid(
                "Application changed; choose its action again".into(),
            ));
        }
        let selected = lucent_usecases::applications::application_action(app, action)?;
        launch::action(app, selected)
    }
}
/// URI selection starts the desktop handler in an independent application unit.
pub struct XdgUri;
impl UriPort for XdgUri {
    fn open(&self, target: &ExternalUri) -> Result<()> {
        launch::command(
            &LaunchCommand {
                program: "xdg-open".into(),
                args: vec![target.value().into()],
                directory: None,
                terminal: false,
            },
            None,
        )
    }
}
fn executable(program: &str) -> bool {
    let check = |p: &Path| {
        fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if program.contains('/') {
        return check(Path::new(program));
    }
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| check(&d.join(program))))
}
fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('s') => out.push(' '),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(c) => {
                    out.push('\\');
                    out.push(c)
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c)
        }
    }
    out
}
fn localized(values: &BTreeMap<&str, &str>, key: &str, locale: &str) -> String {
    let (base, modifier) = locale
        .split_once('@')
        .map_or((locale, None), |(base, modifier)| (base, Some(modifier)));
    let base = base.split('.').next().unwrap_or(base);
    let language = base.split('_').next().unwrap_or(base);
    let mut candidates = vec![];
    if let Some(modifier) = modifier {
        candidates.push(format!("{base}@{modifier}"));
    }
    candidates.push(base.to_string());
    if let Some(modifier) = modifier {
        candidates.push(format!("{language}@{modifier}"));
    }
    candidates.push(language.to_string());
    for candidate in candidates {
        if let Some(value) = values.get(format!("{key}[{candidate}]").as_str()) {
            return unescape(value);
        }
    }
    unescape(values.get(key).copied().unwrap_or(""))
}
pub fn parse_entry(
    id: &str,
    path: &str,
    text: &str,
    desktops: &str,
    locale: &str,
) -> Result<Option<Application>> {
    let mut main = false;
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            main = line == "[Desktop Entry]";
            continue;
        }
        if main
            && !line.starts_with('#')
            && let Some((k, v)) = line.split_once('=')
        {
            values.insert(k.trim(), v.trim());
        }
    }
    let value = |k: &str| values.get(k).copied().unwrap_or("");
    if value("Type") != "Application" || value("Hidden") == "true" || value("NoDisplay") == "true" {
        return Ok(None);
    }
    let desktop_names: Vec<_> = desktops.split(':').collect();
    if !value("OnlyShowIn").is_empty()
        && !value("OnlyShowIn")
            .split(';')
            .any(|d| desktop_names.contains(&d))
    {
        return Ok(None);
    }
    if value("NotShowIn")
        .split(';')
        .filter(|s| !s.is_empty())
        .any(|d| desktop_names.contains(&d))
    {
        return Ok(None);
    }
    if !value("TryExec").is_empty() && !executable(&unescape(value("TryExec"))) {
        return Ok(None);
    }
    let name = localized(&values, "Name", locale);
    if !usable_label(&name) || (value("Exec").is_empty() && value("DBusActivatable") != "true") {
        return Ok(None);
    }
    let description = localized(&values, "Comment", locale);
    if description.len() > MAX_DESCRIPTION_BYTES {
        return Ok(None);
    }
    let icon = unescape(value("Icon"));
    let argv = if value("Exec").is_empty() {
        None
    } else {
        Some(parse_exec(&unescape(value("Exec")), &name, path, &icon)?)
    };
    let directory = unescape(value("Path"));
    let actions = actions::parse(text, &values, path, locale, &name, &icon);
    Ok(Some(Application {
        id: AppId::new(id)?,
        name,
        description,
        keywords: localized(&values, "Keywords", locale)
            .split(';')
            .map(str::to_owned)
            .collect(),
        icon,
        startup_class: unescape(value("StartupWMClass")),
        actions,
        launch: {
            let command = argv.map(|argv| LaunchCommand {
                program: argv[0].clone(),
                args: argv[1..].into(),
                directory: (!directory.is_empty()).then_some(directory),
                terminal: value("Terminal") == "true",
            });
            if value("DBusActivatable") == "true" {
                ApplicationLaunch::DesktopService { fallback: command }
            } else {
                ApplicationLaunch::Command(command.expect("Exec required above"))
            }
        },
    }))
}
/// Parse the desktop-entry quoting grammar, then expand each field code once.
pub fn parse_exec(exec: &str, name: &str, path: &str, icon: &str) -> Result<Vec<String>> {
    let invalid = || DomainError::Invalid("Invalid desktop entry Exec field".into());
    // An input file's byte limit alone cannot bound repeated %c/%k expansion.
    // This is an implementation resource policy, not a desktop-entry limit.
    const MAX_EXEC_BYTES: usize = 1024 * 1024;
    const MAX_EXEC_ARGUMENTS: usize = 4096;
    if exec.len() > MAX_EXEC_BYTES {
        return Err(invalid());
    }
    let mut tokens = vec![];
    let mut token = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' => {
                let c = chars.next().ok_or_else(invalid)?;
                token.push(c);
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    if tokens.len() == MAX_EXEC_ARGUMENTS {
                        return Err(invalid());
                    }
                    tokens.push(std::mem::take(&mut token));
                    started = false;
                }
            }
            c => {
                token.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return Err(invalid());
    }
    if started {
        if tokens.len() == MAX_EXEC_ARGUMENTS {
            return Err(invalid());
        }
        tokens.push(token);
    }
    let mut out = vec![];
    let mut bytes = 0;
    for token in tokens {
        match token.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => continue,
            "%i" => {
                if !icon.is_empty() {
                    if out.len() > MAX_EXEC_ARGUMENTS - 2 {
                        return Err(invalid());
                    }
                    let added = "--icon".len().checked_add(icon.len()).ok_or_else(invalid)?;
                    if added > MAX_EXEC_BYTES - bytes {
                        return Err(invalid());
                    }
                    bytes += added;
                    out.extend(["--icon".into(), icon.into()]);
                }
                continue;
            }
            _ => {}
        }
        if out.len() == MAX_EXEC_ARGUMENTS {
            return Err(invalid());
        }
        let mut expanded = String::new();
        let append = |out: &mut String, value: &str| -> Result<()> {
            if value.len() > MAX_EXEC_BYTES - bytes - out.len() {
                return Err(invalid());
            }
            out.push_str(value);
            Ok(())
        };
        let mut cs = token.chars();
        while let Some(c) = cs.next() {
            if c != '%' {
                append(&mut expanded, c.encode_utf8(&mut [0; 4]))?;
                continue;
            }
            match cs.next() {
                Some('%') => append(&mut expanded, "%")?,
                Some('c') => append(&mut expanded, name)?,
                Some('k') => append(&mut expanded, path)?,
                Some('f' | 'u' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm') => {}
                _ => return Err(invalid()),
            }
        }
        bytes += expanded.len();
        out.push(expanded);
    }
    if out.first().is_none_or(|s| s.is_empty() || s.contains('=')) {
        return Err(invalid());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn argv_preserves_quoting_and_does_not_interpret_shell_text() {
        assert_eq!(
            parse_exec(
                r#"app "two words" "$(touch /tmp/no)" %% %c %k %i %U"#,
                "App %U",
                "/a b.desktop",
                "icon"
            )
            .unwrap(),
            vec![
                "app",
                "two words",
                "$(touch /tmp/no)",
                "%",
                "App %U",
                "/a b.desktop",
                "--icon",
                "icon"
            ]
        );
    }
    #[test]
    fn rejects_unknown_codes_and_unbalanced_quotes() {
        for s in ["app %x", "app %i-suffix", "app \"broken"] {
            assert!(parse_exec(s, "", "", "").is_err());
        }
    }
    #[test]
    fn hidden_and_foreign_desktop_entries_are_omitted() {
        for extra in [
            "Hidden=true",
            "NoDisplay=true",
            "OnlyShowIn=GNOME;",
            "NotShowIn=Hyprland;",
        ] {
            let text = format!("[Desktop Entry]\nType=Application\nName=Test\nExec=test\n{extra}");
            assert!(
                parse_entry("test.desktop", "/test.desktop", &text, "Hyprland", "en_US")
                    .unwrap()
                    .is_none()
            );
        }
    }
    #[test]
    fn desktop_actions_do_not_replace_main_exec() {
        let app=parse_entry("a.desktop","/a.desktop","[Desktop Entry]\nType=Application\nName=Test\nName[sv]=Prov\nExec=app %U\n[Desktop Action Other]\nExec=other","Hyprland","sv_SE.UTF-8").unwrap().unwrap();
        assert_eq!(app.name, "Prov");
        assert_eq!(app.launch.command().unwrap().program, "app");
    }
}

#[cfg(test)]
mod service_entry_tests {
    use super::*;
    #[test]
    fn dbus_only_entries_and_exec_fallbacks_have_explicit_launch_data() {
        let only = "[Desktop Entry]\nType=Application\nName=Service\nDBusActivatable=true\n";
        let app = parse_entry(
            "org.example.Service.desktop",
            "/fixture.desktop",
            only,
            "Hyprland",
            "en",
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            app.launch,
            ApplicationLaunch::DesktopService { fallback: None }
        ));
        let with = format!("{only}Exec=service --literal\n");
        let app = parse_entry(
            "org.example.Service.desktop",
            "/fixture.desktop",
            &with,
            "Hyprland",
            "en",
        )
        .unwrap()
        .unwrap();
        assert!(
            matches!(app.launch,ApplicationLaunch::DesktopService{fallback:Some(command)} if command.args==["--literal"])
        );
        assert!(
            parse_entry(
                "org.example.Service.desktop",
                "/fixture.desktop",
                "[Desktop Entry]\nType=Application\nName=Missing\n",
                "Hyprland",
                "en"
            )
            .unwrap()
            .is_none()
        );
    }
}
