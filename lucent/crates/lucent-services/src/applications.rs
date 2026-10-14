//! XDG desktop entry discovery and isolated argument-array launching.
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
    let locale = locale.split('.').next().unwrap_or(locale);
    let language = locale.split('_').next().unwrap_or(locale);
    let localized = |key: &str| -> String {
        for l in [locale, language] {
            if let Some(v) = values.get(format!("{key}[{l}]").as_str()) {
                return unescape(v);
            }
        }
        unescape(value(key))
    };
    let name = localized("Name");
    if name.is_empty() || (value("Exec").is_empty() && value("DBusActivatable") != "true") {
        return Ok(None);
    }
    let icon = unescape(value("Icon"));
    let argv = if value("Exec").is_empty() {
        None
    } else {
        Some(parse_exec(&unescape(value("Exec")), &name, path, &icon)?)
    };
    let directory = unescape(value("Path"));
    Ok(Some(Application {
        id: AppId::new(id)?,
        name,
        description: localized("Comment"),
        keywords: localized("Keywords")
            .split(';')
            .map(str::to_owned)
            .collect(),
        icon,
        startup_class: unescape(value("StartupWMClass")),
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
        tokens.push(token);
    }
    let mut out = vec![];
    for token in tokens {
        match token.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => continue,
            "%i" => {
                if !icon.is_empty() {
                    out.extend(["--icon".into(), icon.into()]);
                }
                continue;
            }
            _ => {}
        }
        let mut expanded = String::new();
        let mut cs = token.chars();
        while let Some(c) = cs.next() {
            if c != '%' {
                expanded.push(c);
                continue;
            }
            match cs.next() {
                Some('%') => expanded.push('%'),
                Some('c') => expanded.push_str(name),
                Some('k') => expanded.push_str(path),
                Some('f' | 'u' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm') => {}
                _ => return Err(invalid()),
            }
        }
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
