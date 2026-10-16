mod session_lock;
mod wire;
use lucent_domain::*;
use serde_json::Value;
use std::{
    io::Read,
    path::PathBuf,
    time::{Duration, Instant},
};
#[derive(Clone)]
pub struct Hyprland {
    base: PathBuf,
}
impl Hyprland {
    pub fn from_env() -> Result<Self> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| DomainError::Unavailable("No runtime directory".into()))?;
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .map_err(|_| DomainError::Unavailable("Hyprland is not available".into()))?;
        if signature.contains('/') || signature.contains("..") {
            return Err(DomainError::Invalid("Invalid compositor instance".into()));
        }
        Ok(Self {
            base: PathBuf::from(runtime).join("hypr").join(signature),
        })
    }
    pub fn request(&self, request: &str) -> Result<String> {
        wire::request(&self.base.join(".socket.sock"), request).map_err(|_| {
            DomainError::Unavailable("Hyprland command connection is unavailable".into())
        })
    }
    fn json_before(&self, request: &str, deadline: Instant) -> Result<Value> {
        let response = wire::request_before(&self.base.join(".socket.sock"), request, deadline)
            .map_err(|_| {
                DomainError::Unavailable("Hyprland snapshot connection is unavailable".into())
            })?;
        serde_json::from_str(&response).map_err(|e| DomainError::Failed(e.to_string()))
    }
}
impl CompositorPort for Hyprland {
    fn watch(&self, emit: &mut dyn FnMut(Result<CompositorSnapshot>), cancel: &dyn StopSignal) {
        while !cancel.cancelled() {
            // Subscribe before querying, so changes during the initial query
            // remain queued instead of falling into a discovery/observe gap.
            if let Ok(mut socket) = wire::connect(&self.base.join(".socket2.sock")) {
                if socket
                    .set_read_timeout(Some(Duration::from_millis(250)))
                    .is_err()
                {
                    emit(Err(DomainError::Unavailable(
                        "Hyprland observation is unavailable".into(),
                    )));
                    cancel.wait(Duration::from_secs(2));
                    continue;
                }
                emit(self.snapshot());
                let mut events = wire::Events::default();
                let mut buffer = [0u8; 4096];
                while !cancel.cancelled() {
                    match socket.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => match events.push(&buffer[..read]) {
                            Ok(true) => emit(self.snapshot()),
                            Ok(false) => {}
                            Err(_) => break,
                        },
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(_) => break,
                    }
                }
                if !cancel.cancelled() {
                    emit(Err(DomainError::Unavailable(
                        "Hyprland observation disconnected".into(),
                    )));
                }
            } else {
                if !cancel.cancelled() {
                    emit(Err(DomainError::Unavailable(
                        "Hyprland observation is unavailable".into(),
                    )));
                }
            }
            cancel.wait(Duration::from_secs(2));
        }
    }
    fn snapshot(&self) -> Result<CompositorSnapshot> {
        let deadline = Instant::now() + wire::REQUEST_TIMEOUT;
        let active = decode_workspace_ref(&self.json_before("j/activeworkspace", deadline)?);
        let focused = self.json_before("j/activewindow", deadline)?["address"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let ws = self.json_before("j/workspaces", deadline)?;
        let clients = self.json_before("j/clients", deadline)?;
        let monitors = self.json_before("j/monitors", deadline)?;
        Ok(decode_snapshot(
            active.as_ref(),
            &focused,
            &ws,
            &clients,
            &monitors,
        ))
    }

    fn workspace(&self, command: WorkspaceCommand) -> Result<()> {
        let request = workspace_request(command)?;
        let result = self.request(&request)?;
        if result.trim() == "ok" {
            Ok(())
        } else {
            // Do not return a native error that could echo a selected name.
            Err(DomainError::Failed(
                "Hyprland rejected the workspace action".into(),
            ))
        }
    }
    fn focus_window(&self, address: &str) -> Result<()> {
        if !address.starts_with("0x")
            || address.len() <= 2
            || !address[2..].chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err(DomainError::Invalid("Invalid window address".into()));
        }
        let result = self.request(&format!(
            "dispatch hl.dsp.focus({{ window = \"address:{address}\" }})"
        ))?;
        if result.trim() == "ok" {
            Ok(())
        } else {
            Err(DomainError::Failed(result))
        }
    }
}

fn decode_snapshot(
    active: Option<&WorkspaceRef>,
    focused: &str,
    ws: &Value,
    clients: &Value,
    raw_monitors: &Value,
) -> CompositorSnapshot {
    let monitors = decode_monitors(raw_monitors, clients);
    let mut workspaces = std::collections::BTreeMap::new();
    let mut ambiguous = std::collections::BTreeSet::new();
    for value in ws.as_array().into_iter().flatten() {
        let Some(id) = decode_workspace_ref(value) else {
            continue;
        };
        let workspace = Workspace {
            active: active == Some(&id),
            id: id.clone(),
            name: value["name"].as_str().unwrap_or("").into(),
            windows: value["windows"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(0),
        };
        if workspaces.insert(id.clone(), workspace).is_some() {
            ambiguous.insert(id);
        }
    }
    workspaces.retain(|id, _| !ambiguous.contains(id));
    let windows = clients
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            let address = v["address"].as_str().unwrap_or("").to_owned();
            Window {
                focused: !focused.is_empty() && address == focused,
                address,
                app_class: v["class"].as_str().unwrap_or("").into(),
                title: v["title"].as_str().unwrap_or("").into(),
                workspace: decode_workspace_ref(&v["workspace"]),
            }
        })
        .collect();
    CompositorSnapshot {
        workspaces: workspaces.into_values().collect(),
        windows,
        monitors,
    }
}

/// Modern `address` is authoritative. Legacy numbered IDs and negative
/// named/special IDs normalize to the same domain value as modern records.
fn decode_workspace_ref(value: &Value) -> Option<WorkspaceRef> {
    if value.get("address").is_some() {
        let address = value["address"].as_str()?;
        if address.is_empty() {
            return None;
        }
        return match value["type"].as_str() {
            // Modern normal addresses are the actual bare name, not the
            // `name:` selector used by dispatch. Preserve reserved prefixes.
            Some("normal") => {
                if value.get("id").is_some() {
                    value["id"]
                        .as_i64()
                        .and_then(|id| i32::try_from(id).ok())
                        .and_then(|id| WorkspaceRef::numbered(id).ok())
                } else {
                    WorkspaceRef::named(address).ok()
                }
            }
            Some("special") => decode_workspace_address(address, Some("special")),
            None | Some("") => decode_workspace_address(address, None),
            Some(_) => None,
        };
    }
    let id = value["id"].as_i64().and_then(|id| i32::try_from(id).ok());
    if let Some(id) = id.filter(|id| *id > 0) {
        return WorkspaceRef::numbered(id).ok();
    }
    // An invalid/zero numeric identity cannot become a displayed title.
    if value.get("id").is_some() && !id.is_some_and(|id| id < 0) {
        return None;
    }
    let name = value["name"].as_str()?;
    if name.is_empty() {
        return None;
    }
    if value["type"].as_str() == Some("normal") {
        return WorkspaceRef::named(name).ok();
    }
    if name == "special"
        || name.starts_with("special:")
        || value["type"].as_str() == Some("special")
    {
        decode_workspace_address(name, Some("special"))
    } else {
        WorkspaceRef::named(name).ok()
    }
}
fn decode_workspace_address(address: &str, kind: Option<&str>) -> Option<WorkspaceRef> {
    if matches!(address, "special" | "special:special") {
        return WorkspaceRef::special("").ok();
    }
    if let Some(name) = address.strip_prefix("special:") {
        return WorkspaceRef::special(name).ok();
    }
    if kind == Some("special") {
        return WorkspaceRef::special(address).ok();
    }
    if let Some(name) = address.strip_prefix("name:") {
        return WorkspaceRef::named(name).ok();
    }
    if address.as_bytes().iter().all(u8::is_ascii_digit) {
        return address
            .parse::<i32>()
            .ok()
            .and_then(|id| WorkspaceRef::numbered(id).ok());
    }
    WorkspaceRef::named(address).ok()
}
/// Encode values as Lua string literals, never as source expressions. Names
/// can contain quotes/backslashes; validated controls can never enter a request.
fn lua_string(value: &str) -> String {
    let mut result = String::with_capacity(value.len() + 2);
    result.push('"');
    for character in value.chars() {
        if matches!(character, '"' | '\\') {
            result.push('\\');
        }
        result.push(character);
    }
    result.push('"');
    result
}
fn workspace_request(command: WorkspaceCommand) -> Result<String> {
    match command {
        WorkspaceCommand::Select(id) if !id.is_special() => {
            // Current Hyprland forbids numeric names. Reject unsupported
            // capabilities without reinterpreting the named domain value.
            if id.name().is_some_and(|name| name.parse::<i64>().is_ok()) {
                return Err(DomainError::Invalid(
                    "Hyprland does not support numeric named workspaces".into(),
                ));
            }
            let address = id
                .number()
                .map(|number| number.to_string())
                .unwrap_or_else(|| format!("name:{}", id.name().expect("named identity")));
            Ok(format!(
                "dispatch hl.dsp.focus({{ workspace = {} }})",
                lua_string(&address)
            ))
        }
        WorkspaceCommand::ToggleSpecial(id) if id.is_special() => Ok(format!(
            "dispatch hl.dsp.workspace.toggle_special({})",
            lua_string(id.name().expect("special identity"))
        )),
        _ => Err(DomainError::Invalid(
            "Workspace action and identity kind do not match".into(),
        )),
    }
}
fn decode_monitors(raw: &Value, clients: &Value) -> Vec<CompositorMonitor> {
    let mut result = std::collections::BTreeMap::new();
    let mut ambiguous = std::collections::BTreeSet::new();
    for value in raw.as_array().into_iter().flatten() {
        if value["disabled"].as_bool() == Some(true) {
            continue;
        }
        let Some(name) = value["name"].as_str() else {
            continue;
        };
        let Ok(id) = MonitorId::new(name) else {
            continue;
        };
        let workspace = |key: &str| decode_workspace_ref(&value[key]);
        let active_workspace = workspace("activeWorkspace");
        let special_workspace = workspace("specialWorkspace");
        let visible_workspace = special_workspace.as_ref().or(active_workspace.as_ref());
        // Hyprland's internal fullscreen mode 2 covers the output. Mode 1 is
        // maximized; fullscreenClient is the state advertised to the app, not
        // proof that the compositor actually fullscreened it. Never infer a
        // visible fullscreen window from focus alone or another workspace.
        let has_fullscreen_window = value["id"].as_u64().is_some_and(|native_id| {
            visible_workspace.is_some_and(|visible| {
                clients.as_array().into_iter().flatten().any(|client| {
                    client["monitor"].as_u64() == Some(native_id)
                        && client["mapped"].as_bool() == Some(true)
                        && client["hidden"].as_bool() == Some(false)
                        && client["visible"].as_bool() != Some(false)
                        && client["fullscreen"].as_u64() == Some(2)
                        && decode_workspace_ref(&client["workspace"]).as_ref() == Some(visible)
                })
            })
        });
        let monitor = CompositorMonitor {
            id: id.clone(),
            focused: value["focused"].as_bool().unwrap_or(false),
            active_workspace,
            special_workspace,
            has_fullscreen_window,
        };
        if result.insert(id.clone(), monitor).is_some() {
            ambiguous.insert(id);
        }
    }
    result.retain(|id, _| !ambiguous.contains(id));
    result.into_values().collect()
}

#[cfg(test)]
mod protocol_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fullscreen_uses_internal_mode_on_the_visible_workspace_of_the_correct_output() {
        let mut monitors = serde_json::json!([
            {"id":0,"name":"DP-1","focused":false,"activeWorkspace":{"id":1},"specialWorkspace":{"id":0}},
            {"id":1,"name":"DP-2","focused":true,"activeWorkspace":{"id":2},"specialWorkspace":{"id":0}}
        ]);
        let mut clients = serde_json::json!([
            {"monitor":0,"mapped":true,"hidden":false,"visible":true,"fullscreen":2,"fullscreenClient":0,"workspace":{"id":1}}
        ]);
        let fullscreen = |monitors: &Value, clients: &Value| {
            decode_monitors(monitors, clients)
                .into_iter()
                .filter(|m| m.has_fullscreen_window)
                .map(|m| m.id.as_str().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(fullscreen(&monitors, &clients), ["DP-1"]);
        for mode in [0, 1, 9] {
            clients[0]["fullscreen"] = mode.into();
            clients[0]["fullscreenClient"] = 2.into();
            assert!(fullscreen(&monitors, &clients).is_empty());
        }
        clients[0]["fullscreen"] = 2.into();
        clients[0]["workspace"] = serde_json::json!({"id":3});
        assert!(fullscreen(&monitors, &clients).is_empty());
        clients[0]["workspace"] = serde_json::json!({"id":1});
        for (field, value) in [("mapped", false), ("hidden", true), ("visible", false)] {
            let before = clients[0][field].clone();
            clients[0][field] = value.into();
            assert!(fullscreen(&monitors, &clients).is_empty());
            clients[0][field] = before;
        }
        // A special workspace is the visible foreground, not a reason to count
        // a fullscreen app on the normal workspace behind it.
        monitors[0]["specialWorkspace"] =
            serde_json::json!({"address":"special:notes","type":"special"});
        assert!(fullscreen(&monitors, &clients).is_empty());
        clients[0]["workspace"] = serde_json::json!({"id":-99,"name":"special:notes"});
        assert_eq!(fullscreen(&monitors, &clients), ["DP-1"]);
        clients[0]["monitor"] = 1.into();
        assert!(fullscreen(&monitors, &clients).is_empty());
    }
    #[test]
    fn modern_and_legacy_workspace_identity_agree_and_display_names_are_not_authority() {
        let values = [
            (
                serde_json::json!({"address":"9","id":9,"type":"normal","name":"Renamed"}),
                WorkspaceRef::numbered(9).unwrap(),
            ),
            (
                serde_json::json!({"address":"Writing","type":"normal","name":"Displayed title"}),
                WorkspaceRef::named("Writing").unwrap(),
            ),
            (
                serde_json::json!({"id":-1337,"name":"Writing"}),
                WorkspaceRef::named("Writing").unwrap(),
            ),
            (
                serde_json::json!({"address":"special:notes","type":"special","name":"Displayed title"}),
                WorkspaceRef::special("notes").unwrap(),
            ),
            (
                serde_json::json!({"id":-99,"name":"special:notes"}),
                WorkspaceRef::special("notes").unwrap(),
            ),
            (
                serde_json::json!({"id":-99,"name":"special"}),
                WorkspaceRef::special("").unwrap(),
            ),
        ];
        assert_eq!(
            decode_workspace_ref(&serde_json::json!({"address":"special:regular","type":"normal"})),
            Some(WorkspaceRef::named("special:regular").unwrap())
        );
        assert_eq!(
            decode_workspace_ref(&serde_json::json!({"address":"name:literal","type":"normal"})),
            Some(WorkspaceRef::named("name:literal").unwrap())
        );
        for (value, expected) in values {
            assert_eq!(decode_workspace_ref(&value), Some(expected));
        }
        for value in [
            serde_json::json!({"address":"","id":9,"name":"No fallback"}),
            serde_json::json!({"address":false,"id":9}),
            serde_json::json!({"id":0,"name":"No fallback"}),
            serde_json::json!({"id":4294967296u64,"name":"No narrowing"}),
            serde_json::json!({"address":"name:"}),
            serde_json::json!({"address":"4294967296"}),
            serde_json::json!({"address":"bad\naddress"}),
        ] {
            assert!(decode_workspace_ref(&value).is_none());
        }
    }
    #[test]
    fn named_active_window_membership_and_ambiguous_records_remain_typed() {
        let active = WorkspaceRef::named("Writing").unwrap();
        let snapshot = decode_snapshot(
            Some(&active),
            "",
            &serde_json::json!([
                {"address":"name:Writing","name":"Display","windows":1},
                {"id":9,"name":"first"},{"id":9,"name":"duplicate"},{"id":0}
            ]),
            &serde_json::json!([
                {"address":"0xabc","workspace":{"address":"name:Writing"}},
                {"address":"0xdef","workspace":{"id":0}}
            ]),
            &serde_json::json!([]),
        );
        assert_eq!(snapshot.workspaces.len(), 1);
        assert_eq!(snapshot.workspaces[0].id, active);
        assert!(snapshot.workspaces[0].active);
        assert_eq!(snapshot.windows[0].workspace, Some(active));
        assert!(snapshot.windows[1].workspace.is_none());
        assert!(snapshot.windows.iter().all(|window| !window.focused));
    }
    #[test]
    fn workspace_action_kinds_and_literal_encoding_do_not_interpret_names_as_source() {
        assert_eq!(
            workspace_request(WorkspaceCommand::Select(WorkspaceRef::numbered(9).unwrap()))
                .unwrap(),
            r#"dispatch hl.dsp.focus({ workspace = "9" })"#
        );
        let name = r#"Writing "; os.execute('bad') -- \"#;
        let encoded = lua_string(&format!("name:{name}"));
        assert_eq!(
            workspace_request(WorkspaceCommand::Select(WorkspaceRef::named(name).unwrap()))
                .unwrap(),
            format!("dispatch hl.dsp.focus({{ workspace = {encoded} }})")
        );
        assert_eq!(lua_string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(
            workspace_request(WorkspaceCommand::ToggleSpecial(
                WorkspaceRef::special("notes").unwrap()
            ))
            .unwrap(),
            r#"dispatch hl.dsp.workspace.toggle_special("notes")"#
        );
        assert!(
            workspace_request(WorkspaceCommand::Select(
                WorkspaceRef::special("notes").unwrap()
            ))
            .is_err()
        );
        assert!(
            workspace_request(WorkspaceCommand::ToggleSpecial(
                WorkspaceRef::named("notes").unwrap()
            ))
            .is_err()
        );
    }
    #[test]
    fn monitor_metadata_preserves_numbered_and_named_workspace_references() {
        let monitors = decode_monitors(
            &serde_json::json!([
                {"name":"DP-1","focused":true,"activeWorkspace":{"id":9,"name":"Work"},"specialWorkspace":{"id":0,"name":""}},
                {"name":"DP-2","focused":false,"activeWorkspace":{"address":"name:Writing","name":"Displayed title"},"specialWorkspace":{"address":"special:notes","name":"Notes"}},
                {"name":"disabled","focused":true,"disabled":true},
                {"name":"bad\nname","focused":true}
            ]),
            &serde_json::json!([]),
        );
        assert_eq!(monitors.len(), 2);
        assert_eq!(
            monitors[0].active_workspace.as_ref().unwrap().number(),
            Some(9)
        );
        assert!(monitors[0].special_workspace.is_none());
        assert_eq!(
            monitors[1].active_workspace.as_ref().unwrap().name(),
            Some("Writing")
        );
        assert_eq!(
            monitors[1].special_workspace.as_ref().unwrap().name(),
            Some("notes")
        );
        let snapshot = CompositorSnapshot {
            monitors,
            ..Default::default()
        };
        assert_eq!(snapshot.focused_monitor().unwrap().as_str(), "DP-1");
    }
    #[test]
    fn duplicate_names_and_ambiguous_focus_do_not_select_an_output() {
        assert!(
            decode_monitors(
                &serde_json::json!([{"name":"DP-1","focused":true},{"name":"DP-1","focused":true}]),
                &serde_json::json!([]),
            )
            .is_empty()
        );
        let snapshot = CompositorSnapshot {
            monitors: decode_monitors(
                &serde_json::json!([{"name":"DP-1","focused":true},{"name":"DP-2","focused":true}]),
                &serde_json::json!([]),
            ),
            ..Default::default()
        };
        assert!(snapshot.focused_monitor().is_none());
        assert!(decode_workspace_ref(&serde_json::json!({"id":4294967296u64,"name":""})).is_none());
    }
    #[test]
    fn adapter_reports_actual_workspaces_including_special_without_inventing_shortcuts() {
        let snapshot = decode_snapshot(
            Some(&WorkspaceRef::numbered(9).unwrap()),
            "0xabc",
            &serde_json::json!([
                {"id":9,"name":"Work","windows":2},
                {"id":-99,"name":"special:scratchpad","windows":1}
            ]),
            &serde_json::json!([
                {"address":"0xabc","class":"editor","title":"Work","workspace":{"id":9}}
            ]),
            &serde_json::json!([]),
        );
        assert_eq!(
            snapshot
                .workspaces
                .iter()
                .map(|w| w.id.clone())
                .collect::<Vec<_>>(),
            [
                WorkspaceRef::numbered(9).unwrap(),
                WorkspaceRef::special("scratchpad").unwrap()
            ]
        );
        assert!(snapshot.workspaces[0].active);
        assert_eq!(snapshot.workspaces[0].name, "Work");
        assert!(snapshot.windows[0].focused);
        assert!(
            decode_snapshot(
                Some(&WorkspaceRef::numbered(1).unwrap()),
                "",
                &serde_json::json!([]),
                &serde_json::json!([]),
                &serde_json::json!([])
            )
            .workspaces
            .is_empty()
        );
    }
}
