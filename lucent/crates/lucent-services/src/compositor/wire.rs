//! Bounded command connections and incremental event framing. No desktop policy.
use async_io::{Async, Timer};
use futures_lite::{
    future,
    io::{AsyncReadExt, AsyncWriteExt},
};
use std::{io, os::unix::net::UnixStream, path::Path, time::Duration};
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
const EVENT_LIMIT: usize = 64 * 1024;
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn connect(path: &Path) -> io::Result<UnixStream> {
    let stream = async_io::block_on(future::or(Async::<UnixStream>::connect(path), async {
        Timer::after(REQUEST_TIMEOUT).await;
        Err(io::Error::from(io::ErrorKind::TimedOut))
    }))?;
    let stream = stream.into_inner()?;
    stream.set_nonblocking(false)?;
    Ok(stream)
}
pub(super) fn request(path: &Path, text: &str) -> io::Result<String> {
    exchange(path, text, REQUEST_TIMEOUT, RESPONSE_LIMIT)
}
fn exchange(path: &Path, text: &str, timeout: Duration, limit: usize) -> io::Result<String> {
    async_io::block_on(future::or(
        async {
            let mut socket = Async::<UnixStream>::connect(path).await?;
            socket.write_all(text.as_bytes()).await?;
            let mut content = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let read = socket.read(&mut chunk).await?;
                if read == 0 {
                    break;
                }
                if content
                    .len()
                    .checked_add(read)
                    .is_none_or(|len| len > limit)
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Compositor response limit exceeded",
                    ));
                }
                content.extend_from_slice(&chunk[..read]);
            }
            String::from_utf8(content).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Compositor response is not UTF-8",
                )
            })
        },
        async {
            Timer::after(timeout).await;
            Err(io::Error::from(io::ErrorKind::TimedOut))
        },
    ))
}
#[derive(Default)]
pub(super) struct Events {
    partial: Vec<u8>,
}
impl Events {
    /// Preserve partial lines across socket timeouts; emit one refresh demand
    /// for a read containing any relevant complete event, regardless of bursts.
    pub fn push(&mut self, bytes: &[u8]) -> io::Result<bool> {
        let mut refresh = false;
        for &byte in bytes {
            if byte == b'\n' {
                refresh |= relevant(&self.partial);
                self.partial.clear();
            } else {
                if self.partial.len() >= EVENT_LIMIT {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Compositor event limit exceeded",
                    ));
                }
                self.partial.push(byte);
            }
        }
        Ok(refresh)
    }
}
fn relevant(line: &[u8]) -> bool {
    let Some(at) = line.windows(2).position(|bytes| bytes == b">>") else {
        return false;
    };
    matches!(
        &line[..at],
        b"workspace"
            | b"workspacev2"
            | b"focusedmon"
            | b"focusedmonv2"
            | b"activewindow"
            | b"activewindowv2"
            | b"openwindow"
            | b"closewindow"
            | b"movewindow"
            | b"movewindowv2"
            | b"createworkspace"
            | b"createworkspacev2"
            | b"destroyworkspace"
            | b"destroyworkspacev2"
            | b"moveworkspace"
            | b"moveworkspacev2"
            | b"renameworkspace"
            | b"activespecial"
            | b"activespecialv2"
            | b"monitoradded"
            | b"monitoraddedv2"
            | b"monitorremoved"
            | b"monitorremovedv2"
            | b"configreloaded"
    )
}

#[cfg(test)]
mod tests;

pub(super) fn request_before(
    path: &Path,
    text: &str,
    deadline: std::time::Instant,
) -> io::Result<String> {
    let remaining = deadline
        .checked_duration_since(std::time::Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))?;
    exchange(path, text, remaining, RESPONSE_LIMIT)
}
