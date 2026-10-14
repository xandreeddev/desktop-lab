//! Application processes belong to independent user units, never the shell cgroup.
use lucent_domain::{Application, ApplicationLaunch, DomainError, LaunchCommand, Result};

pub(super) fn application(app: &Application) -> Result<()> {
    if let ApplicationLaunch::DesktopService { fallback } = &app.launch {
        match super::activation::activate(&app.id, None) {
            Ok(()) => return Ok(()),
            Err(super::activation::ActivationError::Unsupported) => {}
            Err(super::activation::ActivationError::Failed) => {
                return Err(DomainError::Failed(
                    "Application activation failed without starting another instance".into(),
                ));
            }
        }
        if fallback.is_none() {
            return Err(DomainError::Unavailable(
                "Application has no available desktop activation service".into(),
            ));
        }
    }
    let launch = app
        .launch
        .command()
        .ok_or_else(|| DomainError::Invalid("Application launch is unavailable".into()))?;
    let terminal = if launch.terminal {
        Some(
            ["xdg-terminal-exec", "foot", "alacritty", "kitty"]
                .into_iter()
                .find(|program| super::executable(program))
                .ok_or_else(|| {
                    DomainError::Unavailable("No supported terminal is installed".into())
                })?,
        )
    } else {
        None
    };
    command(launch, terminal)
}

pub(super) fn command(command: &LaunchCommand, terminal: Option<&str>) -> Result<()> {
    let environment = std::env::vars_os().filter_map(|(name, _)| name.into_string().ok());
    let args = arguments(command, terminal, environment)?;
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    // Type=exec waits for execve acknowledgement, not for the application to exit.
    // Failure must not fall back to a child inside lucent.service: that would
    // make a shell restart terminate the user's application and its documents.
    crate::command("systemd-run", &refs)
        .map(|_| ())
        .map_err(|_| DomainError::Failed("Application could not be started".into()))
}

fn arguments(
    command: &LaunchCommand,
    terminal: Option<&str>,
    environment: impl Iterator<Item = String>,
) -> Result<Vec<String>> {
    if command.program.is_empty() || command.program.contains('\0') {
        return Err(DomainError::Invalid(
            "Application executable is missing or invalid".into(),
        ));
    }
    let mut args = [
        "--user",
        "--quiet",
        "--collect",
        "--service-type=exec",
        "--expand-environment=no",
        "--slice=app.slice",
        "--property=ExitType=cgroup",
        "--property=PartOf=graphical-session.target",
    ]
    .map(str::to_owned)
    .to_vec();
    if let Some(directory) = &command.directory {
        args.push(format!("--working-directory={directory}"));
    } else {
        args.push("--same-dir".into());
    }
    // --setenv=NAME imports the value from systemd-run's inherited environment.
    // Values (including credentials) never become command-line arguments.
    // Service supervision descriptors belong to the shell's unit, not the app.
    for name in environment.filter(|name| inherited_environment(name)) {
        args.push(format!("--setenv={name}"));
    }
    args.push("--".into());
    if let Some(terminal) = terminal {
        args.extend([terminal.into(), "-e".into()]);
    }
    args.push(command.program.clone());
    args.extend(command.args.clone());
    Ok(args)
}

fn inherited_environment(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
        && !matches!(
            name,
            "INVOCATION_ID"
                | "SYSTEMD_EXEC_PID"
                | "NOTIFY_SOCKET"
                | "JOURNAL_STREAM"
                | "WATCHDOG_PID"
                | "WATCHDOG_USEC"
                | "LISTEN_PID"
                | "LISTEN_FDS"
                | "LISTEN_FDNAMES"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_handler_receives_one_literal_target_after_the_unit_separator() {
        let target = "https://example.test/?value=$(literal)&other=`literal`";
        let command = LaunchCommand {
            program: "xdg-open".into(),
            args: vec![target.into()],
            directory: None,
            terminal: false,
        };
        let args = arguments(&command, None, std::iter::empty()).unwrap();
        let at = args.iter().position(|value| value == "--").unwrap();
        assert_eq!(&args[at + 1..], ["xdg-open", target]);
        assert!(args.iter().any(|value| value == "--expand-environment=no"));
        assert!(args.iter().any(|value| value == "--slice=app.slice"));
    }

    #[test]
    #[ignore = "isolated transient units; requires a running systemd user manager and python3"]
    fn application_survives_launcher_unit_exit() {
        use std::{
            fs,
            process::Command,
            time::{Duration, Instant},
        };
        const TEST: &str = "applications::launch::tests::application_survives_launcher_unit_exit";
        if let Ok(directory) = std::env::var("LUCENT_LAUNCH_FIXTURE") {
            let mut app = Application {
                id: lucent_domain::AppId::new("lucent-lifetime-fixture.desktop").unwrap(),
                name: "Lifetime fixture".into(), description: String::new(), keywords: vec![],
                icon: String::new(), startup_class: String::new(),
                launch: lucent_domain::ApplicationLaunch::Command(LaunchCommand {
                    program: "/usr/bin/python3".into(), terminal: false,
                    directory: Some(directory.clone()),
                    args: vec!["-c".into(), r#"import json,os,pathlib,sys,time
pathlib.Path('result.json').write_text(json.dumps({'pid':os.getpid(),'args':sys.argv[1:],'cwd':os.getcwd(),'value':os.environ.get('LUCENT_LAUNCH_VALUE'),'group':pathlib.Path('/proc/self/cgroup').read_text()}))
time.sleep(30)"#.into(), "$HOME".into(), "%h".into(), "$(unused)".into(), "".into()],
                }),
            };
            application(&app).unwrap();
            let start = Instant::now();
            while !std::path::Path::new(&directory)
                .join("result.json")
                .exists()
            {
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "launched program did not execute"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            if let ApplicationLaunch::Command(command) = &mut app.launch {
                command.program = "/no-such-lucent-fixture-executable".into();
            }
            assert!(application(&app).is_err(), "exec failure must be reported");
            return; // systemd stops the parent unit and kills any ordinary children.
        }
        let directory =
            std::env::temp_dir().join(format!("lucent-launch-fixture-{}-%h", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let unit = format!("lucent-launch-fixture-{}", std::process::id());
        struct Cleanup(std::path::PathBuf, String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = Command::new("systemctl")
                    .args(["--user", "stop", &self.1])
                    .output();
                if let Ok(text) = fs::read_to_string(self.0.join("result.json"))
                    && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
                    && let Some(group) = value["group"].as_str()
                    && let Some(unit) = group.trim().rsplit('/').next()
                    && unit.starts_with("run-")
                    && unit.ends_with(".service")
                {
                    let _ = Command::new("systemctl")
                        .args(["--user", "stop", unit])
                        .output();
                }
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(directory.clone(), unit.clone());
        let executable = std::env::current_exe().unwrap();
        let args = vec![
            "--user".to_string(),
            "--quiet".into(),
            "--wait".into(),
            "--pipe".into(),
            "--collect".into(),
            "--service-type=exec".into(),
            "--expand-environment=no".into(),
            format!("--unit={unit}"),
            format!("--setenv=LUCENT_LAUNCH_FIXTURE={}", directory.display()),
            "--setenv=LUCENT_LAUNCH_VALUE=$HOME %h".into(),
            executable.to_string_lossy().into_owned(),
            "--exact".into(),
            TEST.into(),
            "--ignored".into(),
            "--nocapture".into(),
        ];
        crate::command_with_timeout(
            "systemd-run",
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            Duration::from_secs(12),
        )
        .unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("result.json")).unwrap()).unwrap();
        let pid = value["pid"].as_u64().unwrap();
        assert!(
            std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "parent unit exit terminated the launched app"
        );
        assert!(!value["group"].as_str().unwrap().contains(&unit));
        assert_eq!(
            value["args"],
            serde_json::json!(["$HOME", "%h", "$(unused)", ""])
        );
        assert_eq!(value["value"], "$HOME %h");
        assert_eq!(value["cwd"], directory.to_string_lossy().as_ref());
    }

    #[test]
    fn application_arguments_remain_literal_across_the_service_boundary() {
        let command = LaunchCommand {
            program: "/opt/example app".into(),
            args: vec![
                "$HOME".into(),
                "$(unused)".into(),
                "--user".into(),
                String::new(),
            ],
            directory: Some("/tmp/a b".into()),
            terminal: true,
        };
        let args = arguments(
            &command,
            Some("foot"),
            [
                "WAYLAND_DISPLAY",
                "PATH",
                "NOTIFY_SOCKET",
                "LISTEN_FDS",
                "BAD=NAME",
            ]
            .map(str::to_owned)
            .into_iter(),
        )
        .unwrap();
        let split = args.iter().position(|arg| arg == "--").unwrap();
        assert_eq!(
            &args[split + 1..],
            [
                "foot",
                "-e",
                "/opt/example app",
                "$HOME",
                "$(unused)",
                "--user",
                ""
            ]
        );
        assert!(args.contains(&"--expand-environment=no".into()));
        assert!(args.contains(&"--service-type=exec".into()));
        assert!(args.contains(&"--working-directory=/tmp/a b".into()));
        assert!(args.contains(&"--setenv=WAYLAND_DISPLAY".into()));
        assert!(!args.iter().any(|arg| arg.contains("NOTIFY_SOCKET")
            || arg.contains("LISTEN_FDS")
            || arg.contains("BAD=NAME")));
    }
}
