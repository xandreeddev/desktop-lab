use super::*;
use std::os::unix::fs::OpenOptionsExt;
const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_CATALOG_BYTES: usize = 16 * 1024 * 1024;
fn collect(path: &Path, depth: u8, files: &mut Vec<PathBuf>) -> Result<()> {
    if depth > 8 {
        return Ok(());
    }
    let dir = match fs::read_dir(path) {
        Ok(dir) => dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => {
            return Err(DomainError::Unavailable(
                "Application catalog directory is unreadable".into(),
            ));
        }
    };
    for entry in dir {
        let entry = entry.map_err(|_| {
            DomainError::Unavailable("Application catalog changed during discovery".into())
        })?;
        let p = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            collect(&p, depth + 1, files)?;
        } else if p.extension().is_some_and(|s| s == "desktop") {
            if files.len() >= MAX_FILES {
                return Err(DomainError::Unavailable(
                    "Application catalog file limit exceeded".into(),
                ));
            }
            files.push(p);
        }
    }
    Ok(())
}
pub(super) fn discover(source: &XdgApplications) -> Result<Vec<Application>> {
    let mut entries = BTreeMap::new();
    let mut bytes = 0usize;
    let mut count = 0usize;
    for root in &source.data_roots {
        let root = root.join("applications");
        let mut files = vec![];
        collect(&root, 0, &mut files)?;
        files.sort();
        count += files.len();
        if count > MAX_FILES {
            return Err(DomainError::Unavailable(
                "Application catalog file limit exceeded".into(),
            ));
        }
        for path in files {
            let id = path
                .strip_prefix(&root)
                .map_err(|_| DomainError::Invalid("Invalid application source".into()))?
                .to_string_lossy()
                .replace('/', "-");
            if entries.contains_key(&id) {
                continue;
            }
            let entry = (|| {
                let mut text = String::new();
                let file = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(&path)
                    .ok()?;
                if !file.metadata().ok()?.is_file() {
                    return None;
                }
                file.take(MAX_FILE_BYTES + 1)
                    .read_to_string(&mut text)
                    .ok()?;
                if text.len() as u64 > MAX_FILE_BYTES {
                    return None;
                }
                parse_entry(
                    &id,
                    &path.to_string_lossy(),
                    &text,
                    &source.desktop,
                    &source.locale,
                )
                .ok()
                .flatten()
            })();
            if let Some(app) = &entry {
                bytes += app.id.as_str().len()
                    + app.name.len()
                    + app.description.len()
                    + app.icon.len()
                    + app.startup_class.len()
                    + app.command.program.len()
                    + app.command.directory.as_ref().map_or(0, String::len)
                    + app.command.args.iter().map(String::len).sum::<usize>()
                    + app.keywords.iter().map(String::len).sum::<usize>();
                if bytes > MAX_CATALOG_BYTES {
                    return Err(DomainError::Unavailable(
                        "Application catalog content limit exceeded".into(),
                    ));
                }
            }
            // Presence masks lower-priority IDs, even when hidden, invalid or unreadable.
            entries.insert(id, entry);
        }
    }
    Ok(entries.into_values().flatten().collect())
}
