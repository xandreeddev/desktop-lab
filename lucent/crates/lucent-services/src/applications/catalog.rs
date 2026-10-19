use super::*;
use std::os::unix::{ffi::OsStrExt, fs::OpenOptionsExt};
const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_CATALOG_BYTES: usize = 16 * 1024 * 1024;
const MAX_DEPENDENCIES: usize = 4096;
const MAX_DEPENDENCY_PATH_BYTES: usize = 4096;
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
/// Resolve one ID under the same deterministic masking and bounded IO policy as
/// discovery, retaining only this entry's source text for a native Open request.
pub(super) fn resolve(
    source: &XdgApplications,
    id: &AppId,
) -> Result<(Application, PathBuf, String)> {
    let unavailable = || DomainError::Invalid("Application changed; choose it again".into());
    let mut count = 0usize;
    for root in &source.data_roots {
        let root = root.join("applications");
        let mut files = vec![];
        collect(&root, 0, &mut files)?;
        count += files.len();
        if count > MAX_FILES {
            return Err(DomainError::Unavailable(
                "Application catalog file limit exceeded".into(),
            ));
        }
        files.sort();
        for path in files {
            let candidate = path
                .strip_prefix(&root)
                .map_err(|_| unavailable())?
                .to_string_lossy()
                .replace('/', "-");
            if candidate != id.as_str() {
                continue;
            }
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
                .map_err(|_| unavailable())?;
            if !file.metadata().map_err(|_| unavailable())?.is_file() {
                return Err(unavailable());
            }
            let mut text = String::new();
            file.take(MAX_FILE_BYTES + 1)
                .read_to_string(&mut text)
                .map_err(|_| unavailable())?;
            if text.len() as u64 > MAX_FILE_BYTES {
                return Err(unavailable());
            }
            let app = parse_entry_with_program_dirs(
                id.as_str(),
                &path.to_string_lossy(),
                &text,
                &source.desktop,
                &source.locale,
                &source.program_dirs,
            )?
            .ok_or_else(unavailable)?;
            return Ok((app, path, text));
        }
    }
    Err(unavailable())
}
/// Metadata dependencies are observed even while TryExec hides an entry. Keep
/// this bounded discovery on the catalog worker, never in client paint/update.
pub(super) fn dependency_files(source: &XdgApplications) -> Result<Vec<(PathBuf, bool)>> {
    let mut dependencies = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
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
            if !seen.insert(id) {
                continue;
            }
            let Some(text) = (|| {
                let file = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(path)
                    .ok()?;
                if !file.metadata().ok()?.is_file() {
                    return None;
                }
                let mut text = String::new();
                file.take(MAX_FILE_BYTES + 1)
                    .read_to_string(&mut text)
                    .ok()?;
                (text.len() as u64 <= MAX_FILE_BYTES).then_some(text)
            })() else {
                continue;
            };
            let mut main = false;
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    main = line == "[Desktop Entry]";
                    continue;
                }
                if main && let Some((key @ ("TryExec" | "Icon"), value)) = line.split_once('=') {
                    let value = unescape(value.trim());
                    let paths = if key == "TryExec" {
                        executable_candidates(&value, &source.program_dirs)
                    } else if Path::new(&value).is_absolute() {
                        vec![PathBuf::from(value)]
                    } else {
                        vec![]
                    };
                    for path in paths {
                        insert_dependency(&mut dependencies, path, key == "Icon")?;
                    }
                }
            }
        }
    }
    Ok(dependencies.into_iter().collect())
}
fn insert_dependency(
    dependencies: &mut BTreeMap<PathBuf, bool>,
    path: PathBuf,
    artwork: bool,
) -> Result<()> {
    let bytes = path.as_os_str().as_bytes();
    // Such paths cannot name a usable Linux executable/icon. Their descriptor
    // remains watched, so correcting the field still triggers discovery.
    if !path.is_absolute() || bytes.len() > MAX_DEPENDENCY_PATH_BYTES || bytes.contains(&0) {
        return Ok(());
    }
    dependencies
        .entry(path)
        .and_modify(|existing| *existing |= artwork)
        .or_insert(artwork);
    if dependencies.len() > MAX_DEPENDENCIES {
        return Err(DomainError::Unavailable(
            "Application dependency watch limit exceeded".into(),
        ));
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
                parse_entry_with_program_dirs(
                    &id,
                    &path.to_string_lossy(),
                    &text,
                    &source.desktop,
                    &source.locale,
                    &source.program_dirs,
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
