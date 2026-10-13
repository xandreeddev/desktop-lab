use super::*;
#[test]
fn catalog_explicit_source_rejects_relative_roots() {
    assert!(
        XdgApplications::with_source(vec!["relative".into()], "Hyprland".into(), String::new())
            .is_err()
    );
}
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("lucent-catalog-{}-{nonce}", std::process::id()));
        fs::create_dir(&p).unwrap();
        Self(p)
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
