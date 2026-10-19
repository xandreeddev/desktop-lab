//! OS adapters. All blocking operations are called from effects/subscription workers.
pub mod applications;
pub mod audio;
pub mod bluetooth;
pub mod compositor;
pub mod desktop_adapters;
pub mod images;
pub mod media;
pub mod network;
pub mod power;
pub mod session;
mod settings;
pub use settings::JsonSettings;
pub mod tray;
pub mod video;
pub mod wallpapers;
pub mod weather;
use chrono::{Datelike, Timelike};
use lucent_domain::*;
use std::{
    fs,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn state_directory() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        })
        .join("lucent")
}
pub fn config_directory() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("lucent")
}
/// Bounded command adapter. stdout is drained concurrently; timeouts kill and reap the child.
pub fn command(program: &str, args: &[&str]) -> Result<String> {
    command_with_timeout(program, args, Duration::from_secs(5))
}
pub fn command_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Result<String> {
    command_with_environment(program, args, timeout, &[])
}
/// Per-child values never mutate global process state or appear in argv.
pub(crate) fn command_with_environment(
    program: &str,
    args: &[&str],
    timeout: Duration,
    environment: &[(&str, &str)],
) -> Result<String> {
    use std::os::unix::process::CommandExt;
    let mut child = Command::new(program)
        .process_group(0)
        .args(args)
        .envs(environment.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| DomainError::Unavailable(format!("{program}: {e}")))?;
    let mut stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout).take(8 * 1024 * 1024).read_to_end(&mut bytes);
        let _ = send.send(bytes);
    });
    let start = Instant::now();
    let mut status = None;
    let mut output = None;
    loop {
        if status.is_none() {
            status = child
                .try_wait()
                .map_err(|error| DomainError::Failed(error.to_string()))?;
        }
        if output.is_none() {
            output = receive.try_recv().ok();
        }
        if let (Some(status), Some(bytes)) = (status, output.as_ref()) {
            if status.success() {
                return Ok(String::from_utf8_lossy(bytes).trim().into());
            }
            return Err(DomainError::Unavailable(format!(
                "{program} reported {}",
                status.code().unwrap_or(-1)
            )));
        }
        if start.elapsed() > timeout {
            // This child created its own group. A descendant can retain stdout
            // even after the direct child exits: bound that wait as well.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(DomainError::Unavailable(format!("{program} timed out")));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[cfg(test)]
mod command_tests {
    use super::*;
    #[test]
    fn descendants_cannot_hold_the_effect_worker_after_the_parent_exits() {
        let start = Instant::now();
        let result = command_with_timeout(
            "sh",
            &["-c", "sleep 10 & exit 0"],
            Duration::from_millis(100),
        );
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(command("printf", &["ok"]).unwrap(), "ok");
    }
}
pub fn clock() -> ClockSnapshot {
    let now = chrono::Local::now();
    ClockSnapshot {
        date: Date::new(now.year(), now.month(), now.day()).expect("valid calendar date"),
        hour: now.hour(),
        minute: now.minute(),
        unix_seconds: now.timestamp().max(0) as u64,
    }
}
#[cfg(test)]
mod calendar_adapter_tests {
    use super::*;
    #[test]
    fn domain_calendar_agrees_with_chrono_across_positive_and_negative_leap_cycles() {
        for year in [-400, 1600] {
            let mut observed = chrono::NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
            let end = chrono::NaiveDate::from_ymd_opt(year + 400, 1, 1).unwrap();
            while observed < end {
                let domain = Date::new(observed.year(), observed.month(), observed.day()).unwrap();
                assert_eq!(
                    domain.weekday(),
                    observed.weekday().num_days_from_monday(),
                    "{observed}"
                );
                let first = observed.with_day(1).unwrap();
                assert_eq!(
                    domain.first_weekday(),
                    first.weekday().num_days_from_monday()
                );
                observed = observed.succ_opt().unwrap();
            }
        }
        for year in [i32::MIN, i32::MAX] {
            let reference =
                chrono::NaiveDate::from_ymd_opt(2000 + year.rem_euclid(400), 12, 31).unwrap();
            assert_eq!(
                Date::new(year, 12, 31).unwrap().weekday(),
                reference.weekday().num_days_from_monday()
            );
        }
    }
}
#[derive(Default)]
pub struct SystemProbe {
    previous: Option<(u64, u64)>,
}
impl SystemProbe {
    pub fn sample(&mut self) -> Result<SystemSnapshot> {
        let stat = fs::read_to_string("/proc/stat")
            .map_err(|e| DomainError::Unavailable(e.to_string()))?;
        let values: Vec<u64> = stat
            .lines()
            .next()
            .unwrap_or("")
            .split_whitespace()
            .skip(1)
            .take(8)
            .filter_map(|n| n.parse().ok())
            .collect();
        let total = values.iter().sum::<u64>();
        let idle = values.get(3).copied().unwrap_or(0) + values.get(4).copied().unwrap_or(0);
        let cpu = self
            .previous
            .map(|(t, i)| {
                let dt = total.saturating_sub(t);
                if dt == 0 {
                    0.
                } else {
                    100. * (dt.saturating_sub(idle.saturating_sub(i))) as f32 / dt as f32
                }
            })
            .unwrap_or(0.);
        self.previous = Some((total, idle));
        let mem = fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let memory = |name: &str| {
            mem.lines()
                .find(|l| l.starts_with(name))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
                / 1024
        };
        let total = memory("MemTotal:");
        let available = memory("MemAvailable:");
        Ok(SystemSnapshot {
            cpu_percent: cpu,
            memory_used_mib: total.saturating_sub(available),
            memory_total_mib: total,
        })
    }
}
pub fn wallpapers() -> Vec<Wallpaper> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let mut files = vec![];
    for dir in [
        home.join("Pictures/Wallpapers"),
        home.join("Pictures/Wallpapers/Lucent"),
        home.join(".config/lucent/wallpapers"),
        home.join(".config/omarchy/current/theme/backgrounds"),
    ] {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|s| {
                    ["png", "jpg", "jpeg", "webp"]
                        .contains(&s.to_string_lossy().to_lowercase().as_str())
                }) {
                    files.push(Wallpaper {
                        path: path.to_string_lossy().into(),
                        name: path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .replace(['_', '-'], " "),
                    });
                }
            }
        }
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files.dedup_by(|a, b| a.path == b.path);
    files
}
/// Native session clients read the selected immutable asset. Initial stock
/// migration is read-only; explicit selection is owned by WallpaperStore.
pub fn current_wallpaper() -> String {
    wallpapers::WallpaperStore::default()
        .current()
        .ok()
        .flatten()
        .map(|asset| asset.path)
        .unwrap_or_default()
}

pub mod notifications;

pub mod wallpaper_catalog;
