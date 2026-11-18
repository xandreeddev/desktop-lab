use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const INITIAL: &str = "cpu 100 0 50 850 0 0 0 0 20 0\n";
const NEXT: &str = "cpu 150 0 50 900 0 0 0 0 40 0\n";
const MEMORY: &str = "MemTotal: 8192 kB\nMemAvailable: 3072 kB\n";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lucent-system-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let fixture = Self(root);
        fixture.write("stat", INITIAL);
        fixture.write("meminfo", MEMORY);
        fixture
    }
    fn write(&self, name: &str, content: impl AsRef<[u8]>) {
        fs::write(self.0.join(name), content).unwrap();
    }
    fn sample(&self, probe: &mut SystemProbe) -> Result<SystemSnapshot> {
        probe.sample_at(&self.0)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn complete_samples_distinguish_cpu_warmup_from_measured_busy_and_idle_intervals() {
    let fixture = Fixture::new();
    let mut probe = SystemProbe::default();
    let initial = fixture.sample(&mut probe).unwrap();
    assert_eq!(initial.cpu_percent, None);
    assert_eq!((initial.memory_used_mib, initial.memory_total_mib), (5, 8));
    assert!(initial.validate().is_ok());
    fixture.write("stat", NEXT);
    assert_eq!(fixture.sample(&mut probe).unwrap().cpu_percent, Some(50.));
    fixture.write("stat", "cpu 150 0 50 1000 0 0 0 0 40 0\n");
    assert_eq!(fixture.sample(&mut probe).unwrap().cpu_percent, Some(0.));
    fixture.write("stat", "cpu 250 0 50 1000 0 0 0 0 140 0\n");
    assert_eq!(fixture.sample(&mut probe).unwrap().cpu_percent, Some(100.));
}

#[test]
fn memory_conversion_subtracts_before_truncating_to_mib() {
    let fixture = Fixture::new();
    fixture.write("meminfo", "MemAvailable: 1023 kB\nMemTotal: 2048 kB\n");
    let sample = fixture.sample(&mut SystemProbe::default()).unwrap();
    assert_eq!((sample.memory_used_mib, sample.memory_total_mib), (1, 2));
}

#[test]
fn invalid_cpu_columns_never_shift_or_silently_create_a_sample() {
    let fixture = Fixture::new();
    for content in [
        "",
        "cpu0 100 0 50 850 0 0 0 0\n",
        "cpu 100 0 50 850\n",
        "cpu 100 0 malformed 850 0 0 0 0\n",
        "cpu -100 0 50 850 0 0 0 0\n",
        "cpu +100 0 50 850 0 0 0 0\n",
        "cpu 100 0 50 850 0 0 0 0 malformed\n",
        "cpu 18446744073709551616 0 0 0 0 0 0 0\n",
        "cpu 18446744073709551615 0 0 1 0 0 0 0\n",
        "cpu 0 0 0 18446744073709551615 1 0 0 0\n",
    ] {
        fixture.write("stat", content);
        assert!(
            fixture.sample(&mut SystemProbe::default()).is_err(),
            "{content}"
        );
    }
}

#[test]
fn memory_requires_unique_complete_consistent_kib_fields() {
    let fixture = Fixture::new();
    for content in [
        "",
        "MemTotal: 8192 kB\n",
        "MemAvailable: 3072 kB\n",
        "MemTotal: 8192 kB\nMemTotal: 8192 kB\nMemAvailable: 3072 kB\n",
        "MemTotal: 8192 kB\nMemAvailable: 3072 kB\nMemAvailable: 3072 kB\n",
        "MemTotal: 8192 MB\nMemAvailable: 3072 kB\n",
        "MemTotal: 8192 kB extra\nMemAvailable: 3072 kB\n",
        "MemTotal: 8192 kB\nMemAvailable: -1 kB\n",
        "MemTotal: 8192 kB\nMemAvailable: 8193 kB\n",
        "MemTotal: 0 kB\nMemAvailable: 0 kB\n",
        "MemTotal: 1023 kB\nMemAvailable: 0 kB\n",
        "MemTotal: 18446744073709551616 kB\nMemAvailable: 0 kB\n",
    ] {
        fixture.write("meminfo", content);
        assert!(
            fixture.sample(&mut SystemProbe::default()).is_err(),
            "{content}"
        );
    }
}

#[test]
fn a_decreasing_counter_rebases_even_when_total_cpu_time_increases() {
    let fixture = Fixture::new();
    for decreasing in 0..8 {
        let mut probe = SystemProbe::default();
        fixture.write("stat", "cpu 100 100 100 100 100 100 100 100\n");
        assert_eq!(fixture.sample(&mut probe).unwrap().cpu_percent, None);
        let counters = (0..8)
            .map(|index| if index == decreasing { "90" } else { "200" })
            .collect::<Vec<_>>()
            .join(" ");
        fixture.write("stat", format!("cpu {counters}\n"));
        assert_eq!(fixture.sample(&mut probe).unwrap().cpu_percent, None);
        fixture.write("stat", "cpu 300 300 300 300 300 300 300 300\n");
        let recovered = fixture.sample(&mut probe).unwrap();
        assert!(recovered.cpu_percent.is_some());
        assert!(recovered.validate().is_ok());
        assert_eq!(fixture.sample(&mut probe).unwrap().cpu_percent, None);
    }
}

#[test]
fn failed_reads_or_parses_retire_the_cpu_interval_until_two_complete_samples() {
    for failure in ["missing-stat", "missing-memory", "bad-stat", "bad-memory"] {
        let fixture = Fixture::new();
        let mut probe = SystemProbe::default();
        fixture.sample(&mut probe).unwrap();
        fixture.write("stat", NEXT);
        assert_eq!(fixture.sample(&mut probe).unwrap().cpu_percent, Some(50.));
        match failure {
            "missing-stat" => fs::remove_file(fixture.0.join("stat")).unwrap(),
            "missing-memory" => fs::remove_file(fixture.0.join("meminfo")).unwrap(),
            "bad-stat" => fixture.write("stat", "cpu 0 invalid\n"),
            _ => fixture.write("meminfo", "MemTotal: 8192 kB\n"),
        }
        assert!(fixture.sample(&mut probe).is_err());
        assert!(probe.previous.is_none());
        fixture.write("stat", "cpu 250 0 50 1000 0 0 0 0\n");
        fixture.write("meminfo", MEMORY);
        assert_eq!(fixture.sample(&mut probe).unwrap().cpu_percent, None);
        fixture.write("stat", "cpu 300 0 50 1050 0 0 0 0\n");
        assert_eq!(fixture.sample(&mut probe).unwrap().cpu_percent, Some(50.));
    }
}

#[test]
fn reads_are_bounded_and_the_aggregate_does_not_read_per_cpu_tail_data() {
    let fixture = Fixture::new();
    fixture.write("stat", format!("cpu {}\n", "0 ".repeat(CPU_LINE_LIMIT)));
    assert!(fixture.sample(&mut SystemProbe::default()).is_err());
    fixture.write(
        "stat",
        format!(
            "{INITIAL}{}",
            "ignored per-CPU data\n".repeat(CPU_LINE_LIMIT)
        ),
    );
    assert!(fixture.sample(&mut SystemProbe::default()).is_ok());
    fixture.write("meminfo", format!("{MEMORY}{}", "x".repeat(MEMORY_LIMIT)));
    assert!(fixture.sample(&mut SystemProbe::default()).is_err());
}

#[test]
fn non_regular_and_non_utf8_inputs_return_errors() {
    for name in ["stat", "meminfo"] {
        let fixture = Fixture::new();
        fixture.write(name, [255]);
        assert!(fixture.sample(&mut SystemProbe::default()).is_err());
        fs::remove_file(fixture.0.join(name)).unwrap();
        fs::create_dir(fixture.0.join(name)).unwrap();
        assert!(fixture.sample(&mut SystemProbe::default()).is_err());
    }
}
