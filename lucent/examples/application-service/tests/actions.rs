//! Exercise the independent consumer without a desktop, real bus or app launch.
use std::{fs, path::PathBuf, process::Command};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("lucent-action-consumer-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("applications")).unwrap();
        fs::write(path.join("applications/fixture action.desktop"), "[Desktop Entry]\nType=Application\nName=Fixture\nDBusActivatable=true\nActions=New-Window;\n[Desktop Action New-Window]\nName=New window\n").unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_application-service"))
            .arg("--data-dir")
            .arg(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn independent_consumer_lists_declared_actions_and_admits_only_exact_identity() {
    let fixture = Fixture::new();
    let listed = fixture.run(&["--list"]);
    assert!(listed.status.success());
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(
        listed["items"][0]["actions"],
        serde_json::json!([{"id":"New-Window","name":"New window"}])
    );
    let wrong_case = fixture.run(&["--action", "fixture action.desktop", "new-window"]);
    assert!(!wrong_case.status.success());
    assert_eq!(
        String::from_utf8(wrong_case.stderr).unwrap().trim(),
        "Application action is no longer available"
    );
    let exact = fixture.run(&["--action", "fixture action.desktop", "New-Window"]);
    assert!(!exact.status.success());
    // The exact action reaches the adapter. Its deliberately non-native desktop
    // identity and absent Exec make it unavailable without consulting a bus.
    assert_eq!(
        String::from_utf8(exact.stderr).unwrap().trim(),
        "Application action has no available activation service"
    );
    let mixed = fixture.run(&["--list", "--action", "fixture action.desktop", "New-Window"]);
    assert!(!mixed.status.success());
    assert_eq!(
        String::from_utf8(mixed.stderr).unwrap().trim(),
        "Choose one application operation"
    );
}
