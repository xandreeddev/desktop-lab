//! Bounded Linux observations. A CPU interval requires two complete samples.
use lucent_domain::{DomainError, Result, SystemSnapshot};
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

const CPU_LINE_LIMIT: usize = 4096;
const MEMORY_LIMIT: usize = 64 * 1024;

fn invalid() -> DomainError {
    DomainError::Invalid("Incomplete or invalid system statistics".into())
}
fn unavailable(error: std::io::Error) -> DomainError {
    DomainError::Unavailable(format!("System statistics could not be read: {error}"))
}
fn open(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(unavailable)?;
    if !file.metadata().map_err(unavailable)?.is_file() {
        return Err(invalid());
    }
    Ok(file)
}
fn cpu_input(path: &Path) -> Result<String> {
    let mut input = String::new();
    // /proc/stat can contain many per-CPU lines; only its aggregate is needed.
    BufReader::new(open(path)?.take((CPU_LINE_LIMIT + 1) as u64))
        .read_line(&mut input)
        .map_err(unavailable)?;
    if input.len() > CPU_LINE_LIMIT {
        return Err(invalid());
    }
    Ok(input)
}
fn memory_input(path: &Path) -> Result<String> {
    let mut input = String::new();
    open(path)?
        .take((MEMORY_LIMIT + 1) as u64)
        .read_to_string(&mut input)
        .map_err(unavailable)?;
    if input.len() > MEMORY_LIMIT {
        return Err(invalid());
    }
    Ok(input)
}
fn number(value: &str) -> Result<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    value.parse().map_err(|_| invalid())
}

#[derive(Clone, Copy)]
struct Cpu {
    counters: [u64; 8],
    total: u64,
    idle: u64,
}
impl Cpu {
    fn parse(input: &str) -> Result<Self> {
        let mut fields = input.split_whitespace();
        if fields.next() != Some("cpu") {
            return Err(invalid());
        }
        let mut counters = [0; 8];
        for counter in &mut counters {
            *counter = number(fields.next().ok_or_else(invalid)?)?;
        }
        // Guest counters are already included in user/nice. Validate optional
        // trailing counters without counting that execution twice.
        for field in fields {
            number(field)?;
        }
        let total = counters
            .iter()
            .try_fold(0u64, |sum, value| sum.checked_add(*value))
            .ok_or_else(invalid)?;
        let idle = counters[3].checked_add(counters[4]).ok_or_else(invalid)?;
        Ok(Self {
            counters,
            total,
            idle,
        })
    }
    fn percent_since(self, previous: Self) -> Option<f32> {
        if self
            .counters
            .iter()
            .zip(previous.counters)
            .any(|(current, previous)| *current < previous)
        {
            // Linux can decrease iowait; counter resets also need a new basis.
            return None;
        }
        let total = self.total.checked_sub(previous.total)?;
        if total == 0 {
            return None;
        }
        let idle = self.idle.checked_sub(previous.idle)?;
        let busy = total.checked_sub(idle)?;
        Some((100. * busy as f64 / total as f64) as f32)
    }
}
fn memory(input: &str) -> Result<(u64, u64)> {
    let (mut total, mut available) = (None, None);
    for line in input.lines() {
        let mut fields = line.split_whitespace();
        let destination = match fields.next() {
            Some("MemTotal:") => &mut total,
            Some("MemAvailable:") => &mut available,
            _ => continue,
        };
        if destination.is_some() {
            return Err(invalid());
        }
        let value = number(fields.next().ok_or_else(invalid)?)?;
        if fields.next() != Some("kB") || fields.next().is_some() {
            return Err(invalid());
        }
        *destination = Some(value);
    }
    let total = total.ok_or_else(invalid)?;
    let available = available.ok_or_else(invalid)?;
    if total < 1024 || available > total {
        return Err(invalid());
    }
    Ok(((total - available) / 1024, total / 1024))
}

#[derive(Default)]
pub struct SystemProbe {
    previous: Option<Cpu>,
}
impl SystemProbe {
    pub fn sample(&mut self) -> Result<SystemSnapshot> {
        self.sample_at(Path::new("/proc"))
    }
    fn sample_at(&mut self, root: &Path) -> Result<SystemSnapshot> {
        let read = || -> Result<(Cpu, u64, u64)> {
            let cpu = Cpu::parse(&cpu_input(&root.join("stat"))?)?;
            let (used, total) = memory(&memory_input(&root.join("meminfo"))?)?;
            Ok((cpu, used, total))
        };
        match read() {
            Ok((cpu, memory_used_mib, memory_total_mib)) => {
                let cpu_percent = self
                    .previous
                    .and_then(|previous| cpu.percent_since(previous));
                self.previous = Some(cpu);
                Ok(SystemSnapshot {
                    cpu_percent,
                    memory_used_mib,
                    memory_total_mib,
                })
            }
            Err(error) => {
                // Never bridge a failed observation with an apparently current
                // CPU percentage. Recovery establishes another complete interval.
                self.previous = None;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests;
