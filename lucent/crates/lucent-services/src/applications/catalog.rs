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
                bytes += retained_bytes(app);
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
fn launch_bytes(launch: &ApplicationLaunch) -> usize {
    launch.command().map_or(0, |command| {
        command.program.capacity()
            + command.directory.as_ref().map_or(0, String::capacity)
            + command.args.capacity() * std::mem::size_of::<String>()
            + command.args.iter().map(String::capacity).sum::<usize>()
    })
}
fn retained_bytes(app: &Application) -> usize {
    // Include empty argument/keyword slots and action storage, not just nonempty
    // text. Enum/Vec/String headers inside Application/Action are counted once.
    std::mem::size_of::<Application>()
        + app.id.as_str().len()
        + app.name.capacity()
        + app.description.capacity()
        + app.icon.capacity()
        + app.startup_class.capacity()
        + launch_bytes(&app.launch)
        + app.keywords.capacity() * std::mem::size_of::<String>()
        + app.keywords.iter().map(String::capacity).sum::<usize>()
        + app.actions.capacity() * std::mem::size_of::<ApplicationAction>()
        + app
            .actions
            .iter()
            .map(|action| {
                action.id.as_str().len()
                    + action.name.capacity()
                    + action.icon.capacity()
                    + launch_bytes(&action.launch)
            })
            .sum::<usize>()
}
