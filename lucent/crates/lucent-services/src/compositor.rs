use lucent_domain::*;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
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
        let run = || -> std::io::Result<String> {
            let mut socket = UnixStream::connect(self.base.join(".socket.sock"))?;
            socket.set_read_timeout(Some(Duration::from_secs(2)))?;
            socket.set_write_timeout(Some(Duration::from_secs(2)))?;
            socket.write_all(request.as_bytes())?;
            let mut result = String::new();
            socket.take(4 * 1024 * 1024).read_to_string(&mut result)?;
            Ok(result)
        };
        run().map_err(|e| DomainError::Unavailable(format!("Hyprland: {e}")))
    }
    fn json(&self, request: &str) -> Result<Value> {
        serde_json::from_str(&self.request(request)?)
            .map_err(|e| DomainError::Failed(e.to_string()))
    }
}
impl CompositorPort for Hyprland {
    fn watch(&self, emit: &mut dyn FnMut(Result<CompositorSnapshot>), cancel: &dyn StopSignal) {
        while !cancel.cancelled() {
            emit(self.snapshot());
            if let Ok(socket) = UnixStream::connect(self.base.join(".socket2.sock")) {
                let _ = socket.set_read_timeout(Some(Duration::from_millis(500)));
                let mut reader = BufReader::new(socket);
                let mut line = String::new();
                while !cancel.cancelled() {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            if [
                                "workspace",
                                "focusedmon",
                                "activewindow",
                                "openwindow",
                                "closewindow",
                                "movewindow",
                                "createworkspace",
                                "destroyworkspace",
                                "monitor",
                            ]
                            .iter()
                            .any(|s| line.starts_with(s))
                            {
                                emit(self.snapshot());
                            }
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(_) => break,
                    }
                }
            }
            cancel.wait(Duration::from_secs(2));
        }
    }
    fn snapshot(&self) -> Result<CompositorSnapshot> {
        let active = self.json("j/activeworkspace")?["id"].as_i64().unwrap_or(1) as i32;
        let focused = self.json("j/activewindow")?["address"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let ws = self.json("j/workspaces")?;
        let clients = self.json("j/clients")?;
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
            .filter(|w| w.id > 0)
            .collect();
        for id in 1..=6 {
            if !workspaces.iter().any(|w| w.id == id) {
                workspaces.push(Workspace {
                    id,
                    name: id.to_string(),
                    windows: 0,
                    active: id == active,
                });
            }
        }
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
        Ok(CompositorSnapshot {
            workspaces,
            windows,
        })
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
        if !address.starts_with("0x") || !address[2..].chars().all(|c| c.is_ascii_hexdigit()) {
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
