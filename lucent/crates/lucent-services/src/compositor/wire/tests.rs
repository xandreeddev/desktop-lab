use super::*;
use std::{
    io::{Read, Write},
    os::unix::net::UnixListener,
    sync::mpsc,
    thread,
    time::Instant,
};
struct Socket {
    root: std::path::PathBuf,
    path: std::path::PathBuf,
}
impl Socket {
    fn new() -> (Self, UnixListener) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lucent-compositor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("command.sock");
        let listener = UnixListener::bind(&path).unwrap();
        (Self { root, path }, listener)
    }
}
impl Drop for Socket {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
fn event_fragments_and_bursts_preserve_frames_without_prefix_false_positives() {
    let mut events = Events::default();
    assert!(!events.push(b"focused").unwrap());
    assert!(!events.push(b"monv2>>DP-2,9").unwrap());
    assert!(events.push(b"\n").unwrap());
    assert!(
        !events
            .push(b"focusedmonUnexpected>>data\nunknown>>workspace\n")
            .unwrap()
    );
    assert!(
        events
            .push(b"workspacev2>>9,Work\nactivespecialv2>>special:notes\n")
            .unwrap()
    );
    assert!(!events.push(b"malformed\n").unwrap());
    assert!(events.push(b"activewindow>>\xff\n").unwrap());
    assert!(events.push(&vec![b'x'; EVENT_LIMIT + 1]).is_err());
}
#[test]
fn request_uses_literal_bytes_and_closes_after_the_bounded_response() {
    let (socket, listener) = Socket::new();
    let server = thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        let mut bytes = [0; 10];
        peer.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"j/monitors");
        peer.write_all(br#"[{"name":"DP-2","focused":true}]"#)
            .unwrap();
    });
    assert_eq!(
        request(&socket.path, "j/monitors").unwrap(),
        r#"[{"name":"DP-2","focused":true}]"#
    );
    server.join().unwrap();
}
#[test]
fn slow_peer_cannot_extend_an_absolute_request_deadline() {
    let (socket, listener) = Socket::new();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut request = [0; 1];
        peer.read_exact(&mut request).unwrap();
        tx.send(()).unwrap();
        for _ in 0..30 {
            if peer.write_all(b"x").is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(15));
        }
    });
    let start = Instant::now();
    assert_eq!(
        exchange(&socket.path, "q", Duration::from_millis(100), 1024)
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
    server.join().unwrap();
}
#[test]
fn oversized_response_is_rejected_and_transport_remains_usable() {
    let (socket, listener) = Socket::new();
    let server = thread::spawn(move || {
        for response in [b"oversized".as_slice(), b"ok"] {
            let (mut peer, _) = listener.accept().unwrap();
            let mut q = [0; 1];
            peer.read_exact(&mut q).unwrap();
            let _ = peer.write_all(response);
        }
    });
    assert_eq!(
        exchange(&socket.path, "q", Duration::from_secs(1), 2)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        exchange(&socket.path, "q", Duration::from_secs(1), 2).unwrap(),
        "ok"
    );
    server.join().unwrap();
}
#[test]
fn an_expired_snapshot_deadline_starts_no_new_connection() {
    let (socket, listener) = Socket::new();
    listener.set_nonblocking(true).unwrap();
    assert_eq!(
        request_before(
            &socket.path,
            "j/monitors",
            Instant::now() - Duration::from_millis(1)
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}
