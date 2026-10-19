//! Bounded desktop preferences. File operations and replacement ownership remain
//! in this adapter; the root coordinator is the only client settings writer.
use lucent_domain::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const FILE_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Clone)]
pub struct JsonSettings {
    pub path: PathBuf,
}
impl Default for JsonSettings {
    fn default() -> Self {
        Self {
            path: crate::state_directory().join("desktop.json"),
        }
    }
}

fn file_limit() -> DomainError {
    DomainError::Invalid("Desktop settings exceed the 4 MiB storage limit".into())
}

/// Bound serialized bytes as they are produced, including JSON escaping and
/// indentation. A huge aggregate must not allocate its entire encoding first.
#[derive(Default)]
struct Encoded(Vec<u8>);
impl Write for Encoded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > (FILE_LIMIT as usize).saturating_sub(self.0.len()) {
            return Err(io::Error::other("Desktop settings storage limit exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn destination(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(io::Error::other("Desktop settings must be a regular file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

struct Temporary {
    path: PathBuf,
    file: File,
    cleanup: bool,
}
impl Temporary {
    fn create(parent: &Path) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // Stale files after an interrupted process never authorize truncation,
        // following a link, or deleting an entry this attempt did not create.
        for _ in 0..64 {
            let path = parent.join(format!(
                ".lucent-settings-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file,
                        cleanup: true,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other(
            "Desktop settings temporary storage is occupied",
        ))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl SettingsPort for JsonSettings {
    fn load(&self) -> Result<DesktopSettings> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(DesktopSettings::default());
            }
            Err(error) => {
                return Err(DomainError::Failed(format!(
                    "Settings could not be read: {error}"
                )));
            }
        };
        let metadata = file
            .metadata()
            .map_err(|error| DomainError::Failed(format!("Settings could not be read: {error}")))?;
        if !metadata.is_file() {
            return Err(DomainError::Invalid(
                "Desktop settings must be a regular file".into(),
            ));
        }
        if metadata.len() > FILE_LIMIT {
            return Err(file_limit());
        }
        let mut bytes = Vec::new();
        file.take(FILE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| DomainError::Failed(format!("Settings could not be read: {error}")))?;
        if bytes.len() as u64 > FILE_LIMIT {
            return Err(file_limit());
        }
        let value = serde_json::from_slice::<DesktopSettings>(&bytes)
            .map_err(|error| DomainError::Invalid(format!("Settings: {error}")))?;
        lucent_usecases::validate_settings(&value)?;
        Ok(value)
    }

    fn save(&self, settings: &DesktopSettings) -> Result<()> {
        lucent_usecases::validate_settings(settings)?;
        let mut encoded = Encoded::default();
        if serde_json::to_writer_pretty(&mut encoded, settings).is_err() {
            // Whitespace must not prevent a compact document within the same
            // byte envelope from being saved again. Reuse the bounded buffer.
            encoded.0.clear();
            serde_json::to_writer(&mut encoded, settings).map_err(|_| file_limit())?;
        }
        let save = || -> io::Result<()> {
            destination(&self.path)?;
            let parent = self
                .path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
            let mut temporary = Temporary::create(parent)?;
            temporary.file.write_all(&encoded.0)?;
            // Recheck before replacement. Even if another same-user writer
            // changes the name afterward, rename never writes through a link.
            destination(&self.path)?;
            fs::rename(&temporary.path, &self.path)?;
            temporary.cleanup = false;
            Ok(())
        };
        save().map_err(|error| {
            DomainError::Failed(format!("Could not save desktop settings: {error}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{FileTypeExt, PermissionsExt, symlink};

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "lucent-settings-storage-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn store(&self) -> JsonSettings {
            JsonSettings {
                path: self.0.join("state/desktop.json"),
            }
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn settings_storage_is_private_and_never_follows_legacy_temporary_links() {
        let directory = Directory::new();
        let store = directory.store();
        assert_eq!(store.load().unwrap(), DesktopSettings::default());
        fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        fs::set_permissions(
            store.path.parent().unwrap(),
            fs::Permissions::from_mode(0o750),
        )
        .unwrap();
        let unrelated = directory.0.join("keep.txt");
        fs::write(&unrelated, b"retained unrelated content").unwrap();
        let legacy = store.path.with_extension("tmp");
        symlink(&unrelated, &legacy).unwrap();
        let settings = DesktopSettings {
            notes: "New private notes".into(),
            ..Default::default()
        };
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap(), settings);
        assert_eq!(fs::read(&unrelated).unwrap(), b"retained unrelated content");
        assert!(
            fs::symlink_metadata(&legacy)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::metadata(&store.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(store.path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o750
        );
        let names: Vec<_> = fs::read_dir(store.path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            names.len(),
            2,
            "only desktop.json and the pre-existing legacy link remain"
        );
    }

    #[test]
    fn settings_reads_and_saves_reject_links_and_special_files_without_touching_them() {
        let directory = Directory::new();
        let store = directory.store();
        fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        let target = directory.0.join("original.json");
        let original = br#"{"version":1,"notes":"keep original"}"#;
        fs::write(&target, original).unwrap();
        symlink(&target, &store.path).unwrap();
        assert!(store.load().is_err());
        assert!(store.save(&DesktopSettings::default()).is_err());
        assert_eq!(fs::read(&target).unwrap(), original);
        assert!(
            fs::symlink_metadata(&store.path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_file(&store.path).unwrap();
        symlink(directory.0.join("missing.json"), &store.path).unwrap();
        assert!(
            store.load().is_err(),
            "a dangling link is not missing settings"
        );
        fs::remove_file(&store.path).unwrap();
        let fifo = std::ffi::CString::new(store.path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: fifo is an owned NUL-terminated pathname; no borrowed storage escapes.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let reader = store.clone();
        let (send, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            send.send(reader.load()).unwrap();
        });
        assert!(
            receive
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("settings FIFO read blocked")
                .is_err()
        );
        worker.join().unwrap();
        assert!(store.save(&DesktopSettings::default()).is_err());
        assert!(
            fs::symlink_metadata(&store.path)
                .unwrap()
                .file_type()
                .is_fifo()
        );
        assert_eq!(
            fs::read_dir(store.path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn settings_file_and_encoded_aggregate_have_independent_byte_bounds() {
        let directory = Directory::new();
        let store = directory.store();
        fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        let mut file = File::create(&store.path).unwrap();
        file.write_all(b"{}").unwrap();
        std::io::copy(&mut std::io::repeat(b' ').take(FILE_LIMIT - 2), &mut file).unwrap();
        assert_eq!(store.load().unwrap(), DesktopSettings::default());
        file.set_len(FILE_LIMIT + 1).unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::metadata(&store.path).unwrap().len(), FILE_LIMIT + 1);

        let original = br#"{"version":1,"notes":"keep original"}"#;
        fs::write(&store.path, original).unwrap();
        let mut settings = DesktopSettings::default();
        // Escaping can exceed the disk envelope even when raw string bytes do not.
        settings.layout.visible_widgets = vec!["\0".repeat(FILE_LIMIT as usize / 2)];
        assert!(store.save(&settings).is_err());
        assert_eq!(fs::read(&store.path).unwrap(), original);
        assert_eq!(
            fs::read_dir(store.path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn compact_settings_near_the_limit_round_trip_when_pretty_whitespace_exceeds_it() {
        let directory = Directory::new();
        let store = directory.store();
        fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        let mut settings = DesktopSettings::default();
        settings.layout.visible_widgets = vec!["x".into(); FILE_LIMIT as usize / 8];
        lucent_usecases::validate_settings(&settings).unwrap();
        let mut pretty = Encoded::default();
        assert!(serde_json::to_writer_pretty(&mut pretty, &settings).is_err());
        let mut compact = Encoded::default();
        serde_json::to_writer(&mut compact, &settings).unwrap();
        assert!(compact.0.len() < FILE_LIMIT as usize);
        fs::write(&store.path, &compact.0).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded, settings);
        store.save(&loaded).unwrap();
        assert_eq!(store.load().unwrap(), loaded);
        assert_eq!(fs::read(&store.path).unwrap(), compact.0);
        assert_eq!(
            fs::read_dir(store.path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn invalid_saved_notes_are_preserved_and_temporary_ownership_cleans_failed_writes() {
        let directory = Directory::new();
        let store = directory.store();
        fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        let notes = "x".repeat(64 * 1024 + 1);
        let original = serde_json::to_vec(&serde_json::json!({"version":1,"notes":notes})).unwrap();
        fs::write(&store.path, &original).unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read(&store.path).unwrap(), original);
        let invalid = DesktopSettings {
            notes: "x".repeat(NOTES_MAX_BYTES + 1),
            ..Default::default()
        };
        assert!(store.save(&invalid).is_err());
        assert_eq!(fs::read(&store.path).unwrap(), original);

        let failed_path;
        {
            let mut pending = Temporary::create(store.path.parent().unwrap()).unwrap();
            failed_path = pending.path.clone();
            pending.file.write_all(b"interrupted candidate").unwrap();
            assert_eq!(
                pending.file.metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
            // An I/O failure or early return drops precisely this attempt's candidate.
        }
        assert!(!failed_path.exists());
        assert_eq!(fs::read(&store.path).unwrap(), original);
        assert_eq!(
            fs::read_dir(store.path.parent().unwrap()).unwrap().count(),
            1
        );
    }
    #[test]
    fn live_frame_rate_preserves_numeric_preferences_and_explicit_source_mode() {
        for fps in [1, 15, 30, 60] {
            let rate = WallpaperFrameRate::Capped(WallpaperFps::new(fps).unwrap());
            assert_eq!(serde_json::to_string(&rate).unwrap(), fps.to_string());
            assert_eq!(
                serde_json::from_str::<WallpaperFrameRate>(&fps.to_string()).unwrap(),
                rate
            );
        }
        assert_eq!(
            serde_json::to_string(&WallpaperFrameRate::Source).unwrap(),
            "\"source\""
        );
        assert_eq!(
            serde_json::from_str::<WallpaperFrameRate>("\"source\"").unwrap(),
            WallpaperFrameRate::Source
        );
        for value in [
            "0", "61", "256", "-1", "30.5", "null", "{}", "\"auto\"", "\"60\"",
        ] {
            assert!(
                serde_json::from_str::<WallpaperFrameRate>(value).is_err(),
                "accepted invalid rate {value}"
            );
        }
    }

    #[test]
    fn missing_profile_frame_rate_follows_source_without_rewriting_existing_cap() {
        let mut value = serde_json::json!({
            "asset": {
                "source": { "path": "/media/scene.mp4", "name": "Scene" },
                "path": "/owned/source.mp4",
                "poster": { "path": "/owned/poster.png", "name": "Scene" }
            }
        });
        let profile: LiveWallpaperSettings = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(profile.fps, WallpaperFrameRate::Source);
        value["fps"] = serde_json::json!(30);
        let profile: LiveWallpaperSettings = serde_json::from_value(value).unwrap();
        assert_eq!(profile.fps.cap().unwrap().value(), 30);
        assert_eq!(serde_json::to_value(profile).unwrap()["fps"], 30);
    }

    #[test]
    fn monitor_layouts_round_trip_legacy_keys_and_reject_invalid_saved_identities() {
        let legacy=br#"{"version":1,"positions":{"clock":{"x":32,"y":48}},"visible_widgets":["clock"],"notes":"Shared"}"#;
        let mut settings: DesktopSettings = serde_json::from_slice(legacy).unwrap();
        assert_eq!(settings.layout.visible_widgets, ["clock"]);
        assert!(settings.monitor_layouts.is_empty());
        let id = MonitorId::new("DP-1").unwrap();
        lucent_usecases::ensure_monitor_layout(&mut settings, &id).unwrap();
        settings
            .monitor_layouts
            .get_mut(&id)
            .unwrap()
            .positions
            .insert("clock".into(), Placement::new(64., 80.).unwrap());
        let encoded = serde_json::to_value(&settings).unwrap();
        assert!(encoded.get("layout").is_none());
        assert_eq!(encoded["positions"]["clock"]["x"], 32.);
        assert_eq!(
            encoded["monitor_layouts"]["DP-1"]["positions"]["clock"]["x"],
            64.
        );
        assert_eq!(
            serde_json::from_value::<DesktopSettings>(encoded).unwrap(),
            settings
        );
        for bad in [
            r#"{"monitor_layouts":{"":{"visible_widgets":[]}}}"#,
            r#"{"monitor_layouts":{"bad\nname":{}}}"#,
            r#"{"monitor_layouts":{"DP-1":{"positions":{"clock":{"x":1e39,"y":0}}}}}"#,
        ] {
            assert!(serde_json::from_str::<DesktopSettings>(bad).is_err());
        }
    }

    #[test]
    fn settings_adapter_validates_before_replacing_existing_preferences() {
        let directory =
            std::env::temp_dir().join(format!("lucent-settings-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let store = JsonSettings {
            path: directory.join("desktop.json"),
        };
        fs::write(&store.path, br#"{"version":1,"notes":"retained"}"#).unwrap();
        let mut settings = store.load().unwrap();
        assert_eq!(settings.launcher, LauncherSize::default());
        settings
            .layout
            .positions
            .insert("clock".into(), Placement::new(-12.5, 800.).unwrap());
        settings.pinned = vec![AppId::new("org.example.App.desktop").unwrap()];
        settings.launcher = LauncherSize {
            width: Some(800),
            max_height: Some(720),
        };
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap(), settings);
        let stored: serde_json::Value =
            serde_json::from_slice(&fs::read(&store.path).unwrap()).unwrap();
        assert_eq!(
            stored["pinned"],
            serde_json::json!(["org.example.App.desktop"])
        );
        assert_eq!(
            stored["positions"]["clock"],
            serde_json::json!({"x":-12.5,"y":800.})
        );
        let saved = fs::read(&store.path).unwrap();
        settings.launcher.width = Some(0);
        assert!(store.save(&settings).is_err());
        assert_eq!(fs::read(&store.path).unwrap(), saved);
        fs::write(&store.path, br#"{"version":1,"launcher":{"max_height":0}}"#).unwrap();
        assert!(store.load().is_err());
        fs::write(&store.path, br#"{"version":2}"#).unwrap();
        assert!(store.load().is_err());
        let invalid_id = br#"{"version":1,"pinned":["/tmp/app.desktop"],"notes":"keep me"}"#;
        fs::write(&store.path, invalid_id).unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read(&store.path).unwrap(), invalid_id);
        let invalid_position =
            br#"{"version":1,"positions":{"clock":{"x":1e39,"y":10}},"notes":"keep me"}"#;
        fs::write(&store.path, invalid_position).unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read(&store.path).unwrap(), invalid_position);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn weather_preferences_validate_roundtrip_and_preserve_invalid_files() {
        let directory = Directory::new();
        let store = JsonSettings {
            path: directory.0.join("desktop.json"),
        };
        fs::write(&store.path, br#"{"version":1}"#).unwrap();
        let mut settings = store.load().unwrap();
        assert!(
            settings.weather.is_none(),
            "only absent preferences allow legacy import"
        );
        settings.weather = Some(WeatherPreferences {
            enabled: false,
            location: Some(
                WeatherLocation::new("Fixture place", WeatherCoordinates::new(0., 0.).unwrap())
                    .unwrap(),
            ),
            unit: TemperatureUnit::Fahrenheit,
        });
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap(), settings);
        let original = fs::read(&store.path).unwrap();
        settings.weather.as_mut().unwrap().enabled = true;
        settings.weather.as_mut().unwrap().location = None;
        assert!(store.save(&settings).is_err());
        assert_eq!(fs::read(&store.path).unwrap(), original);
        for bad in [br#"{"weather":{"enabled":true,"location":{"name":"Fixture","coordinates":{"latitude":91,"longitude":0}}}}"#.as_slice(),br#"{"weather":{"unit":"kelvin"}}"#.as_slice()] {
            fs::write(&store.path,bad).unwrap();assert!(store.load().is_err());assert_eq!(fs::read(&store.path).unwrap(),bad);
        }
    }
}
