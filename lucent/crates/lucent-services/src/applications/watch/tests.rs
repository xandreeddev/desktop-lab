use super::*;
#[test]
fn catalog_explicit_source_rejects_relative_roots() {
    assert!(
        XdgApplications::with_source(vec!["relative".into()], "Hyprland".into(), String::new())
            .is_err()
    );
}
use std::{
    os::unix::fs::DirBuilderExt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..1024 {
            let path = std::env::temp_dir().join(format!(
                "lucent-catalog-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("Catalog fixture directory creation failed: {error}"),
            }
        }
        panic!("Catalog fixture directory allocation exhausted");
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Stop(AtomicBool);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn wait(&self, d: Duration) {
        let end = Instant::now() + d;
        while !self.cancelled() && Instant::now() < end {
            thread::sleep(Duration::from_millis(10));
        }
    }
}
struct Observer {
    stop: Arc<Stop>,
    receive: mpsc::Receiver<Result<Vec<Application>>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Observer {
    fn new(source: XdgApplications) -> Self {
        let stop = Arc::new(Stop(AtomicBool::new(false)));
        let s = stop.clone();
        let (send, receive) = mpsc::channel();
        let worker = thread::spawn(move || {
            source.watch(
                &mut |value| {
                    let _ = send.send(value);
                },
                s.as_ref(),
            )
        });
        Self {
            stop,
            receive,
            worker: Some(worker),
        }
    }
    fn state(&self, predicate: impl Fn(&[Application]) -> bool) -> Vec<Application> {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let value = self
                .receive
                .recv_timeout(end.saturating_duration_since(Instant::now()))
                .unwrap()
                .unwrap();
            if predicate(&value) {
                return value;
            }
        }
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        self.stop.0.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn entry(name: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName={name}\nExec=/usr/bin/true\n")
}
#[test]
fn catalog_kernel_special_files_do_not_block_and_still_mask_lower_entries() {
    use std::os::unix::fs::OpenOptionsExt;
    let directory = Directory::new();
    let high = directory.0.join("high");
    let low = directory.0.join("low");
    fs::create_dir_all(high.join("applications")).unwrap();
    fs::create_dir_all(low.join("applications")).unwrap();
    fs::write(low.join("applications/example.desktop"), entry("Low")).unwrap();
    let fifo = high.join("applications/example.desktop");
    let path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let source =
        XdgApplications::with_source(vec![high, low], "Hyprland".into(), String::new()).unwrap();
    let (send, receive) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result = source.discover();
        let _ = send.send(());
        result
    });
    let completed = receive.recv_timeout(Duration::from_secs(2)).is_ok();
    if !completed {
        // Release a regressed blocking reader before joining/reporting failure.
        drop(
            fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(fifo),
        );
    }
    let apps = worker.join().unwrap().unwrap();
    assert!(
        completed,
        "Discovery blocked on a non-regular desktop entry"
    );
    assert!(
        apps.is_empty(),
        "An invalid high-priority entry must mask the lower entry"
    );
}
#[test]
fn catalog_kernel_creates_updates_masks_deletes_and_recreates_roots() {
    let directory = Directory::new();
    let high = directory.0.join("high");
    let low = directory.0.join("low");
    fs::create_dir_all(low.join("applications")).unwrap();
    fs::write(low.join("applications/example.desktop"), entry("Low")).unwrap();
    let watch = Observer::new(
        XdgApplications::with_source(
            vec![high.clone(), low.clone()],
            "Hyprland".into(),
            String::new(),
        )
        .unwrap(),
    );
    assert_eq!(watch.state(|a| a.len() == 1)[0].name, "Low");
    fs::create_dir_all(high.join("applications/nested")).unwrap();
    fs::write(high.join("applications/example.desktop"), entry("High")).unwrap();
    fs::write(
        high.join("applications/nested/second.desktop"),
        entry("Second"),
    )
    .unwrap();
    let apps = watch.state(|a| a.len() == 2 && a.iter().any(|a| a.name == "High"));
    assert!(
        apps.iter()
            .any(|a| a.id.as_str() == "nested-second.desktop")
    );
    fs::write(
        high.join("applications/example.desktop"),
        "[Desktop Entry]\nType=Application\nHidden=true\n",
    )
    .unwrap();
    watch.state(|a| a.len() == 1 && a[0].name == "Second");
    fs::remove_file(high.join("applications/example.desktop")).unwrap();
    watch.state(|a| a.len() == 2 && a.iter().any(|a| a.name == "Low"));
    fs::rename(high.join("applications"), high.join("old-applications")).unwrap();
    watch.state(|a| a.len() == 1 && a[0].name == "Low");
    fs::create_dir(high.join("applications")).unwrap();
    fs::write(high.join("applications/third.desktop"), entry("Third")).unwrap();
    watch.state(|a| a.len() == 2 && a.iter().any(|a| a.name == "Third"));
}
#[test]
fn catalog_idle_has_no_duplicate_publications_and_stop_is_bounded() {
    let directory = Directory::new();
    let watch = Observer::new(
        XdgApplications::with_source(vec![directory.0.clone()], "Hyprland".into(), String::new())
            .unwrap(),
    );
    watch.state(|a| a.is_empty());
    assert!(matches!(
        watch.receive.recv_timeout(Duration::from_millis(350)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    let before = Instant::now();
    drop(watch);
    assert!(before.elapsed() < Duration::from_secs(1));
}
#[test]
fn catalog_observes_absolute_tryexec_and_icon_content_without_descriptor_changes() {
    let directory = Directory::new();
    let apps = directory.0.join("applications");
    let executable = directory.0.join("future/tools/bin/owned-fixture");
    let icon = directory.0.join("artwork/owned.svg");
    fs::create_dir(&apps).unwrap();
    fs::create_dir(icon.parent().unwrap()).unwrap();
    fs::write(&icon, "first").unwrap();
    fs::write(apps.join("owned.desktop"), format!("[Desktop Entry]\nType=Application\nName=Owned\nExec=/usr/bin/true\nTryExec={}\nIcon={}\n", executable.display(), icon.display())).unwrap();
    let source =
        XdgApplications::with_source(vec![directory.0.clone()], "Hyprland".into(), String::new())
            .unwrap();
    let stop = Arc::new(Stop(AtomicBool::new(false)));
    let worker_stop = stop.clone();
    let (send, receive) = mpsc::channel();
    let worker = thread::spawn(move || {
        source.watch_updates(
            &mut |update| {
                let _ = send.send(update);
            },
            worker_stop.as_ref(),
        )
    });
    struct Cleanup(Arc<Stop>, Option<thread::JoinHandle<()>>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.0.0.store(true, Ordering::Release);
            self.1.take().unwrap().join().unwrap();
        }
    }
    let _cleanup = Cleanup(stop, Some(worker));
    assert!(
        receive
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap()
            .applications
            .is_empty()
    );
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, "owned nonexecuted fixture").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let update = receive
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
            .unwrap();
        if update.applications.len() == 1 {
            break;
        }
    }
    fs::write(&icon, "changed artwork, identical descriptor").unwrap();
    let update = receive
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(update.applications.len(), 1);
    assert!(update.artwork_changed);
    fs::remove_file(&executable).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let update = receive
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
            .unwrap();
        if update.applications.is_empty() {
            break;
        }
    }
}

fn executable_fixture(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "owned executable metadata fixture; never executed").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
fn captured_source(directory: &Directory, program_dirs: Vec<PathBuf>) -> XdgApplications {
    fs::create_dir_all(directory.0.join("applications")).unwrap();
    fs::write(
        directory.0.join("applications/owned.desktop"),
        "[Desktop Entry]\nType=Application\nName=Owned\nExec=/usr/bin/true\nTryExec=owned-fixture\n",
    )
    .unwrap();
    let mut source =
        XdgApplications::with_source(vec![directory.0.clone()], "Hyprland".into(), String::new())
            .unwrap();
    // No process-global PATH mutation: parser and observer consume this exact
    // immutable search path, including currently missing candidate directories.
    source.program_dirs = program_dirs;
    source
}

#[test]
fn discovery_and_current_descriptor_resolution_share_captured_executable_lookup() {
    let directory = Directory::new();
    let programs = directory.0.join("private-programs");
    executable_fixture(&programs.join("owned-fixture"));
    let source = captured_source(&directory, vec![programs.clone()]);
    let applications = source.discover().unwrap();
    assert_eq!(applications.len(), 1);
    assert_eq!(
        catalog::resolve(&source, &applications[0].id).unwrap().0,
        applications[0]
    );
    fs::set_permissions(
        programs.join("owned-fixture"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(source.discover().unwrap().is_empty());
    assert!(catalog::resolve(&source, &applications[0].id).is_err());
}

#[test]
fn kernel_artwork_invalidation_matches_the_icon_child_without_executable_or_sibling_noise() {
    let directory = Directory::new();
    let programs = directory.0.join("programs");
    let executable = programs.join("owned-fixture");
    executable_fixture(&executable);
    let source = captured_source(&directory, vec![programs.clone()]);
    let icon = programs.join("z-artwork/icon.svg");
    fs::create_dir(icon.parent().unwrap()).unwrap();
    fs::write(&icon, "initial owned artwork").unwrap();
    let desktop = directory.0.join("applications/owned.desktop");
    let descriptor = format!(
        "{}Icon={}\n",
        fs::read_to_string(&desktop).unwrap(),
        icon.display()
    );
    fs::write(&desktop, &descriptor).unwrap();
    let mut watcher = Watcher::new().unwrap();
    watcher.refresh(&source).unwrap();

    fs::write(directory.0.join("unrelated-sibling"), "unrelated").unwrap();
    let changed = watcher.changed().unwrap();
    assert!(!changed.any && !changed.artwork);

    // One read drains both events. An irrelevant sibling must not contribute
    // artwork=true just because the executable contributes any=true.
    fs::write(directory.0.join("unrelated-sibling"), "changed sibling").unwrap();
    fs::write(&executable, "same executable eligibility; changed bytes").unwrap();
    let changed = watcher.changed().unwrap();
    assert!(changed.any && !changed.artwork);

    fs::write(&icon, "changed owned artwork; descriptor is identical").unwrap();
    let changed = watcher.changed().unwrap();
    assert!(changed.any && changed.artwork);
    assert_eq!(fs::read_to_string(desktop).unwrap(), descriptor);

    // The executable registers shared prefixes first with artwork=false; the
    // icon must promote those interests rather than lose its invalidation.
    fs::rename(&programs, directory.0.join("retired-programs")).unwrap();
    let changed = watcher.changed().unwrap();
    assert!(changed.any && changed.artwork);
}

#[test]
fn a_shared_executable_and_icon_dependency_keeps_artwork_invalidation() {
    let directory = Directory::new();
    let programs = directory.0.join("programs");
    let executable = programs.join("owned-fixture");
    executable_fixture(&executable);
    let source = captured_source(&directory, vec![programs]);
    let desktop = directory.0.join("applications/owned.desktop");
    let descriptor = format!(
        "{}Icon={}\n",
        fs::read_to_string(&desktop).unwrap(),
        executable.display()
    );
    fs::write(&desktop, descriptor).unwrap();
    let dependencies = catalog::dependency_files(&source).unwrap();
    assert_eq!(
        dependencies.iter().find(|(path, _)| path == &executable),
        Some(&(executable.clone(), true))
    );
    let mut watcher = Watcher::new().unwrap();
    watcher.refresh(&source).unwrap();
    fs::write(&executable, "changed bytes of shared executable/icon").unwrap();
    let changed = watcher.changed().unwrap();
    assert!(changed.any && changed.artwork);
}

#[test]
fn relative_tryexec_observes_every_captured_candidate_and_dangling_file_target_recovery() {
    use std::os::unix::fs::symlink;
    let directory = Directory::new();
    let first = directory.0.join("search/first");
    let second = directory.0.join("search/second");
    fs::create_dir_all(&first).unwrap();
    symlink(
        "../../payload/release/bin/tool",
        first.join("owned-fixture"),
    )
    .unwrap();
    let target = directory.0.join("payload/release/bin/tool");
    let source = captured_source(&directory, vec![first.clone(), second.clone()]);
    let watcher = Observer::new(source);
    watcher.state(|apps| apps.is_empty());

    // The first PATH candidate exists as a dangling symlink. A later candidate
    // must still become eligible without touching the desktop descriptor.
    executable_fixture(&second.join("owned-fixture"));
    watcher.state(|apps| apps.len() == 1);
    fs::remove_file(second.join("owned-fixture")).unwrap();
    watcher.state(|apps| apps.is_empty());

    // This destination did not exist during registration and is outside PATH.
    executable_fixture(&target);
    watcher.state(|apps| apps.len() == 1);
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    watcher.state(|apps| apps.is_empty());
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
    watcher.state(|apps| apps.len() == 1);
    fs::remove_file(&target).unwrap();
    watcher.state(|apps| apps.is_empty());
    executable_fixture(&target);
    watcher.state(|apps| apps.len() == 1);

    // Replacing a normal directory prefix also has to retire the old target.
    fs::rename(
        directory.0.join("payload/release"),
        directory.0.join("payload/retired"),
    )
    .unwrap();
    watcher.state(|apps| apps.is_empty());
    executable_fixture(&target);
    watcher.state(|apps| apps.len() == 1);
}

#[test]
fn relative_tryexec_tracks_symlink_prefix_retargeting_and_cycle_repair() {
    use std::os::unix::fs::symlink;
    let directory = Directory::new();
    let path = directory.0.join("search/bin");
    fs::create_dir_all(&path).unwrap();
    fs::create_dir_all(directory.0.join("payload")).unwrap();
    symlink("../../payload/current/bin/tool", path.join("owned-fixture")).unwrap();
    // The destination itself contains a symlink prefix. Retain both prefixes
    // and the expanded missing destination, rather than canonicalize-and-drop.
    symlink("releases/first", directory.0.join("payload/current")).unwrap();
    let source = captured_source(&directory, vec![path.clone()]);
    let watcher = Observer::new(source);
    watcher.state(|apps| apps.is_empty());
    executable_fixture(&directory.0.join("payload/releases/first/bin/tool"));
    watcher.state(|apps| apps.len() == 1);

    fs::remove_file(directory.0.join("payload/current")).unwrap();
    symlink("releases/second", directory.0.join("payload/current")).unwrap();
    watcher.state(|apps| apps.is_empty());
    executable_fixture(&directory.0.join("payload/releases/second/bin/tool"));
    watcher.state(|apps| apps.len() == 1);

    fs::remove_file(path.join("owned-fixture")).unwrap();
    symlink("loop", path.join("owned-fixture")).unwrap();
    symlink("owned-fixture", path.join("loop")).unwrap();
    watcher.state(|apps| apps.is_empty());
    fs::remove_file(path.join("loop")).unwrap();
    executable_fixture(&path.join("loop"));
    watcher.state(|apps| apps.len() == 1);
}
