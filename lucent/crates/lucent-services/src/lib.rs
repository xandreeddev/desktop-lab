//! OS adapters. All blocking operations are called from effects/subscription workers.
pub mod applications;
pub mod compositor;
pub mod desktop_adapters;
pub mod images;
use chrono::{Datelike, Timelike};
use lucent_domain::*;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
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
#[derive(Clone)]
pub struct JsonSettings {
    pub path: PathBuf,
}
impl Default for JsonSettings {
    fn default() -> Self {
        Self {
            path: state_directory().join("desktop.json"),
        }
    }
}
impl SettingsPort for JsonSettings {
    fn load(&self) -> Result<DesktopSettings> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                let value: DesktopSettings = serde_json::from_slice(&bytes)
                    .map_err(|e| DomainError::Invalid(format!("Settings: {e}")))?;
                lucent_usecases::validate_settings(&value)?;
                Ok(value)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DesktopSettings::default()),
            Err(e) => Err(DomainError::Failed(e.to_string())),
        }
    }
    fn save(&self, settings: &DesktopSettings) -> Result<()> {
        lucent_usecases::validate_settings(settings)?;
        let save = || -> std::io::Result<()> {
            fs::create_dir_all(self.path.parent().unwrap())?;
            let temporary = self.path.with_extension("tmp");
            fs::write(&temporary, serde_json::to_vec_pretty(settings)?)?;
            fs::rename(temporary, &self.path)
        };
        save().map_err(|e| DomainError::Failed(format!("Could not save desktop settings: {e}")))
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

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
        settings.launcher = LauncherSize {
            width: Some(800),
            max_height: Some(720),
        };
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap(), settings);
        let saved = fs::read(&store.path).unwrap();
        settings.launcher.width = Some(0);
        assert!(store.save(&settings).is_err());
        assert_eq!(fs::read(&store.path).unwrap(), saved);
        fs::write(&store.path, br#"{"version":1,"launcher":{"max_height":0}}"#).unwrap();
        assert!(store.load().is_err());
        fs::write(&store.path, br#"{"version":2}"#).unwrap();
        assert!(store.load().is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
/// Bounded command adapter. stdout is drained concurrently; timeouts kill and reap the child.
pub fn command(program: &str, args: &[&str]) -> Result<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| DomainError::Unavailable(format!("{program}: {e}")))?;
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout).take(8 * 1024 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let bytes = reader.join().unwrap_or_default();
                if status.success() {
                    return Ok(String::from_utf8_lossy(&bytes).trim().into());
                }
                return Err(DomainError::Unavailable(format!(
                    "{program} reported {}",
                    status.code().unwrap_or(-1)
                )));
            }
            Ok(None) => {}
            Err(e) => return Err(DomainError::Failed(e.to_string())),
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(DomainError::Unavailable(format!("{program} timed out")));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub fn clock() -> ClockSnapshot {
    let now = chrono::Local::now();
    ClockSnapshot {
        date: Date {
            year: now.year(),
            month: now.month(),
            day: now.day(),
            weekday: now.weekday().num_days_from_monday(),
        },
        hour: now.hour(),
        minute: now.minute(),
        unix_seconds: now.timestamp().max(0) as u64,
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
        let volume = command("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]).ok();
        let network = command("nmcli", &["-t", "-f", "STATE", "general"])
            .unwrap_or_else(|_| "Unavailable".into());
        Ok(SystemSnapshot {
            cpu_percent: cpu,
            memory_used_mib: total.saturating_sub(available),
            memory_total_mib: total,
            volume: volume
                .as_ref()
                .and_then(|s| s.split_whitespace().nth(1))
                .and_then(|n| n.parse::<f32>().ok())
                .map(|v| (v * 100.).round() as u32),
            muted: volume.is_some_and(|s| s.contains("MUTED")),
            network,
        })
    }
}
pub fn media() -> Result<MediaSnapshot> {
    let data = command(
        "playerctl",
        &[
            "metadata",
            "--format",
            "{{playerName}}\t{{title}}\t{{artist}}\t{{status}}\t{{mpris:artUrl}}\t{{mpris:length}}",
        ],
    )?;
    let p: Vec<_> = data.split('\t').collect();
    let get = |n| p.get(n).copied().unwrap_or("").to_string();
    let position = command("playerctl", &["position"])
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.) as u64;
    Ok(MediaSnapshot {
        player: get(0),
        title: get(1),
        artist: get(2),
        playing: get(3) == "Playing",
        art_url: get(4),
        position,
        length: get(5).parse::<u64>().unwrap_or(0) / 1_000_000,
    })
}
pub fn weather() -> Result<WeatherSnapshot> {
    let path = config_directory().join("weather.json");
    let bytes = fs::read(path).map_err(|_| {
        DomainError::Unavailable("Choose a weather location in configuration".into())
    })?;
    let config: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| DomainError::Invalid(e.to_string()))?;
    let lat = config["latitude"]
        .as_f64()
        .filter(|v| v.abs() <= 90.)
        .ok_or_else(|| DomainError::Invalid("Invalid latitude".into()))?;
    let lon = config["longitude"]
        .as_f64()
        .filter(|v| v.abs() <= 180.)
        .ok_or_else(|| DomainError::Invalid("Invalid longitude".into()))?;
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,weather_code"
    );
    let body = command("curl", &["-fsSL", "--max-time", "4", &url])?;
    let value: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| DomainError::Failed(e.to_string()))?;
    let current = &value["current"];
    let code = current["weather_code"]
        .as_u64()
        .ok_or_else(|| DomainError::Failed("Weather response missing current conditions".into()))?
        as u32;
    let temperature = current["temperature_2m"]
        .as_f64()
        .ok_or_else(|| DomainError::Failed("Weather response missing temperature".into()))?
        as f32;
    let description = match code {
        0 => "Clear sky",
        1 | 2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51..=67 => "Rain",
        71..=77 => "Snow",
        80..=82 => "Showers",
        85 | 86 => "Snow showers",
        95..=99 => "Thunderstorm",
        _ => "Cloudy",
    };
    Ok(WeatherSnapshot {
        temperature,
        description: description.into(),
        code,
    })
}
pub fn wallpapers() -> Vec<Wallpaper> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let mut files = vec![];
    for dir in [
        home.join("Pictures/Wallpapers"),
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
pub fn apply_wallpaper(path: &str) -> Result<()> {
    let path = Path::new(path);
    if !path.is_absolute() || !path.is_file() {
        return Err(DomainError::Invalid(
            "Wallpaper must be an existing absolute file path".into(),
        ));
    }
    // Omarchy owns the background layer and persistence. Its adapter avoids a
    // competing wallpaper daemon and updates the normal theme background link.
    command(
        "omarchy-theme-bg-set",
        &[path
            .to_str()
            .ok_or_else(|| DomainError::Invalid("Invalid wallpaper path".into()))?],
    )
    .map(|_| ())
}

/// Omarchy owns the current wallpaper link; the client reads it without changing it.
pub fn current_wallpaper() -> String {
    std::env::var_os("HOME")
        .and_then(|home| {
            fs::canonicalize(PathBuf::from(home).join(".local/state/omarchy/current/background"))
                .ok()
        })
        .map(|p| p.to_string_lossy().into())
        .unwrap_or_default()
}

pub mod notifications;
