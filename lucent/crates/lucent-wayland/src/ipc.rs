//! Owned command transport. Dropping it stops and joins the listener on every
//! runtime exit path, independently of whether its filesystem name still exists.
use calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction, generic::Generic};
use lucent_api::Cancellation;
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::mpsc::{self, SyncSender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) struct Command {
    pub text: String,
    pub reply: SyncSender<String>,
}
struct SocketFile {
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl Drop for SocketFile {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.dev() == self.device && m.ino() == self.inode)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(super) struct Server {
    cancel: Cancellation,
    signal: LoopSignal,
    thread: Option<JoinHandle<io::Result<()>>>,
    // Dropped after the worker has released its listener, while the runtime still
    // holds its instance lock. An unrelated replacement socket is not removed.
    _socket: SocketFile,
}
impl Server {
    /// The caller holds the application instance lock and removes any stale name.
    pub fn bind(
        path: &Path,
        deliver: impl FnMut(Command, Cancellation) -> bool + Send + 'static,
    ) -> io::Result<Self> {
        let listener = UnixListener::bind(path)?;
        let metadata = fs::symlink_metadata(path)?;
        let socket = SocketFile {
            path: path.to_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let (ready, started) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("lucent-ipc".into())
            .spawn(move || {
                let setup = prepare(listener, deliver, worker_cancel.clone());
                let mut event_loop = match setup {
                    Ok(event_loop) => event_loop,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return Ok(());
                    }
                };
                if ready.send(Ok(event_loop.get_signal())).is_err() {
                    return Ok(());
                }
                // EventLoop::run resets its stop flag. An explicit cancellation check
                // preserves a stop requested immediately after the startup handshake.
                while !worker_cancel.cancelled() {
                    event_loop
                        .dispatch(None, &mut ())
                        .map_err(|e| io::Error::other(e.to_string()))?;
                }
                Ok(())
            })?;
        match started.recv() {
            Ok(Ok(signal)) => Ok(Self {
                cancel,
                signal,
                thread: Some(thread),
                _socket: socket,
            }),
            result => {
                let _ = thread.join();
                Err(match result {
                    Ok(Err(error)) => error,
                    _ => io::Error::other("IPC worker failed during startup"),
                })
            }
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.signal.wakeup();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn prepare(
    listener: UnixListener,
    mut deliver: impl FnMut(Command, Cancellation) -> bool + 'static,
    cancel: Cancellation,
) -> io::Result<EventLoop<'static, ()>> {
    let event_loop = EventLoop::try_new().map_err(|e| io::Error::other(e.to_string()))?;
    event_loop
        .handle()
        .insert_source(
            Generic::new(listener, Interest::READ, Mode::Level),
            move |_, listener, _| {
                // No timer or accept polling while idle. Pending commands are handled
                // serially, with cancellation checked between connections and waits.
                while !cancel.cancelled() {
                    let stream = match listener.accept() {
                        Ok((stream, _)) => stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) => return Err(error),
                    };
                    serve(stream, &cancel, &mut deliver);
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(event_loop)
}
const IO_POLL: Duration = Duration::from_millis(25);
const IO_DEADLINE: Duration = Duration::from_secs(1);
const MAX_COMMAND_BYTES: usize = 4096;

fn read_command(stream: &mut UnixStream, cancel: &Cancellation) -> Option<String> {
    let deadline = Instant::now() + IO_DEADLINE;
    let mut bytes = Vec::new();
    let mut buffer = [0; 512];
    while bytes.len() < MAX_COMMAND_BYTES {
        if cancel.cancelled() || Instant::now() >= deadline {
            return None;
        }
        let capacity = buffer.len().min(MAX_COMMAND_BYTES - bytes.len());
        match stream.read(&mut buffer[..capacity]) {
            Ok(0) => break,
            Ok(count) => {
                let end = buffer[..count].iter().position(|byte| *byte == b'\n');
                bytes.extend_from_slice(&buffer[..end.map_or(count, |index| index + 1)]);
                if end.is_some() {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return None,
        }
    }
    if cancel.cancelled() {
        None
    } else {
        String::from_utf8(bytes).ok()
    }
}
fn write_response(stream: &mut UnixStream, mut response: String, cancel: &Cancellation) {
    response.push('\n');
    let mut remaining = response.as_bytes();
    let deadline = Instant::now() + IO_DEADLINE;
    let mut attempted = false;
    while !remaining.is_empty() {
        if (attempted && cancel.cancelled()) || Instant::now() >= deadline {
            return;
        }
        // A ready reply may acknowledge the command that requested shutdown.
        // Give it one bounded write even when cancellation won the scheduling
        // race; stop before further writes if the peer cannot accept it.
        attempted = true;
        match stream.write(remaining) {
            Ok(0) => return,
            Ok(count) => remaining = &remaining[count..],
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return,
        }
    }
}

fn serve(
    mut stream: UnixStream,
    cancel: &Cancellation,
    deliver: &mut impl FnMut(Command, Cancellation) -> bool,
) {
    // Short waits plus absolute deadlines apply only to an active connection.
    // A slow trickle of bytes must not extend teardown indefinitely.
    if stream.set_read_timeout(Some(IO_POLL)).is_err()
        || stream.set_write_timeout(Some(IO_POLL)).is_err()
    {
        return;
    }
    let Some(text) = read_command(&mut stream, cancel) else {
        return;
    };
    let (reply, result) = mpsc::sync_channel(1);
    if !deliver(
        Command {
            text: text.trim().into(),
            reply,
        },
        cancel.clone(),
    ) {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match result.recv_timeout(IO_POLL) {
            Ok(response) => {
                write_response(&mut stream, response, cancel);
                return;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout)
                if cancel.cancelled() || Instant::now() >= deadline =>
            {
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    if !cancel.cancelled() {
        write_response(&mut stream, "error: client not responding".into(), cancel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "lucent-ipc-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn socket(&self) -> PathBuf {
            self.0.join("test.sock")
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn normal_requests_and_early_error_cleanup_allow_immediate_retry() {
        let directory = Directory::new();
        let path = directory.socket();
        for _ in 0..8 {
            let attempt = || -> io::Result<()> {
                let _server = Server::bind(&path, |command, _| {
                    command.reply.send(format!("echo:{}", command.text)).is_ok()
                })?;
                assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
                let mut client = UnixStream::connect(&path)?;
                client.set_read_timeout(Some(Duration::from_secs(3)))?;
                writeln!(client, "inspect")?;
                let mut response = String::new();
                BufReader::new(client).read_line(&mut response)?;
                assert_eq!(response, "echo:inspect\n");
                // A question-mark return after setup must still join the worker.
                Err(io::Error::other("simulated runtime initialization error"))
            };
            assert!(attempt().is_err());
            assert!(!path.exists());
        }
        // Also cover cancellation immediately after the startup handshake, before
        // the worker necessarily reaches its first dispatch call.
        for _ in 0..16 {
            drop(Server::bind(&path, |_, _| true).unwrap());
        }
        assert!(!path.exists());
    }
    #[test]
    fn drop_wakes_an_unlinked_listener_and_preserves_a_replacement_socket() {
        let directory = Directory::new();
        let path = directory.socket();
        for replace in [false, true] {
            let server = Server::bind(&path, |_, _| true).unwrap();
            fs::remove_file(&path).unwrap();
            let replacement = replace.then(|| UnixListener::bind(&path).unwrap());
            let (finished, result) = mpsc::channel();
            thread::spawn(move || {
                drop(server);
                finished.send(()).unwrap();
            });
            result
                .recv_timeout(Duration::from_secs(3))
                .expect("unlinked listener did not stop");
            if replacement.is_some() {
                assert!(
                    UnixStream::connect(&path).is_ok(),
                    "replacement socket was removed"
                );
                fs::remove_file(&path).unwrap();
            } else {
                assert!(!path.exists());
            }
        }
    }
    #[test]
    fn drop_cancels_delivery_even_when_the_runtime_queue_is_full() {
        let directory = Directory::new();
        let path = directory.socket();
        let (out, incoming, _runtime_lifetime) = crate::delivery::Delivery::channel();
        for n in 0..crate::delivery::CAPACITY {
            assert!(out.send(n));
        }
        let (entered, ready) = mpsc::channel();
        let server = Server::bind(&path, move |_, cancel| {
            entered.send(()).unwrap();
            out.scoped(cancel).send(999)
        })
        .unwrap();
        let mut client = UnixStream::connect(&path).unwrap();
        writeln!(client, "inspect").unwrap();
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
        let (finished, result) = mpsc::channel();
        thread::spawn(move || {
            drop(server);
            finished.send(()).unwrap();
        });
        result
            .recv_timeout(Duration::from_secs(3))
            .expect("full delivery blocked shutdown");
        for n in 0..crate::delivery::CAPACITY {
            assert_eq!(incoming.recv().unwrap(), n);
        }
        assert!(incoming.try_recv().is_err());
        assert!(!path.exists());
    }
    #[test]
    fn drop_releases_a_pending_reply_without_waiting_for_the_ui_thread() {
        let directory = Directory::new();
        let (held_reply, ready) = mpsc::channel();
        let server = Server::bind(&directory.socket(), move |command, _| {
            held_reply.send(command.reply).unwrap();
            true
        })
        .unwrap();
        let mut client = UnixStream::connect(directory.socket()).unwrap();
        writeln!(client, "inspect").unwrap();
        let _reply = ready.recv_timeout(Duration::from_secs(3)).unwrap();
        let (finished, result) = mpsc::channel();
        thread::spawn(move || {
            drop(server);
            finished.send(()).unwrap();
        });
        result
            .recv_timeout(Duration::from_secs(2))
            .expect("pending reply blocked shutdown");
        assert!(!directory.socket().exists());
    }
    #[test]
    fn incomplete_client_does_not_deliver_after_cancellation() {
        let directory = Directory::new();
        let count = Arc::new(AtomicUsize::new(0));
        let received = count.clone();
        let server = Server::bind(&directory.socket(), move |_, _| {
            received.fetch_add(1, Ordering::SeqCst);
            true
        })
        .unwrap();
        let mut client = UnixStream::connect(directory.socket()).unwrap();
        write!(client, "partial command").unwrap();
        let (finished, result) = mpsc::channel();
        thread::spawn(move || {
            drop(server);
            finished.send(()).unwrap();
        });
        result
            .recv_timeout(Duration::from_secs(3))
            .expect("partial input blocked shutdown");
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(!directory.socket().exists());
    }
    #[test]
    fn failed_bind_preserves_an_existing_socket() {
        let directory = Directory::new();
        let original = UnixListener::bind(directory.socket()).unwrap();
        assert!(Server::bind(&directory.socket(), |_, _| true).is_err());
        assert!(UnixStream::connect(directory.socket()).is_ok());
        drop(original);
    }
    #[test]
    fn trickling_input_cannot_extend_the_absolute_read_deadline() {
        let (mut receiver, mut sender) = UnixStream::pair().unwrap();
        receiver.set_read_timeout(Some(IO_POLL)).unwrap();
        let writing = thread::spawn(move || {
            for _ in 0..100 {
                if sender.write_all(b"x").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
        });
        let start = Instant::now();
        assert!(read_command(&mut receiver, &Cancellation::default()).is_none());
        assert!(start.elapsed() < Duration::from_secs(2));
        drop(receiver);
        writing.join().unwrap();
    }
    #[test]
    fn cancellation_interrupts_a_blocked_response_write() {
        let (mut writer, _reader) = UnixStream::pair().unwrap();
        writer.set_write_timeout(Some(IO_POLL)).unwrap();
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let (started, ready) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let worker = thread::spawn(move || {
            let response = "x".repeat(4 * 1024 * 1024);
            started.send(()).unwrap();
            write_response(&mut writer, response, &worker_cancel);
            done.send(()).unwrap();
        });
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(finished.recv_timeout(Duration::from_millis(60)).is_err());
        cancel.cancel();
        finished
            .recv_timeout(Duration::from_millis(700))
            .expect("cancelled write stayed blocked");
        worker.join().unwrap();
    }
    #[test]
    fn a_ready_quit_reply_is_not_lost_when_shutdown_starts() {
        let directory = Directory::new();
        let _server = Server::bind(&directory.socket(), |command, cancel| {
            command.reply.send("ok".into()).unwrap();
            cancel.cancel();
            true
        })
        .unwrap();
        let mut client = UnixStream::connect(directory.socket()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        writeln!(client, "quit").unwrap();
        let mut response = String::new();
        BufReader::new(client).read_line(&mut response).unwrap();
        assert_eq!(response, "ok\n");
    }
}
