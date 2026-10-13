//! Kernel backlight catalog. Transport names and filesystem identity stay private.
use super::*;
use std::{collections::BTreeMap, fs, io::Read, os::unix::fs::MetadataExt, path::Path};
#[derive(Clone)]
pub(super) struct Device {
    pub id: PowerObjectId,
    pub name: String,
    path: PathBuf,
    identity: (u64, u64),
    pub maximum: u32,
}

/// Kernel uevents wake observation after external brightness changes/hotplug.
pub(super) struct Updates(async_io::Async<std::os::fd::OwnedFd>);
impl Updates {
    pub fn new() -> std::io::Result<Self> {
        use std::os::fd::{AsRawFd, FromRawFd};
        // SAFETY: socket owns a fresh descriptor; the sockaddr is fully initialized.
        let raw = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_DGRAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                libc::NETLINK_KOBJECT_UEVENT,
            )
        };
        if raw < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) };
        let mut address: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
        address.nl_family = libc::AF_NETLINK as u16;
        address.nl_groups = 1;
        let result = unsafe {
            libc::bind(
                fd.as_raw_fd(),
                (&address as *const libc::sockaddr_nl).cast(),
                std::mem::size_of_val(&address) as libc::socklen_t,
            )
        };
        if result < 0 {
            return Err(std::io::Error::last_os_error());
        }
        async_io::Async::new_nonblocking(fd).map(Self)
    }
    pub async fn next(&mut self) -> std::io::Result<bool> {
        use std::os::fd::AsRawFd;
        self.0
            .read_with(|fd| {
                let mut buffer = [0u8; 32 * 1024];
                let mut sender: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
                let mut length = std::mem::size_of_val(&sender) as libc::socklen_t;
                // SAFETY: recvfrom receives valid writable buffers and their sizes.
                let count = unsafe {
                    libc::recvfrom(
                        fd.as_raw_fd(),
                        buffer.as_mut_ptr().cast(),
                        buffer.len(),
                        0,
                        (&mut sender as *mut libc::sockaddr_nl).cast(),
                        &mut length,
                    )
                };
                if count < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(sender.nl_pid == 0
                    && buffer[..count as usize]
                        .split(|b| *b == 0)
                        .any(|field| field == b"SUBSYSTEM=backlight"))
            })
            .await
    }
}
#[derive(Default)]
pub(super) struct Catalog {
    pub devices: BTreeMap<String, Device>,
}
fn number(path: &Path) -> Result<u32> {
    let mut text = String::new();
    fs::File::open(path)
        .and_then(|file| file.take(64).read_to_string(&mut text))
        .map_err(|_| DomainError::Unavailable("Backlight value is unavailable".into()))?;
    text.trim()
        .parse()
        .map_err(|_| DomainError::Failed("Backlight returned an invalid value".into()))
}
fn identity(path: &Path) -> Result<(u64, u64)> {
    let meta = fs::metadata(path).map_err(|_| DomainError::Invalid("Backlight retired".into()))?;
    Ok((meta.dev(), meta.ino()))
}
impl Catalog {
    pub fn read(&mut self, adapter: &PowerServices) -> Result<Vec<BrightnessDevice>> {
        let entries = match fs::read_dir(&adapter.backlight_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.devices.clear();
                return Ok(vec![]);
            }
            Err(_) => {
                return Err(DomainError::Unavailable(
                    "Backlight catalog is unavailable".into(),
                ));
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            let entry =
                entry.map_err(|_| DomainError::Unavailable("Backlight catalog changed".into()))?;
            if paths.len() >= 64 {
                return Err(DomainError::Failed("Too many backlight controls".into()));
            }
            paths.push(entry.path());
        }
        paths.sort();
        let mut next = BTreeMap::new();
        let mut result = Vec::new();
        for path in paths {
            let Some(name) = path.file_name().and_then(|s| s.to_str()).map(str::to_owned) else {
                continue;
            };
            if name.len() > 128
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
            {
                continue;
            }
            let maximum = number(&path.join("max_brightness"))?;
            if maximum == 0 {
                return Err(DomainError::Failed("Backlight maximum is zero".into()));
            }
            let brightness = number(&path.join("brightness"))?;
            if brightness > maximum {
                return Err(DomainError::Failed(
                    "Backlight value exceeds its maximum".into(),
                ));
            }
            let key = identity(&path)?;
            let id = match self
                .devices
                .get(&name)
                .filter(|d| d.identity == key && d.maximum == maximum)
            {
                Some(device) => device.id,
                None => PowerObjectId::new(adapter.identity()?)?,
            };
            next.insert(
                name.clone(),
                Device {
                    id,
                    name: name.clone(),
                    path,
                    identity: key,
                    maximum,
                },
            );
            result.push(BrightnessDevice {
                id,
                name,
                brightness: Percentage::new(f64::from(brightness) * 100. / f64::from(maximum))?,
            });
        }
        self.devices = next;
        Ok(result)
    }
    pub fn resolve(&self, id: PowerObjectId) -> Result<Device> {
        let device = self
            .devices
            .values()
            .find(|d| d.id == id)
            .ok_or_else(|| DomainError::Invalid("Choose a current backlight".into()))?;
        if identity(&device.path)? != device.identity
            || number(&device.path.join("max_brightness"))? != device.maximum
        {
            return Err(DomainError::Invalid("Backlight identity changed".into()));
        }
        Ok(device.clone())
    }
}
