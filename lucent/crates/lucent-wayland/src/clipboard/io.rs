//! Bounded nonblocking pipe steps, independent of the component and protocol roles.
use lucent_api::{CLIPBOARD_TEXT_LIMIT, ClipboardError, ClipboardOutcome};
use std::{
    fs::File,
    io::{Read, Write},
    time::{Duration, Instant},
};
pub(super) const DEADLINE: Duration = Duration::from_secs(2);
const CHUNK: usize = 4096;
const STEPS: usize = 8;
pub(super) enum Transfer {
    Read(Vec<u8>),
    Write {
        bytes: std::sync::Arc<[u8]>,
        offset: usize,
    },
}
impl Transfer {
    pub(super) fn interest(&self) -> calloop::Interest {
        match self {
            Self::Read(_) => calloop::Interest::READ,
            Self::Write { .. } => calloop::Interest::WRITE,
        }
    }
    pub(super) fn step(
        &mut self,
        file: &File,
        deadline: Instant,
        now: Instant,
    ) -> Option<ClipboardOutcome> {
        if now >= deadline {
            return Some(ClipboardOutcome::Error(ClipboardError::Timeout));
        }
        let mut pipe = file;
        for _ in 0..STEPS {
            let result = match self {
                Self::Read(bytes) => {
                    let mut chunk = [0; CHUNK];
                    match pipe.read(&mut chunk) {
                        Ok(0) => {
                            return Some(match String::from_utf8(std::mem::take(bytes)) {
                                Ok(text) => ClipboardOutcome::Text(text),
                                Err(_) => ClipboardOutcome::Error(ClipboardError::InvalidText),
                            });
                        }
                        Ok(n) => {
                            if bytes.len() + n > CLIPBOARD_TEXT_LIMIT {
                                return Some(ClipboardOutcome::Error(ClipboardError::TooLarge));
                            }
                            bytes.extend_from_slice(&chunk[..n]);
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
                Self::Write { bytes, offset } => {
                    if *offset == bytes.len() {
                        return Some(ClipboardOutcome::WritePosted);
                    }
                    let end = (*offset + CHUNK).min(bytes.len());
                    match pipe.write(&bytes[*offset..end]) {
                        Ok(0) => {
                            return Some(ClipboardOutcome::Error(ClipboardError::Disconnected));
                        }
                        Ok(n) => {
                            *offset += n;
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
            };
            if let Err(error) = result {
                match error.kind() {
                    std::io::ErrorKind::WouldBlock => return None,
                    std::io::ErrorKind::Interrupted => continue,
                    _ => return Some(ClipboardOutcome::Error(ClipboardError::Disconnected)),
                }
            }
        }
        None
    }
}
pub(super) fn nonblocking(file: &File) -> std::io::Result<()> {
    let flags = rustix::fs::fcntl_getfl(file)?;
    rustix::fs::fcntl_setfl(file, flags | rustix::fs::OFlags::NONBLOCK)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::{fd::OwnedFd, unix::net::UnixStream};
    fn pair() -> (File, File) {
        let (a, b) = UnixStream::pair().unwrap();
        let a = File::from(OwnedFd::from(a));
        let b = File::from(OwnedFd::from(b));
        nonblocking(&a).unwrap();
        nonblocking(&b).unwrap();
        (a, b)
    }
    fn deadline() -> Instant {
        Instant::now() + DEADLINE
    }
    #[test]
    fn partial_utf8_waits_for_eof_and_idle_pipe_returns_immediately() {
        let (read, write) = pair();
        let mut transfer = Transfer::Read(vec![]);
        let limit = deadline();
        assert_eq!(transfer.step(&read, limit, Instant::now()), None);
        (&write).write_all(&[0xc3]).unwrap();
        assert_eq!(transfer.step(&read, limit, Instant::now()), None);
        (&write).write_all(&[0xa9]).unwrap();
        drop(write);
        assert_eq!(
            transfer.step(&read, limit, Instant::now()),
            Some(ClipboardOutcome::Text("é".into()))
        );
    }
    #[test]
    fn invalid_and_oversized_content_is_rejected_without_returning_text() {
        let (read, write) = pair();
        (&write).write_all(&[0xff]).unwrap();
        drop(write);
        let mut transfer = Transfer::Read(vec![]);
        assert_eq!(
            transfer.step(&read, deadline(), Instant::now()),
            Some(ClipboardOutcome::Error(ClipboardError::InvalidText))
        );
        let (read, write) = pair();
        (&write).write_all(b"xy").unwrap();
        let mut transfer = Transfer::Read(vec![b'a'; CLIPBOARD_TEXT_LIMIT - 1]);
        assert_eq!(
            transfer.step(&read, deadline(), Instant::now()),
            Some(ClipboardOutcome::Error(ClipboardError::TooLarge))
        );
        if let Transfer::Read(bytes) = transfer {
            assert_eq!(bytes.len(), CLIPBOARD_TEXT_LIMIT - 1);
        }
    }
    #[test]
    fn absolute_deadline_cannot_be_extended_by_a_slow_sender() {
        let (read, write) = pair();
        let mut transfer = Transfer::Read(vec![]);
        let limit = deadline();
        (&write).write_all(b"a").unwrap();
        assert_eq!(
            transfer.step(&read, limit, limit - Duration::from_millis(1)),
            None
        );
        (&write).write_all(b"b").unwrap();
        assert_eq!(
            transfer.step(&read, limit, limit),
            Some(ClipboardOutcome::Error(ClipboardError::Timeout))
        );
    }
    #[test]
    fn dispatch_budget_yields_and_reads_exact_limit() {
        let (read, write) = pair();
        (&write)
            .write_all(&vec![b'a'; CLIPBOARD_TEXT_LIMIT])
            .unwrap();
        drop(write);
        let mut transfer = Transfer::Read(vec![]);
        let limit = deadline();
        assert_eq!(transfer.step(&read, limit, Instant::now()), None);
        if let Transfer::Read(bytes) = &transfer {
            assert_eq!(bytes.len(), STEPS * CHUNK);
        }
        assert_eq!(transfer.step(&read, limit, Instant::now()), None);
        assert_eq!(
            transfer.step(&read, limit, Instant::now()),
            Some(ClipboardOutcome::Text("a".repeat(CLIPBOARD_TEXT_LIMIT)))
        );
    }
    #[test]
    fn writes_are_partial_bounded_and_report_disconnected_consumers() {
        let (read, write) = pair();
        let bytes: std::sync::Arc<[u8]> = vec![b'a'; CLIPBOARD_TEXT_LIMIT].into();
        let mut transfer = Transfer::Write { bytes, offset: 0 };
        let limit = deadline();
        assert_eq!(transfer.step(&write, limit, Instant::now()), None);
        if let Transfer::Write { offset, .. } = &transfer {
            assert_eq!(*offset, STEPS * CHUNK);
        }
        assert_eq!(transfer.step(&write, limit, Instant::now()), None);
        assert_eq!(
            transfer.step(&write, limit, Instant::now()),
            Some(ClipboardOutcome::WritePosted)
        );
        drop(write);
        let mut actual = vec![];
        (&read).read_to_end(&mut actual).unwrap();
        assert_eq!(actual, vec![b'a'; CLIPBOARD_TEXT_LIMIT]);
        let (read, write) = pair();
        drop(read);
        let mut transfer = Transfer::Write {
            bytes: b"bye".as_slice().into(),
            offset: 0,
        };
        assert_eq!(
            transfer.step(&write, deadline(), Instant::now()),
            Some(ClipboardOutcome::Error(ClipboardError::Disconnected))
        );
    }
}
