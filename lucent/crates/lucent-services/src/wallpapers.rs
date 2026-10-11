//! Single-writer wallpaper storage. The desktop owns selection ordering;
//! this adapter owns immutable source snapshots and an atomic selected link.
use lucent_domain::*;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const SOURCE_LIMIT: u64 = 32 * 1024 * 1024;
const RETAINED_ASSETS: usize = 8;
static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct WallpaperStore {
    pub state: PathBuf,
    pub legacy: Option<PathBuf>,
}
impl Default for WallpaperStore {
    fn default() -> Self {
        Self {
            state: crate::state_directory(),
            legacy: std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".local/state/omarchy/current/background")),
        }
    }
}
fn failure(error: impl std::fmt::Display) -> DomainError {
    DomainError::Failed(format!("Wallpaper storage: {error}"))
}
impl WallpaperStore {
    fn assets(&self) -> PathBuf {
        self.state.join("wallpaper-assets")
    }

    fn metadata(&self, path: &Path) -> Result<WallpaperAsset> {
        let mut bytes = Vec::new();
        fs::File::open(
            path.parent()
                .ok_or_else(|| failure("Missing asset directory"))?
                .join("source.json"),
        )
        .and_then(|file| file.take(16 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(failure)?;
        if bytes.len() > 16 * 1024 {
            return Err(failure("Invalid asset metadata"));
        }
        let asset: WallpaperAsset = serde_json::from_slice(&bytes).map_err(failure)?;
        if Path::new(&asset.path) != path {
            return Err(failure("Asset metadata does not match its file"));
        }
        Ok(asset)
    }
    fn owned(&self, asset: &WallpaperAsset) -> Result<PathBuf> {
        let path = Path::new(&asset.path);
        let directory = path
            .parent()
            .ok_or_else(|| failure("Missing asset directory"))?;
        if directory.parent() != Some(fs::canonicalize(self.assets()).map_err(failure)?.as_path())
            || !directory
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("asset-"))
            || fs::canonicalize(path).map_err(failure)? != path
            || self.metadata(path)? != *asset
        {
            return Err(failure("Asset is outside this wallpaper store"));
        }
        Ok(directory.to_path_buf())
    }
    pub fn current(&self) -> Result<Option<WallpaperAsset>> {
        let path = match fs::canonicalize(self.state.join("wallpaper")) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(legacy) = &self.legacy else {
                    return Ok(None);
                };
                match fs::canonicalize(legacy) {
                    Ok(path) => path,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(error) => return Err(failure(error)),
                }
            }
            Err(error) => return Err(failure(error)),
        };
        if path.starts_with(fs::canonicalize(self.assets()).unwrap_or_else(|_| self.assets())) {
            return self.metadata(&path).map(Some);
        }
        // Existing selections migrate on the next explicit selection; never
        // rewrite or delete the original source while merely reading it.
        Ok(Some(WallpaperAsset {
            source: Wallpaper {
                name: path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                path: path.to_string_lossy().into(),
            },
            path: path.to_string_lossy().into(),
        }))
    }
    pub fn stage(&self, source: &Wallpaper) -> Result<WallpaperAsset> {
        let original = Path::new(&source.path);
        if !original.is_absolute() || !original.is_file() {
            return Err(DomainError::Invalid(
                "Wallpaper must be an existing absolute file path".into(),
            ));
        }
        // A retained immutable snapshot can be selected again during rollback.
        if original.starts_with(fs::canonicalize(self.assets()).unwrap_or_else(|_| self.assets())) {
            let asset = self.metadata(original)?;
            self.owned(&asset)?;
            return Ok(asset);
        }
        fs::create_dir_all(self.assets()).map_err(failure)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(failure)?
            .as_nanos();
        let directory = fs::canonicalize(self.assets())
            .map_err(failure)?
            .join(format!(
                "asset-{stamp}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(failure)?;
        let result = (|| {
            let extension = original.extension().unwrap_or_default().to_string_lossy();
            let path = directory.join(format!("image.{extension}"));
            let mut input = fs::File::open(original)
                .map_err(failure)?
                .take(SOURCE_LIMIT + 1);
            let mut output = fs::File::create(&path).map_err(failure)?;
            if std::io::copy(&mut input, &mut output).map_err(failure)? > SOURCE_LIMIT {
                return Err(DomainError::Invalid(
                    "Wallpaper exceeds the 32 MiB source limit".into(),
                ));
            }
            output.flush().map_err(failure)?;
            output.sync_all().map_err(failure)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).map_err(failure)?;
            let asset = WallpaperAsset {
                source: source.clone(),
                path: path.to_string_lossy().into(),
            };
            fs::write(
                directory.join("source.json"),
                serde_json::to_vec(&asset).map_err(failure)?,
            )
            .map_err(failure)?;
            Ok(asset)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(directory);
        }
        result
    }
    pub fn commit(&self, asset: &WallpaperAsset) -> Result<()> {
        self.owned(asset)?;
        let temporary = self.state.join(format!(
            "wallpaper-{}.new",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        symlink(&asset.path, &temporary).map_err(failure)?;
        if let Err(error) = fs::rename(&temporary, self.state.join("wallpaper")) {
            let _ = fs::remove_file(temporary);
            return Err(failure(error));
        }
        // Selection succeeded even if best-effort cleanup cannot remove an old
        // cache entry. Cleanup must never turn a committed selection into failure.
        let _ = self.prune();
        Ok(())
    }
    pub fn discard(&self, asset: &WallpaperAsset) -> Result<()> {
        if !Path::new(&asset.path).exists() {
            return Ok(());
        }
        let directory = self.owned(asset)?;
        if fs::canonicalize(self.state.join("wallpaper"))
            .ok()
            .as_deref()
            != Some(Path::new(&asset.path))
        {
            fs::remove_dir_all(directory).map_err(failure)?;
        }
        Ok(())
    }
    fn prune(&self) -> Result<()> {
        let selected = fs::canonicalize(self.state.join("wallpaper")).map_err(failure)?;
        let mut entries = fs::read_dir(fs::canonicalize(self.assets()).map_err(failure)?)
            .map_err(failure)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_ok_and(|kind| kind.is_dir())
                    && e.file_name().to_string_lossy().starts_with("asset-")
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        let remove = entries.len().saturating_sub(RETAINED_ASSETS);
        for entry in entries
            .into_iter()
            .filter(|entry| !selected.starts_with(entry.path()))
            .take(remove)
        {
            fs::remove_dir_all(entry.path()).map_err(failure)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        store: WallpaperStore,
        source: Wallpaper,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "lucent-wallpaper-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let path = root.join("source.png");
            image::RgbaImage::from_pixel(2, 2, image::Rgba([80, 100, 120, 255]))
                .save(&path)
                .unwrap();
            Self {
                store: WallpaperStore {
                    state: root.join("state"),
                    legacy: None,
                },
                source: Wallpaper {
                    name: "Original".into(),
                    path: path.to_string_lossy().into(),
                },
                root,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn immutable_snapshot_survives_source_edit_and_rollback_reuses_it() {
        let f = Fixture::new();
        let asset = f.store.stage(&f.source).unwrap();
        assert_eq!(f.store.current().unwrap(), None);
        let image = crate::images::try_load(Path::new(&asset.path), 32).unwrap();
        f.store.commit(&asset).unwrap();
        fs::write(&f.source.path, b"changed original").unwrap();
        let selected = f.store.current().unwrap().unwrap();
        assert_eq!(selected, asset);
        assert_eq!(
            crate::images::try_load(Path::new(&selected.path), 32)
                .unwrap()
                .pixels(),
            image.pixels()
        );
        assert_eq!(
            fs::metadata(&selected.path).unwrap().permissions().mode() & 0o777,
            0o400
        );
        let retained = f
            .store
            .stage(&Wallpaper {
                name: "Rollback".into(),
                path: selected.path.clone(),
            })
            .unwrap();
        assert_eq!(retained, selected);
        f.store.discard(&selected).unwrap();
        assert!(Path::new(&selected.path).exists());
        assert_eq!(fs::read(&f.source.path).unwrap(), b"changed original");
    }
    #[test]
    fn invalid_decode_and_foreign_commit_preserve_selected_asset() {
        let f = Fixture::new();
        let selected = f.store.stage(&f.source).unwrap();
        f.store.commit(&selected).unwrap();
        fs::write(&f.source.path, b"not an image").unwrap();
        let invalid = f.store.stage(&f.source).unwrap();
        assert!(crate::images::try_load(Path::new(&invalid.path), 32).is_err());
        f.store.discard(&invalid).unwrap();
        assert!(!Path::new(&invalid.path).exists());
        let forged = WallpaperAsset {
            source: f.source.clone(),
            path: f.source.path.clone(),
        };
        assert!(f.store.commit(&forged).is_err());
        assert!(f.store.discard(&forged).is_err());
        assert_eq!(f.store.current().unwrap(), Some(selected));
        assert!(Path::new(&f.source.path).exists());
    }
    #[test]
    fn failed_rename_keeps_destination_and_cleans_temporary_link() {
        let f = Fixture::new();
        let asset = f.store.stage(&f.source).unwrap();
        fs::create_dir(f.store.state.join("wallpaper")).unwrap();
        assert!(f.store.commit(&asset).is_err());
        assert!(f.store.state.join("wallpaper").is_dir());
        assert!(
            !fs::read_dir(&f.store.state).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".new"))
        );
        f.store.discard(&asset).unwrap();
        assert!(Path::new(&f.source.path).exists());
    }
    #[test]
    fn oversized_source_is_bounded_and_cleaned_without_selection_change() {
        let f = Fixture::new();
        fs::File::create(&f.source.path)
            .unwrap()
            .set_len(SOURCE_LIMIT + 1)
            .unwrap();
        assert!(f.store.stage(&f.source).is_err());
        assert_eq!(fs::read_dir(f.store.assets()).unwrap().count(), 0);
        assert!(f.store.current().unwrap().is_none());
        assert!(
            f.store
                .stage(&Wallpaper {
                    name: "relative".into(),
                    path: "relative.png".into()
                })
                .is_err()
        );
    }
    #[test]
    fn retention_is_bounded_and_preserves_old_selected_snapshot() {
        let f = Fixture::new();
        let first = f.store.stage(&f.source).unwrap();
        f.store.commit(&first).unwrap();
        for _ in 0..RETAINED_ASSETS + 3 {
            f.store.stage(&f.source).unwrap();
        }
        f.store.commit(&first).unwrap();
        assert_eq!(
            fs::read_dir(f.store.assets()).unwrap().count(),
            RETAINED_ASSETS
        );
        assert_eq!(f.store.current().unwrap(), Some(first));
        assert!(Path::new(&f.source.path).exists());
    }
    #[test]
    fn legacy_read_is_non_mutating_and_symlinked_state_is_supported() {
        let mut f = Fixture::new();
        f.store.legacy = Some(PathBuf::from(&f.source.path));
        assert_eq!(f.store.current().unwrap().unwrap().path, f.source.path);
        assert!(!f.store.state.exists());
        let target = f.root.join("real-state");
        fs::create_dir(&target).unwrap();
        symlink(&target, &f.store.state).unwrap();
        let asset = f.store.stage(&f.source).unwrap();
        f.store.commit(&asset).unwrap();
        assert_eq!(f.store.current().unwrap(), Some(asset));
        assert!(Path::new(&f.source.path).exists());
    }
    #[test]
    fn svg_snapshot_and_extensionless_login_copy_decode_identically() {
        let f = Fixture::new();
        fs::write(&f.source.path, br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4"><rect width="8" height="4" fill="#2060a0"/></svg>"##).unwrap();
        let asset = f.store.stage(&f.source).unwrap();
        let selected = crate::images::try_load(Path::new(&asset.path), 32).unwrap();
        f.store.commit(&asset).unwrap();
        let login = f.root.join("login-wallpaper");
        fs::copy(&asset.path, &login).unwrap();
        assert_eq!(
            crate::images::try_load(&login, 32).unwrap().pixels(),
            selected.pixels()
        );
    }
}
