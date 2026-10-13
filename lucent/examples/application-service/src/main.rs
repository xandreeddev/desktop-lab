//! Independent application catalog/launch consumer; no Wayland or desktop dependency.
use lucent_domain::*;
use lucent_services::applications::XdgApplications;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
struct Stop(AtomicBool);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn wait(&self, d: Duration) {
        let end = Instant::now() + d;
        while !self.cancelled() && Instant::now() < end {
            thread::sleep(Duration::from_millis(25));
        }
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut roots = vec![];
    let mut seconds = 5u64;
    let mut list = false;
    let mut launch = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--list" => list = true,
            "--observe" => (),
            "--data-dir" => {
                i += 1;
                roots.push(PathBuf::from(args.get(i).ok_or_else(|| {
                    DomainError::Invalid("Data directory required".into())
                })?));
            }
            "--seconds" => {
                i += 1;
                seconds = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .filter(|s| *s > 0 && *s <= 600)
                    .ok_or_else(|| DomainError::Invalid("Seconds must be 1..600".into()))?;
            }
            "--launch" => {
                i += 1;
                launch = Some(AppId::new(args.get(i).ok_or_else(|| {
                    DomainError::Invalid("Application ID required".into())
                })?)?);
            }
            _ => {
                return Err(DomainError::Invalid(
                    "application-service [--observe|--list|--launch ID] [--seconds 1..600] [--data-dir ABSOLUTE_ROOT]".into(),
                ));
            }
        }
        i += 1;
    }
    let source = if roots.is_empty() {
        XdgApplications::default()
    } else {
        XdgApplications::with_source(roots, "Hyprland".into(), String::new())?
    };
    if let Some(id) = launch {
        let apps = source.discover()?;
        let app = apps
            .iter()
            .find(|app| app.id == id)
            .ok_or_else(|| DomainError::Invalid("Application is not installed".into()))?;
        source.launch(app)?;
        println!("{}", serde_json::json!({"exec_acknowledged":true}));
        return Ok(());
    }
    if list {
        let apps = source.discover()?;
        println!(
            "{}",
            serde_json::json!({"items":apps.iter().map(|app|serde_json::json!({"id":app.id.as_str(),"name":app.name})).collect::<Vec<_>>()})
        );
        return Ok(());
    }
    let stop = Arc::new(Stop(AtomicBool::new(false)));
    let s = stop.clone();
    let worker = thread::spawn(move || {
        source.watch(
            &mut |value| {
                println!(
                    "{}",
                    match value {
                        Ok(apps) => serde_json::json!({"state":"ready","count":apps.len()}),
                        Err(error) =>
                            serde_json::json!({"state":"unavailable","error":error.to_string()}),
                    }
                )
            },
            s.as_ref(),
        )
    });
    thread::sleep(Duration::from_secs(seconds));
    stop.0.store(true, Ordering::Release);
    worker
        .join()
        .map_err(|_| DomainError::Failed("Application observer failed".into()))?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
