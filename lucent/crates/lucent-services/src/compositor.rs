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
        let active = self.json_before("j/activeworkspace", deadline)?["id"]
            .as_i64()
            .and_then(|id| i32::try_from(id).ok())
            .unwrap_or(0);
        let focused = self.json_before("j/activewindow", deadline)?["address"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let ws = self.json_before("j/workspaces", deadline)?;
        let clients = self.json_before("j/clients", deadline)?;
        let monitors = self.json_before("j/monitors", deadline)?;
        Ok(decode_snapshot(active, &focused, &ws, &clients, &monitors))
    }

    fn switch_workspace(&self, id: i32) -> Result<()> {
        if id < 1 {
            return Err(DomainError::Invalid("Invalid workspace".into()));
        }
        let result = self.request(&format!(
            "dispatch hl.dsp.focus({{ workspace = \"{id}\" }})"
        ))?;
        if result.trim() == "ok" {
            Ok(())
        } else {
            Err(DomainError::Failed(result))
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
    active: i32,
    focused: &str,
    ws: &Value,
    clients: &Value,
    raw_monitors: &Value,
) -> CompositorSnapshot {
    let monitors = decode_monitors(raw_monitors);
    let mut workspaces: Vec<_> = ws
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| Workspace {
            id: v["id"].as_i64().unwrap_or(0) as i32,
            name: v["name"].as_str().unwrap_or("").into(),
            windows: v["windows"].as_u64().unwrap_or(0) as usize,
            active: v["id"].as_i64() == Some(active as i64),
        })
        .collect();
    workspaces.sort_by_key(|w| w.id);
    let windows = clients
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            let address = v["address"].as_str().unwrap_or("").to_owned();
            Window {
                focused: address == focused,
                address,
                app_class: v["class"].as_str().unwrap_or("").into(),
                title: v["title"].as_str().unwrap_or("").into(),
                workspace: v["workspace"]["id"].as_i64().unwrap_or(0) as i32,
            }
        })
        .collect();
    CompositorSnapshot {
        workspaces,
        windows,
        monitors,
    }
}

fn decode_workspace_ref(value: &Value) -> Option<WorkspaceRef> {
    if let Some(id) = value["id"]
        .as_i64()
        .and_then(|id| i32::try_from(id).ok())
        .filter(|id| *id != 0)
    {
        return WorkspaceRef::numbered(id).ok();
    }
    let address = value["address"]
        .as_str()
        .filter(|name| !name.is_empty())
        .or_else(|| value["name"].as_str().filter(|name| !name.is_empty()))?;
    WorkspaceRef::named(address).ok()
}
fn decode_monitors(raw: &Value) -> Vec<CompositorMonitor> {
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
        let monitor = CompositorMonitor {
            id: id.clone(),
            focused: value["focused"].as_bool().unwrap_or(false),
            active_workspace: workspace("activeWorkspace"),
            special_workspace: workspace("specialWorkspace"),
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
    fn monitor_metadata_preserves_numbered_and_named_workspace_references() {
        let monitors = decode_monitors(&serde_json::json!([
            {"name":"DP-1","focused":true,"activeWorkspace":{"id":9,"name":"Work"},"specialWorkspace":{"id":0,"name":""}},
            {"name":"DP-2","focused":false,"activeWorkspace":{"address":"name:Writing","name":"Displayed title"},"specialWorkspace":{"address":"special:notes","name":"Notes"}},
            {"name":"disabled","focused":true,"disabled":true},
            {"name":"bad\nname","focused":true}
        ]));
        assert_eq!(monitors.len(), 2);
        assert_eq!(
            monitors[0].active_workspace.as_ref().unwrap().number(),
            Some(9)
        );
        assert!(monitors[0].special_workspace.is_none());
        assert_eq!(
            monitors[1].active_workspace.as_ref().unwrap().address(),
            Some("name:Writing")
        );
        assert_eq!(
            monitors[1].special_workspace.as_ref().unwrap().address(),
            Some("special:notes")
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
                &serde_json::json!([{"name":"DP-1","focused":true},{"name":"DP-1","focused":true}])
            )
            .is_empty()
        );
        let snapshot = CompositorSnapshot {
            monitors: decode_monitors(
                &serde_json::json!([{"name":"DP-1","focused":true},{"name":"DP-2","focused":true}]),
            ),
            ..Default::default()
        };
        assert!(snapshot.focused_monitor().is_none());
        assert!(decode_workspace_ref(&serde_json::json!({"id":4294967296u64,"name":""})).is_none());
    }
    #[test]
    fn adapter_reports_actual_workspaces_including_special_without_inventing_shortcuts() {
        let snapshot = decode_snapshot(
            9,
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
            snapshot.workspaces.iter().map(|w| w.id).collect::<Vec<_>>(),
            [-99, 9]
        );
        assert!(snapshot.workspaces[1].active);
        assert_eq!(snapshot.workspaces[1].name, "Work");
        assert!(snapshot.windows[0].focused);
        assert!(
            decode_snapshot(
                1,
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
