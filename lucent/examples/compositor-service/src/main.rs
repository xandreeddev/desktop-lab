//! Read-only independent CompositorPort consumer. No UI/renderer dependencies.
use lucent_domain::{CompositorPort, StopSignal};
use std::{
    thread,
    time::{Duration, Instant},
};
struct Stop(Instant);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        Instant::now() >= self.0
    }
    fn wait(&self, duration: Duration) {
        thread::sleep(duration.min(self.0.saturating_duration_since(Instant::now())));
    }
}
fn summary(snapshot: lucent_domain::Result<lucent_domain::CompositorSnapshot>) {
    match snapshot {
        Ok(value) => println!(
            "monitors={} workspaces={} windows={} unique_focus={}",
            value.monitors.len(),
            value.workspaces.len(),
            value.windows.len(),
            value.focused_monitor().is_some()
        ),
        Err(error) => eprintln!("{error}"),
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help") {
        println!(
            "compositor-service --snapshot | --observe [--seconds 1..600]\nRead-only CompositorPort consumer. Prints counts only; no display/window metadata."
        );
        return Ok(());
    }
    let adapter = lucent_services::compositor::Hyprland::from_env()?;
    match args.as_slice() {
        [mode] if mode == "--snapshot" => summary(Ok(adapter.snapshot()?)),
        [mode] if mode == "--observe" => {
            adapter.watch(&mut summary, &Stop(Instant::now() + Duration::from_secs(5)))
        }
        [mode, flag, seconds] if mode == "--observe" && flag == "--seconds" => {
            let seconds: u64 = seconds.parse()?;
            if !(1..=600).contains(&seconds) {
                return Err("Seconds must be 1..600".into());
            }
            adapter.watch(
                &mut summary,
                &Stop(Instant::now() + Duration::from_secs(seconds)),
            );
        }
        _ => return Err("Use --snapshot or --observe [--seconds 1..600]".into()),
    }
    Ok(())
}
