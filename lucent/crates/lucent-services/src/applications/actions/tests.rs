use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn entry(extra: &str) -> Application {
    parse_entry(
        "org.example.Actions.desktop", "/fixture entry.desktop",
        &format!("[Desktop Entry]\nType=Application\nName=Main %U\nIcon=main-icon\nExec=/main --primary\nPath=/fixture directory\nTerminal=true\n{extra}"),
        "Hyprland", "sr_RS.UTF-8@latin",
    ).unwrap().unwrap()
}

#[test]
fn only_declared_actions_keep_list_order_and_localized_labels() {
    let app = entry(
        "Actions=Second;First;Second;Missing;\n\
[Desktop Action First]\nName=First\nName[sr_RS@latin]=Prvi\nIcon=first-icon\nIcon[sr_RS@latin]=localized-icon\nExec=/first\n\
[Desktop Action Second]\nName=Second\nExec=/second\n\
[Desktop Action Unlisted]\nName=Unlisted\nExec=/unlisted\n",
    );
    assert_eq!(
        app.actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        ["Second", "First"]
    );
    assert_eq!(app.actions[1].name, "Prvi");
    assert_eq!(app.actions[1].icon, "localized-icon");
    assert_eq!(app.launch.command().unwrap().program, "/main");
    assert_eq!(app.actions[0].launch.command().unwrap().program, "/second");
}

#[test]
fn locale_fallback_keeps_modifier_after_removing_encoding() {
    let values = BTreeMap::from([
        ("Name", "Base"),
        ("Name[sr]", "Language"),
        ("Name[sr@latin]", "Language modifier"),
        ("Name[sr_RS]", "Country"),
        ("Name[sr_RS@latin]", "Country modifier"),
    ]);
    for (removed, expected) in [
        (vec![], "Country modifier"),
        (vec!["Name[sr_RS@latin]"], "Country"),
        (
            vec!["Name[sr_RS@latin]", "Name[sr_RS]"],
            "Language modifier",
        ),
        (
            vec!["Name[sr_RS@latin]", "Name[sr_RS]", "Name[sr@latin]"],
            "Language",
        ),
        (
            vec![
                "Name[sr_RS@latin]",
                "Name[sr_RS]",
                "Name[sr@latin]",
                "Name[sr]",
            ],
            "Base",
        ),
    ] {
        let mut candidate = values.clone();
        for key in removed {
            candidate.remove(key);
        }
        assert_eq!(localized(&candidate, "Name", "sr_RS.UTF-8@latin"), expected);
    }
}

#[test]
fn ambiguous_and_incomplete_action_groups_cannot_change_main_launch() {
    let app = entry(
        "Actions=NoName;EmptyName;ControlName;NoExec;BrokenExec;Duplicate;Repeated;Valid;bad_id;\n\
[Desktop Action NoName]\nExec=/wrong\n\
[Desktop Action EmptyName]\nName=\nExec=/wrong\n\
[Desktop Action ControlName]\nName=bad\\nlabel\nExec=/wrong\n\
[Desktop Action NoExec]\nName=Missing command\n\
[Desktop Action BrokenExec]\nName=Broken\nExec=/wrong %x\n\
[Desktop Action Duplicate]\nName=First\nName=Second\nExec=/wrong\n\
[Desktop Action Repeated]\nName=First\nExec=/wrong\n\
[Desktop Action Repeated]\nName=Second\nExec=/wrong\n\
[Desktop Action Valid]\nName=Valid\nExec=/chosen\n\
[Desktop Action bad_id]\nName=Bad identity\nExec=/wrong\n",
    );
    assert_eq!(app.actions.len(), 1);
    assert_eq!(app.actions[0].id.as_str(), "Valid");
    assert_eq!(app.launch.command().unwrap().program, "/main");
    assert_eq!(app.launch.command().unwrap().args, ["--primary"]);
}

#[test]
fn action_exec_is_literal_and_inherits_only_main_cwd_terminal_and_macro_context() {
    let app = entry(
        "Actions=New;\n[Desktop Action New]\nName=Action label\nIcon=action-icon\nExec=/action \"two words\" \"$(unused)\" %% %c %k %i %U\nPath=/ignored\nTerminal=false\n",
    );
    let command = app.actions[0].launch.command().unwrap();
    assert_eq!(command.program, "/action");
    assert_eq!(
        command.args,
        [
            "two words",
            "$(unused)",
            "%",
            "Main %U",
            "/fixture entry.desktop",
            "--icon",
            "main-icon"
        ]
    );
    assert_eq!(command.directory.as_deref(), Some("/fixture directory"));
    assert!(command.terminal);
}

#[test]
fn service_only_actions_are_valid_but_never_inherit_main_exec() {
    let app = entry(
        "DBusActivatable=true\nActions=Native;Fallback;MissingName;\n\
[Desktop Action Native]\nName=Native action\n\
[Desktop Action Fallback]\nName=Fallback action\nExec=/action-only\n\
[Desktop Action MissingName]\nName[sr]=Only localized is insufficient\n",
    );
    assert_eq!(app.actions.len(), 2);
    assert!(matches!(
        app.actions[0].launch,
        ApplicationLaunch::DesktopService { fallback: None }
    ));
    assert_eq!(
        app.actions[1].launch.command().unwrap().program,
        "/action-only"
    );
    assert_eq!(app.launch.command().unwrap().program, "/main");
}

#[test]
fn action_resource_caps_skip_invalid_ids_and_keep_first_thirty_two_valid_actions() {
    let ids = (0..34).map(|i| format!("Action-{i}")).collect::<Vec<_>>();
    let too_long = "a".repeat(ApplicationActionId::MAX_BYTES + 1);
    let mut text = format!(
        "Actions={too_long};{};\n[Desktop Action {too_long}]\nName=Too long\nExec=/wrong\n",
        ids.join(";")
    );
    for id in &ids {
        text.push_str(&format!("[Desktop Action {id}]\nName={id}\nExec=/action\n"));
    }
    let app = entry(&text);
    assert_eq!(app.actions.len(), 32);
    assert_eq!(app.actions.first().unwrap().id.as_str(), "Action-0");
    assert_eq!(app.actions.last().unwrap().id.as_str(), "Action-31");
}

#[test]
fn expanded_commands_are_bounded_before_field_code_amplification() {
    let name = "a".repeat(600_000);
    assert!(parse_exec("/fixture %c %c", &name, "", "").is_err());
    assert!(parse_exec("/fixture %k %k", "", &name, "").is_err());
    assert!(parse_exec("/fixture %i %i", "", "", &name).is_err());
    assert_eq!(parse_exec("/fixture %c", &name, "", "").unwrap()[1], name);
}

#[test]
fn empty_arguments_and_icon_expansion_obey_the_argument_count_bound() {
    let at_limit = format!("/fixture {}", "\"\" ".repeat(4095));
    assert_eq!(parse_exec(&at_limit, "", "", "").unwrap().len(), 4096);
    assert!(parse_exec(&format!("{at_limit}\"\""), "", "", "").is_err());
    let icon = format!("/fixture {}%i", "\"\" ".repeat(4094));
    assert!(parse_exec(&icon, "", "", "icon").is_err());
    let icon = format!("/fixture {}%i", "\"\" ".repeat(4093));
    assert_eq!(parse_exec(&icon, "", "", "icon").unwrap().len(), 4096);
}

#[test]
fn unusable_or_oversized_labels_are_rejected_without_truncating_commands() {
    let app = entry(&format!(
        "Actions=TooLong;Valid;\n[Desktop Action TooLong]\nName={}\nExec=/wrong\n[Desktop Action Valid]\nName=Valid\nExec=/chosen\n",
        "x".repeat(MAX_LABEL_BYTES + 1)
    ));
    assert_eq!(app.actions.len(), 1);
    assert_eq!(app.actions[0].id.as_str(), "Valid");
    for field in [
        format!("Name={}", "x".repeat(MAX_LABEL_BYTES + 1)),
        "Name=bad\\nlabel".into(),
        format!(
            "Name=Valid\nComment={}",
            "x".repeat(MAX_DESCRIPTION_BYTES + 1)
        ),
    ] {
        let text = format!("[Desktop Entry]\nType=Application\n{field}\nExec=/main\n");
        assert!(
            parse_entry("fixture.desktop", "/fixture.desktop", &text, "Hyprland", "")
                .unwrap()
                .is_none()
        );
    }
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let path = std::env::temp_dir().join(format!(
            "lucent-action-catalog-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("applications")).unwrap();
        Self(path)
    }
    fn write(&self, text: &str) {
        fs::write(self.0.join("applications/fixture action.desktop"), text).unwrap();
    }
    fn source(&self) -> XdgApplications {
        XdgApplications::with_source(vec![self.0.clone()], "Hyprland".into(), String::new())
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const CAPTURED: &str = "[Desktop Entry]\nType=Application\nName=Fixture\nDBusActivatable=true\nActions=New;\n[Desktop Action New]\nName=New window\n";

#[test]
fn adapter_rejects_replaced_removed_hidden_or_modified_captured_action_before_dispatch() {
    let fixture = Fixture::new();
    fixture.write(CAPTURED);
    let source = fixture.source();
    let captured = source.discover().unwrap().remove(0);
    let action = captured.actions[0].id.clone();
    // This identity is deliberately not a valid D-Bus endpoint and has no Exec:
    // even an erroneously admitted test request cannot reach a bus or process.
    assert!(matches!(
        source.launch_action(&captured, &action),
        Err(DomainError::Unavailable(_))
    ));
    for changed in [
        CAPTURED.replace("Name=New window", "Name=Changed action"),
        format!("{CAPTURED}Exec=/changed-action\n"),
        CAPTURED.replace("Actions=New;", "Actions=Other;"),
        CAPTURED.replace("Name=Fixture", "Name=Changed app"),
        CAPTURED.replace("Type=Application", "Type=Application\nHidden=true"),
    ] {
        fixture.write(&changed);
        assert!(matches!(
            source.launch_action(&captured, &action),
            Err(DomainError::Invalid(_))
        ));
    }
    // Path/Terminal have no launch semantics for a native-only action: there is
    // no executable command in which to retain them. Do not invent source-byte
    // authority for metadata that cannot change this action's dispatch.
    for extra in ["Path=/changed directory", "Terminal=true"] {
        fixture.write(&CAPTURED.replace("Type=Application", &format!("Type=Application\n{extra}")));
        assert_eq!(
            source.discover().unwrap().as_slice(),
            std::slice::from_ref(&captured)
        );
    }
    fs::remove_file(fixture.0.join("applications/fixture action.desktop")).unwrap();
    assert!(matches!(
        source.launch_action(&captured, &action),
        Err(DomainError::Invalid(_))
    ));
}

#[test]
fn action_fallback_directory_and_terminal_changes_retire_captured_descriptor() {
    let fixture = Fixture::new();
    let program = fixture.0.join("never-created-action-executable");
    assert!(!program.exists());
    let text = format!("{CAPTURED}Exec={}\n", program.display());
    fixture.write(&text);
    let source = fixture.source();
    let captured = source.discover().unwrap().remove(0);
    let action = &captured.actions[0].id;
    let command = captured.actions[0].launch.command().unwrap();
    assert_eq!(command.program.as_str(), program.to_string_lossy().as_ref());
    assert_eq!(command.directory, None);
    assert!(!command.terminal);
    // The endpoint is invalid and the captured fallback executable is absent:
    // faulty stale admission still cannot execute an application or terminal.
    for extra in ["Path=/changed directory", "Terminal=true"] {
        fixture.write(&text.replace("Type=Application", &format!("Type=Application\n{extra}")));
        assert_ne!(
            source.discover().unwrap().as_slice(),
            std::slice::from_ref(&captured)
        );
        assert!(matches!(
            source.launch_action(&captured, action),
            Err(DomainError::Invalid(_))
        ));
    }
}

#[test]
fn new_higher_priority_mask_retires_captured_action_authority() {
    let high = Fixture::new();
    let low = Fixture::new();
    low.write(CAPTURED);
    let source = XdgApplications::with_source(
        vec![high.0.clone(), low.0.clone()],
        "Hyprland".into(),
        String::new(),
    )
    .unwrap();
    let captured = source.discover().unwrap().remove(0);
    high.write(&CAPTURED.replace("Type=Application", "Type=Application\nHidden=true"));
    assert!(matches!(
        source.launch_action(&captured, &captured.actions[0].id),
        Err(DomainError::Invalid(_))
    ));
}

#[test]
fn catalog_retained_limit_includes_expanded_action_commands() {
    let fixture = Fixture::new();
    let ids = (0..32).map(|i| format!("Action-{i}")).collect::<Vec<_>>();
    let mut text = format!(
        "[Desktop Entry]\nType=Application\nName=Fixture\nIcon={}\nExec=/main\nActions={};\n",
        "i".repeat(520_000),
        ids.join(";")
    );
    for id in ids {
        text.push_str(&format!(
            "[Desktop Action {id}]\nName=Action\nExec=/action %i\n"
        ));
    }
    assert!(text.len() < 1024 * 1024);
    fixture.write(&text);
    assert!(
        matches!(fixture.source().discover(), Err(DomainError::Unavailable(message)) if message == "Application catalog content limit exceeded")
    );
}

#[test]
fn catalog_retained_limit_includes_empty_argument_storage() {
    let fixture = Fixture::new();
    let ids = (0..32).map(|i| format!("Action-{i}")).collect::<Vec<_>>();
    let mut text = format!(
        "[Desktop Entry]\nType=Application\nName=Fixture\nExec=/main\nActions={};\n",
        ids.join(";")
    );
    let args = "\"\" ".repeat(4095);
    for id in ids {
        text.push_str(&format!(
            "[Desktop Action {id}]\nName=Action\nExec=/action {args}\n"
        ));
    }
    assert!(text.len() < 1024 * 1024);
    fixture.write(&text);
    assert_eq!(fixture.source().discover().unwrap().len(), 1);
    for i in 1..6 {
        fs::write(
            fixture.0.join(format!("applications/fixture-{i}.desktop")),
            &text,
        )
        .unwrap();
    }
    assert!(
        matches!(fixture.source().discover(), Err(DomainError::Unavailable(message)) if message == "Application catalog content limit exceeded")
    );
}
