//! Linux directory notifications with parent watches for roots not yet created.
use super::*;
use std::{
    collections::BTreeMap,
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    },
    time::{Duration, Instant},
};
const MASK: u32 = libc::IN_CREATE
    | libc::IN_CLOSE_WRITE
    | libc::IN_DELETE
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_ATTRIB
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF;
#[derive(Clone)]
enum Interest {
    Desktop,
    All,
    Child(Vec<u8>),
}
struct Watcher {
    fd: OwnedFd,
    interests: BTreeMap<i32, Vec<Interest>>,
}
fn error() -> DomainError {
    DomainError::Unavailable("Application directory observation is unavailable".into())
}
impl Watcher {
    fn new() -> Result<Self> {
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if fd < 0 {
            return Err(error());
        }
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            interests: BTreeMap::new(),
        })
    }
    fn add(
        &self,
        path: &Path,
        interest: Interest,
        next: &mut BTreeMap<i32, Vec<Interest>>,
    ) -> Result<()> {
        if next.len() >= 4096 {
            return Err(DomainError::Unavailable(
                "Application directory watch limit exceeded".into(),
            ));
        }
        let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| error())?;
        let id = unsafe { libc::inotify_add_watch(self.fd.as_raw_fd(), path.as_ptr(), MASK) };
        if id < 0 {
            return Err(error());
        }
        next.entry(id).or_default().push(interest);
        Ok(())
    }
    fn parent(&self, path: &Path, next: &mut BTreeMap<i32, Vec<Interest>>) -> Result<()> {
        let mut child = path;
        while let Some(parent) = child.parent() {
            if parent.is_dir() {
                let name = child.file_name().ok_or_else(error)?.as_bytes().to_vec();
                return self.add(parent, Interest::Child(name), next);
            }
            child = parent;
        }
        Err(error())
    }
    fn tree(&self, path: &Path, depth: u8, next: &mut BTreeMap<i32, Vec<Interest>>) -> Result<()> {
        if depth > 8 || !path.is_dir() {
            return Ok(());
        }
        self.add(path, Interest::Desktop, next)?;
        for entry in fs::read_dir(path).map_err(|_| error())? {
            let entry = entry.map_err(|_| error())?;
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                self.tree(&entry.path(), depth + 1, next)?;
            }
        }
        Ok(())
    }
    fn refresh(&mut self, source: &XdgApplications) -> Result<()> {
        let mut next = BTreeMap::new();
        for root in &source.data_roots {
            let root = root.join("applications");
            self.parent(&root, &mut next)?;
            self.tree(&root, 0, &mut next)?;
        }
        for directory in &source.program_dirs {
            self.parent(directory, &mut next)?;
            if directory.is_dir() {
                self.add(directory, Interest::All, &mut next)?;
            }
        }
        for id in self.interests.keys().filter(|id| !next.contains_key(id)) {
            unsafe { libc::inotify_rm_watch(self.fd.as_raw_fd(), *id) };
        }
        self.interests = next;
        Ok(())
    }
    fn changed(&self) -> Result<bool> {
        let mut descriptor = libc::pollfd {
            fd: self.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let n = unsafe { libc::poll(&mut descriptor, 1, 100) };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(error());
        }
        if n == 0 {
            return Ok(false);
        }
        if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(error());
        }
        let mut buffer = [0u8; 65536];
        let mut dirty = false;
        for _ in 0..16 {
            let n = unsafe {
                libc::read(
                    self.fd.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if n < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
                    break;
                }
                return Err(error());
            }
            if n == 0 {
                break;
            }
            let mut offset = 0;
            let count = n as usize;
            while offset + std::mem::size_of::<libc::inotify_event>() <= count {
                let event = unsafe {
                    std::ptr::read_unaligned(
                        buffer.as_ptr().add(offset).cast::<libc::inotify_event>(),
                    )
                };
                let begin = offset + std::mem::size_of::<libc::inotify_event>();
                let end = begin + event.len as usize;
                if end > count {
                    return Err(error());
                }
                let name = &buffer[begin..end];
                let name = &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())];
                dirty |= event.mask & libc::IN_Q_OVERFLOW != 0
                    || self.interests.get(&event.wd).is_some_and(|interests| {
                        interests.iter().any(|interest| {
                            if event.mask
                                & (libc::IN_DELETE_SELF | libc::IN_MOVE_SELF | libc::IN_IGNORED)
                                != 0
                            {
                                return true;
                            }
                            match interest {
                                Interest::All => true,
                                Interest::Desktop => {
                                    event.mask & libc::IN_ISDIR != 0 || name.ends_with(b".desktop")
                                }
                                Interest::Child(child) => name == child,
                            }
                        })
                    });
                offset = end;
            }
        }
        Ok(dirty)
    }
}
fn session(
    source: &XdgApplications,
    emit: &mut dyn FnMut(Result<Vec<Application>>),
    stop: &dyn StopSignal,
) -> Result<()> {
    let mut watcher = Watcher::new()?;
    watcher.refresh(source)?;
    let mut previous = None;
    let mut recovery = true;
    let mut first = None;
    let mut last = None;
    let publish = |previous: &mut Option<Vec<Application>>,
                   recovery: &mut bool,
                   emit: &mut dyn FnMut(Result<Vec<Application>>)| {
        match source.discover() {
            Ok(apps) => {
                if *recovery || previous.as_ref() != Some(&apps) {
                    emit(Ok(apps.clone()));
                    *previous = Some(apps);
                }
                *recovery = false;
            }
            Err(error) => {
                *recovery = true;
                emit(Err(error));
            }
        }
    };
    publish(&mut previous, &mut recovery, emit);
    while !stop.cancelled() {
        if watcher.changed()? {
            let now = Instant::now();
            first.get_or_insert(now);
            last = Some(now);
        }
        let now = Instant::now();
        if first.is_some_and(|t| now.duration_since(t) >= Duration::from_millis(500))
            || last.is_some_and(|t| now.duration_since(t) >= Duration::from_millis(150))
        {
            // Add new directories before scanning; queued changes remain on the same descriptor.
            watcher.refresh(source)?;
            publish(&mut previous, &mut recovery, emit);
            first = None;
            last = None;
        }
    }
    Ok(())
}
pub(super) fn observe(
    source: &XdgApplications,
    emit: &mut dyn FnMut(Result<Vec<Application>>),
    stop: &dyn StopSignal,
) {
    while !stop.cancelled() {
        match session(source, emit, stop) {
            Ok(()) => return,
            Err(error) => {
                emit(Err(error));
                stop.wait(Duration::from_secs(2));
            }
        }
    }
}
#[cfg(test)]
mod tests;
