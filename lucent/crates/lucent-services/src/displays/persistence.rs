//! Keep writes only our marked tail block, preserving all preceding user bytes.
use super::*;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
};
const BEGIN: &str = "-- BEGIN LUCENT DISPLAY CONFIGURATION\n";
const END: &str = "-- END LUCENT DISPLAY CONFIGURATION\n";
const MAX_BYTES: u64 = 256 * 1024;
pub(super) struct Captured {
    path: PathBuf,
    bytes: Vec<u8>,
    identity: (u64, u64, u64, i64, i64),
    mode: u32,
}
fn open(path: &std::path::Path) -> Result<(File, std::fs::Metadata)> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| failed("User monitor configuration must be an existing regular file"))?;
    let meta = file
        .metadata()
        .map_err(|_| failed("User monitor configuration cannot be inspected"))?;
    if !meta.is_file() || meta.len() > MAX_BYTES || meta.uid() != unsafe { libc::geteuid() } {
        return Err(failed(
            "User monitor configuration is not a bounded owned regular file",
        ));
    }
    Ok((file, meta))
}
fn identity(m: &std::fs::Metadata) -> (u64, u64, u64, i64, i64) {
    (m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec())
}
impl Captured {
    pub fn capture() -> Result<Self> {
        let base = crate::config_directory();
        let path = base
            .parent()
            .ok_or_else(|| failed("Configuration directory is unavailable"))?
            .join("hypr/monitors.lua");
        Self::at(path)
    }
    fn at(path: PathBuf) -> Result<Self> {
        let this = Self::inspect(path)?;
        this.base()?;
        Ok(this)
    }
    fn inspect(path: PathBuf) -> Result<Self> {
        let (mut file, meta) = open(&path)?;
        let mut bytes = vec![];
        Read::by_ref(&mut file)
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| failed("User monitor configuration cannot be read"))?;
        let after = file
            .metadata()
            .map_err(|_| failed("User monitor configuration cannot be reinspected"))?;
        if bytes.len() as u64 > MAX_BYTES
            || bytes.len() as u64 != meta.len()
            || identity(&meta) != identity(&after)
            || meta.mode() != after.mode()
        {
            return Err(failed("User monitor configuration changed while reading"));
        }
        let this = Self {
            path,
            bytes,
            identity: identity(&meta),
            mode: meta.mode(),
        };
        Ok(this)
    }
    fn base(&self) -> Result<&[u8]> {
        let text = std::str::from_utf8(&self.bytes)
            .map_err(|_| failed("User monitor configuration is not UTF-8"))?;
        match text.find(BEGIN) {
            None => {
                if text.contains(END) {
                    Err(failed("Invalid Lucent display configuration marker"))
                } else {
                    Ok(&self.bytes)
                }
            }
            Some(start) => {
                if text[start + BEGIN.len()..].contains(BEGIN)
                    || !text.ends_with(END)
                    || text[..start].contains(END)
                {
                    return Err(failed(
                        "Lucent display configuration must be one complete tail block",
                    ));
                }
                Ok(&self.bytes[..start])
            }
        }
    }
    fn unchanged(&self) -> Result<()> {
        let current = Self::inspect(self.path.clone())?;
        if current.identity != self.identity
            || current.bytes != self.bytes
            || current.mode != self.mode
        {
            return Err(failed(
                "User monitor configuration changed; reverting instead of overwriting it",
            ));
        }
        Ok(())
    }
    pub fn content(&self, layout: &[DisplayConfiguration]) -> Result<Vec<u8>> {
        let mut content = self.base()?.to_vec();
        if !content.is_empty() && !content.ends_with(b"\n") {
            content.push(b'\n');
        }
        content.extend_from_slice(BEGIN.as_bytes());
        for c in layout {
            content.extend_from_slice(native::lua(c).as_bytes());
            content.push(b'\n');
        }
        content.extend_from_slice(END.as_bytes());
        Ok(content)
    }
    #[cfg(test)]
    pub fn keep(&self, layout: &[DisplayConfiguration]) -> Result<()> {
        self.keep_checked(layout, &|| Ok(()))
    }
    pub fn keep_checked(
        &self,
        layout: &[DisplayConfiguration],
        validate: &dyn Fn() -> Result<()>,
    ) -> Result<()> {
        validate()?;
        self.unchanged()?;
        let content = self.content(layout)?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| failed("Monitor configuration directory is unavailable"))?;
        let temp = parent.join(format!(
            ".lucent-displays-{}-{}.tmp",
            std::process::id(),
            super::boot_millis()?
        ));
        let backup = temp.with_extension("baseline");
        let recovery_required = std::cell::Cell::new(false);
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(self.mode & 0o777)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temp)
                .map_err(|_| failed("Display configuration cannot be staged"))?;
            file.write_all(&content)
                .and_then(|_| file.sync_all())
                .map_err(|_| failed("Display configuration could not be saved"))?;
            let staged = file
                .metadata()
                .map_err(|_| failed("Display configuration cannot be inspected"))?;
            // Retain the exact original inode until the replacement is durable.
            std::fs::hard_link(&self.path, &backup)
                .map_err(|_| failed("Display configuration rollback could not be established"))?;
            let captured = Self::at(backup.clone())?;
            if captured.identity != self.identity || captured.bytes != self.bytes {
                return Err(failed("Monitor configuration changed before commit"));
            }
            self.unchanged()?;
            validate()?;
            // Exchange atomically and inspect the outgoing inode. This closes
            // the final check/rename race with an editor's atomic replacement.
            exchange(&temp, &self.path)?;
            recovery_required.set(true);
            let unchanged = Self::inspect(temp.clone()).is_ok_and(|outgoing| {
                outgoing.identity == self.identity && outgoing.bytes == self.bytes
            });
            if !unchanged {
                restore_exchange(&temp, &self.path, &staged)?;
                recovery_required.set(false);
                return Err(failed(
                    "Monitor configuration changed during commit; external file restored",
                ));
            }
            if File::open(parent).and_then(|f| f.sync_all()).is_err() || validate().is_err() {
                restore_exchange(&temp, &self.path, &staged)?;
                recovery_required.set(false);
                return Err(failed(
                    "Display configuration durability or authority check failed; original inode restored",
                ));
            }
            recovery_required.set(false);
            Ok(())
        })();
        if !recovery_required.get() {
            let _ = std::fs::remove_file(&temp);
            let _ = std::fs::remove_file(&backup);
        }
        result
    }
}
fn exchange(first: &std::path::Path, second: &std::path::Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let a = std::ffi::CString::new(first.as_os_str().as_bytes())
        .map_err(|_| failed("Invalid monitor configuration path"))?;
    let b = std::ffi::CString::new(second.as_os_str().as_bytes())
        .map_err(|_| failed("Invalid monitor configuration path"))?;
    if unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    } != 0
    {
        return Err(failed(
            "Atomic display configuration exchange is unavailable",
        ));
    }
    Ok(())
}
fn restore_exchange(
    temp: &std::path::Path,
    path: &std::path::Path,
    staged: &std::fs::Metadata,
) -> Result<()> {
    let (_, current) = open(path)?;
    if current.dev() != staged.dev() || current.ino() != staged.ino() {
        return Err(failed(
            "Display configuration changed again; recovery checkpoints retained for inspection",
        ));
    }
    exchange(temp, path)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tail_block_preserves_user_bytes_and_rejects_non_tail_markers() {
        let path =
            std::env::temp_dir().join(format!("lucent-display-persistence-{}", std::process::id()));
        std::fs::write(
            &path,
            b"-- user monitor rules\nhl.monitor({ output = \"\", mode = \"preferred\" })\n",
        )
        .unwrap();
        let captured = Captured::at(path.clone()).unwrap();
        let original = captured.bytes.clone();
        captured.keep(&[]).unwrap();
        let next = Captured::at(path.clone()).unwrap();
        assert_eq!(next.base().unwrap(), original);
        let before = std::fs::metadata(&path).unwrap().ino();
        std::fs::write(&path, b"-- edited externally\n").unwrap();
        assert!(next.keep(&[]).is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), before);
        assert_eq!(std::fs::read(&path).unwrap(), b"-- edited externally\n");
        std::fs::write(&path, format!("{BEGIN}{END}-- user after marker\n")).unwrap();
        assert!(Captured::at(path.clone()).is_err());
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod authority_tests {
    use super::*;
    #[test]
    fn authority_loss_after_exchange_restores_exact_original_inode() {
        let path =
            std::env::temp_dir().join(format!("lucent-display-authority-{}", std::process::id()));
        std::fs::write(&path, b"-- original user rule\n").unwrap();
        let captured = Captured::at(path.clone()).unwrap();
        let original = std::fs::metadata(&path).unwrap().ino();
        let checks = std::cell::Cell::new(0);
        let result = captured.keep_checked(&[], &|| {
            checks.set(checks.get() + 1);
            if checks.get() == 3 {
                Err(failed("Authority retired"))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(checks.get(), 3);
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), original);
        assert_eq!(std::fs::read(&path).unwrap(), b"-- original user rule\n");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn oversized_or_symlink_configuration_is_rejected_before_staging() {
        let directory =
            std::env::temp_dir().join(format!("lucent-display-file-check-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("monitors.lua");
        std::fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        assert!(Captured::at(path.clone()).is_err());
        let link = directory.join("link.lua");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(Captured::at(link.clone()).is_err());
        std::fs::remove_file(link).unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
