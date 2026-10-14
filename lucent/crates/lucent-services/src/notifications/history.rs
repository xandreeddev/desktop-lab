//! Versioned, bounded private history file. No rendering or native bus ownership.
use lucent_domain::*;
use lucent_usecases::notifications::validate_archive;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
const FILE_LIMIT: u64 = 4 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    archive: NotificationArchive,
}
pub struct JsonNotificationHistory {
    pub path: PathBuf,
}
impl Default for JsonNotificationHistory {
    fn default() -> Self {
        Self {
            path: crate::state_directory().join("notifications.json"),
        }
    }
}
impl NotificationHistoryPort for JsonNotificationHistory {
    fn load(&self) -> Result<NotificationArchive> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(NotificationArchive::default());
            }
            Err(_) => {
                return Err(DomainError::Unavailable(
                    "Notification history could not be read".into(),
                ));
            }
        };
        let meta = file.metadata().map_err(|_| {
            DomainError::Unavailable("Notification history could not be read".into())
        })?;
        if !meta.is_file() || meta.len() > FILE_LIMIT {
            return Err(DomainError::Invalid(
                "Notification history file exceeds supported limits".into(),
            ));
        }
        let mut bytes = vec![];
        file.take(FILE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                DomainError::Unavailable("Notification history could not be read".into())
            })?;
        if bytes.len() as u64 > FILE_LIMIT {
            return Err(DomainError::Invalid(
                "Notification history file exceeds supported limits".into(),
            ));
        }
        let document: Document = serde_json::from_slice(&bytes)
            .map_err(|_| DomainError::Invalid("Notification history file is invalid".into()))?;
        if document.version != 1 {
            return Err(DomainError::Invalid(
                "Notification history version is unsupported".into(),
            ));
        }
        validate_archive(&document.archive)?;
        Ok(document.archive)
    }
    fn save(&self, archive: &NotificationArchive) -> Result<()> {
        validate_archive(archive)?;
        let bytes = serde_json::to_vec(&Document {
            version: 1,
            archive: archive.clone(),
        })
        .map_err(|_| DomainError::Failed("Notification history could not be encoded".into()))?;
        if bytes.len() as u64 > FILE_LIMIT {
            return Err(DomainError::Invalid(
                "Notification history file exceeds supported limits".into(),
            ));
        }
        let save = || -> std::io::Result<()> {
            let parent = self
                .path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .ok_or_else(|| std::io::Error::other("History directory required"))?;
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true).mode(0o700).create(parent)?;
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let temporary = parent.join(format!(
                ".notifications-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut created = false;
            let result = || -> std::io::Result<()> {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&temporary)?;
                created = true;
                file.write_all(&bytes)?;
                file.sync_all()?;
                fs::rename(&temporary, &self.path)?;
                File::open(parent)?.sync_all()
            }();
            if result.is_err() && created {
                let _ = fs::remove_file(&temporary);
            }
            result
        };
        save().map_err(|_| DomainError::Failed("Notification history could not be saved".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn path() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir()
            .join(format!(
                "lucent-history-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
            .join("state/notifications.json")
    }
    #[test]
    fn history_file_is_private_atomic_and_validated_before_replacement() {
        let store = JsonNotificationHistory { path: path() };
        assert_eq!(store.load().unwrap(), NotificationArchive::default());
        let mut archive = NotificationArchive {
            do_not_disturb: true,
            ..Default::default()
        };
        store.save(&archive).unwrap();
        assert_eq!(store.load().unwrap(), archive);
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
            0o700
        );
        let original = fs::read(&store.path).unwrap();
        archive.entries.push(Notification {
            id: 0,
            revision: 1,
            received_unix_ms: 0,
            desktop_entry: None,
            app: "Fixture".into(),
            summary: String::new(),
            body: String::new(),
            actions: vec![],
            critical: false,
            resident: false,
            transient: false,
            timeout_ms: 0,
        });
        assert!(store.save(&archive).is_err());
        assert_eq!(fs::read(&store.path).unwrap(), original);
        fs::remove_dir_all(store.path.parent().unwrap().parent().unwrap()).unwrap();
    }
    #[test]
    fn malformed_future_or_oversized_files_are_preserved_and_never_echo_content() {
        let store = JsonNotificationHistory { path: path() };
        fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        for bad in [
            br#"{"private-note":"not for error messages"}"#.as_slice(),
            br#"{"version":2,"archive":{"entries":[],"do_not_disturb":false,"last_revision":0}}"#
                .as_slice(),
        ] {
            fs::write(&store.path, bad).unwrap();
            let error = store.load().unwrap_err().to_string();
            assert!(!error.contains("private-note"));
            assert!(!error.contains("not for error"));
            assert_eq!(fs::read(&store.path).unwrap(), bad);
        }
        let file = File::create(&store.path).unwrap();
        file.set_len(FILE_LIMIT + 1).unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::metadata(&store.path).unwrap().len(), FILE_LIMIT + 1);
        fs::remove_dir_all(store.path.parent().unwrap().parent().unwrap()).unwrap();
    }
    #[test]
    fn final_symlinks_are_not_read_as_history() {
        let store = JsonNotificationHistory { path: path() };
        let directory = store.path.parent().unwrap();
        fs::create_dir_all(directory).unwrap();
        let other = directory.join("other.json");
        fs::write(&other, b"private data").unwrap();
        std::os::unix::fs::symlink(&other, &store.path).unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read(&other).unwrap(), b"private data");
        fs::remove_dir_all(directory.parent().unwrap()).unwrap();
    }
}
