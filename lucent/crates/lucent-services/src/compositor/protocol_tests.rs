//! Actual owned Unix sockets; no host compositor or graphical session is used.
use super::*;
use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Instant,
};
struct Stop(AtomicBool);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn wait(&self, duration: Duration) {
        if !self.cancelled() {
            thread::sleep(duration.min(Duration::from_millis(1)));
        }
    }
}
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn accept(listener: &UnixListener) -> UnixStream {
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < end =>
            {
                thread::sleep(Duration::from_millis(2))
            }
            Err(error) => panic!("Owned compositor fixture did not receive a connection: {error}"),
        }
    }
}
#[test]
fn watch_subscribes_before_snapshot_and_preserves_focus_event_split_across_timeout() {
    let directory = Directory(
        std::env::temp_dir().join(format!("lucent-compositor-watch-{}", std::process::id())),
    );
    std::fs::create_dir(&directory.0).unwrap();
    let commands = UnixListener::bind(directory.0.join(".socket.sock")).unwrap();
    let events = UnixListener::bind(directory.0.join(".socket2.sock")).unwrap();
    commands.set_nonblocking(true).unwrap();
    events.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        // This ordering detects an initial snapshot before event registration.
        let mut stream = accept(&events);
        for phase in 0..2 {
            for _ in 0..5 {
                let mut peer = accept(&commands);
                peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut buffer = [0; 128];
                let n = peer.read(&mut buffer).unwrap();
                let request = std::str::from_utf8(&buffer[..n]).unwrap();
                let response = match request {
                    "j/activeworkspace" => serde_json::json!({"id":9}),
                    "j/activewindow" => serde_json::json!({}),
                    "j/workspaces" => serde_json::json!([{"id":9,"name":"Work","windows":0}]),
                    "j/clients" => serde_json::json!([]),
                    "j/monitors" => {
                        serde_json::json!([{"name":if phase==0{"DP-1"}else{"DP-2"},"focused":true,"activeWorkspace":{"id":9},"specialWorkspace":{"id":0,"name":""}}])
                    }
                    _ => panic!("Unexpected owned query"),
                };
                peer.write_all(response.to_string().as_bytes()).unwrap();
            }
            if phase == 0 {
                stream.write_all(b"focused").unwrap();
                thread::sleep(Duration::from_millis(320));
                stream
                    .write_all(b"monv2>>DP-2,9\nactivespecialv2>>special:notes,DP-2\n")
                    .unwrap();
            }
        }
    });
    let adapter = Hyprland {
        base: directory.0.clone(),
    };
    let stop = Stop(AtomicBool::new(false));
    let mut snapshots = vec![];
    adapter.watch(
        &mut |snapshot| {
            if snapshot.is_err() || !snapshots.is_empty() {
                stop.0.store(true, Ordering::Release);
            }
            snapshots.push(snapshot);
        },
        &stop,
    );
    server.join().unwrap();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(
        snapshots[0]
            .as_ref()
            .unwrap()
            .focused_monitor()
            .unwrap()
            .as_str(),
        "DP-1"
    );
    assert_eq!(
        snapshots[1]
            .as_ref()
            .unwrap()
            .focused_monitor()
            .unwrap()
            .as_str(),
        "DP-2"
    );
}

#[test]
fn workspace_actions_use_owned_native_connection_and_invalid_kinds_send_nothing() {
    let directory = Directory(
        std::env::temp_dir().join(format!("lucent-compositor-actions-{}", std::process::id())),
    );
    std::fs::create_dir(&directory.0).unwrap();
    let listener = UnixListener::bind(directory.0.join(".socket.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let adapter = Hyprland {
        base: directory.0.clone(),
    };
    assert!(
        adapter
            .workspace(WorkspaceCommand::Select(
                WorkspaceRef::special("notes").unwrap()
            ))
            .is_err()
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    let server = thread::spawn(move || {
        let mut requests = vec![];
        for response in ["ok", "ok", "private native rejection"] {
            let mut peer = accept(&listener);
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buffer = [0; 4096];
            let n = peer.read(&mut buffer).unwrap();
            requests.push(String::from_utf8(buffer[..n].to_vec()).unwrap());
            peer.write_all(response.as_bytes()).unwrap();
        }
        requests
    });
    adapter
        .workspace(WorkspaceCommand::Select(
            WorkspaceRef::named("Writing room").unwrap(),
        ))
        .unwrap();
    adapter
        .workspace(WorkspaceCommand::ToggleSpecial(
            WorkspaceRef::special("notes").unwrap(),
        ))
        .unwrap();
    let error = adapter
        .workspace(WorkspaceCommand::Select(WorkspaceRef::numbered(9).unwrap()))
        .unwrap_err();
    assert_eq!(error.to_string(), "Hyprland rejected the workspace action");
    assert_eq!(
        server.join().unwrap(),
        [
            r#"dispatch hl.dsp.focus({ workspace = "name:Writing room" })"#,
            r#"dispatch hl.dsp.workspace.toggle_special("notes")"#,
            r#"dispatch hl.dsp.focus({ workspace = "9" })"#,
        ]
    );
}
