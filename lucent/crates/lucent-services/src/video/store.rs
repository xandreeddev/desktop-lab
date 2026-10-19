//! Read-only import lookup. Owned media is never rewritten or collected here.
use super::*;
use std::{
    io::{Read, Seek, SeekFrom},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
};

const STORE_ENTRIES: usize = 1024;
const METADATA_BYTES: u64 = 16 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    length: u64,
    mode: u32,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl Identity {
    fn of(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            mode: metadata.mode(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }
}

fn open_regular(path: &Path, limit: u64) -> Result<(fs::File, Identity)> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| storage())?;
    let metadata = file.metadata().map_err(|_| storage())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(DomainError::Invalid(
            "Local media storage contains an invalid regular file".into(),
        ));
    }
    Ok((file, Identity::of(&metadata)))
}

fn unchanged(path: &Path, file: &fs::File, identity: Identity) -> Result<()> {
    let current = file.metadata().map_err(|_| storage())?;
    let named = fs::symlink_metadata(path).map_err(|_| storage())?;
    if !current.is_file()
        || !named.is_file()
        || Identity::of(&current) != identity
        || Identity::of(&named) != identity
    {
        return Err(DomainError::Invalid(
            "Local media changed while it was being read".into(),
        ));
    }
    Ok(())
}

pub(super) fn metadata(path: &Path) -> Result<LiveWallpaperAsset> {
    let (mut file, identity) = open_regular(path, METADATA_BYTES)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| storage())?;
    unchanged(path, &file, identity)?;
    if bytes.len() as u64 != identity.length {
        return Err(storage());
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| DomainError::Invalid("Live wallpaper asset metadata is invalid".into()))
}

/// Keep one validated descriptor through lookup and copying. Reopening the
/// original pathname after comparison could import a replacement source.
pub(super) struct Source {
    path: PathBuf,
    file: fs::File,
    identity: Identity,
}
impl Source {
    pub(super) fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(DomainError::Invalid(
                "Use an absolute local media path".into(),
            ));
        }
        let (file, identity) = open_regular(path, SOURCE_BYTES)?;
        Ok(Self {
            path: path.to_owned(),
            file,
            identity,
        })
    }

    fn unchanged(&self) -> Result<()> {
        unchanged(&self.path, &self.file, self.identity)
    }

    fn matches(&mut self, path: &Path) -> Result<bool> {
        let Ok((mut candidate, identity)) = open_regular(path, SOURCE_BYTES) else {
            return Ok(false);
        };
        // Generated source/poster files are immutable. A user-modified or
        // foreign writable file cannot become the reusable owned copy.
        if identity.mode & 0o222 != 0 || identity.length != self.identity.length {
            return Ok(false);
        }
        self.unchanged()?;
        self.file.seek(SeekFrom::Start(0)).map_err(|_| storage())?;
        let mut source_bytes = [0; 64 * 1024];
        let mut owned_bytes = [0; 64 * 1024];
        let mut remaining = self.identity.length;
        let mut equal = true;
        while remaining > 0 {
            let count = remaining.min(source_bytes.len() as u64) as usize;
            self.file
                .read_exact(&mut source_bytes[..count])
                .map_err(|_| storage())?;
            if candidate.read_exact(&mut owned_bytes[..count]).is_err()
                || source_bytes[..count] != owned_bytes[..count]
            {
                equal = false;
                break;
            }
            remaining -= count as u64;
        }
        self.unchanged()?;
        Ok(equal && unchanged(path, &candidate, identity).is_ok())
    }

    pub(super) fn copy_to(&mut self, out: &mut fs::File) -> Result<()> {
        self.unchanged()?;
        self.file.seek(SeekFrom::Start(0)).map_err(|_| storage())?;
        let count = std::io::copy(&mut (&mut self.file).take(SOURCE_BYTES + 1), out)
            .map_err(|_| storage())?;
        self.unchanged()?;
        if count != self.identity.length {
            return Err(storage());
        }
        Ok(())
    }
}

impl NativeVideoWallpapers {
    pub(super) fn reusable(
        &self,
        source: &Wallpaper,
        input: &mut Source,
    ) -> Result<Option<LiveWallpaperAsset>> {
        let root = match fs::canonicalize(&self.state) {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(storage()),
        };
        let entries = fs::read_dir(root).map_err(|_| storage())?;
        // Lookup is opportunistic and bounded, not a durable import quota. A
        // matching asset outside this window may not be reused; new imports
        // remain possible without deleting any historical media.
        for entry in entries.take(STORE_ENTRIES) {
            let Ok(entry) = entry else { continue };
            if !entry.file_name().to_string_lossy().starts_with("media-")
                || !entry.file_type().is_ok_and(|kind| kind.is_dir())
            {
                continue;
            }
            let Ok(asset) = metadata(&entry.path().join("asset.json")) else {
                continue;
            };
            if asset.source != *source
                || Path::new(&asset.path).parent() != Some(entry.path().as_path())
                || self.owned(&asset).is_err()
            {
                continue;
            }
            let Ok((poster, identity)) = open_regular(Path::new(&asset.poster.path), SOURCE_BYTES)
            else {
                continue;
            };
            if identity.mode & 0o222 != 0
                || unchanged(Path::new(&asset.poster.path), &poster, identity).is_err()
            {
                continue;
            }
            if input.matches(Path::new(&asset.path))?
                && unchanged(Path::new(&asset.poster.path), &poster, identity).is_ok()
                && self.owned(&asset).is_ok()
            {
                input.unchanged()?;
                return Ok(Some(asset));
            }
        }
        Ok(None)
    }
}
