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
    let mut action = None;
    let mut open = None;
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
            "--open" => {
                i += 1;
                let id = AppId::new(
                    args.get(i)
                        .ok_or_else(|| DomainError::Invalid("Application ID required".into()))?,
                )?;
                let mut targets = vec![];
                i += 1;
                while i < args.len() {
                    let value = &args[i];
                    targets.push(if value.starts_with('/') {
                        ApplicationOpenTarget::file(value)?
                    } else {
                        ApplicationOpenTarget::uri(ExternalUri::new(value)?)
                    });
                    i += 1;
                }
                open = Some((id, ApplicationOpenRequest::new(targets)?));
            }
            "--action" => {
                i += 1;
                let app = AppId::new(
                    args.get(i)
                        .ok_or_else(|| DomainError::Invalid("Application ID required".into()))?,
                )?;
                i += 1;
                let id = ApplicationActionId::new(
                    args.get(i)
                        .ok_or_else(|| DomainError::Invalid("Action ID required".into()))?,
                )?;
                action = Some((app, id));
            }
            _ => {
                return Err(DomainError::Invalid(
                    "application-service [--observe|--list|--launch ID|--action APP_ID ACTION_ID|--open APP_ID FILE_OR_URI ...] [--seconds 1..600] [--data-dir ABSOLUTE_ROOT]".into(),
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
    if [action.is_some(), launch.is_some(), open.is_some(), list]
        .into_iter()
        .filter(|value| *value)
        .count()
        > 1
    {
        return Err(DomainError::Invalid(
            "Choose one application operation".into(),
        ));
    }
    if let Some((id, request)) = open {
        let apps = source.discover()?;
        let app = apps
            .iter()
            .find(|app| app.id == id)
            .ok_or_else(|| DomainError::Invalid("Application is not installed".into()))?;
        source.open(app, &request, ApplicationLaunchContext::default())?;
        println!("{}", serde_json::json!({"open_acknowledged":true}));
        return Ok(());
    }
    if let Some((id, action)) = action {
        let apps = source.discover()?;
        let app = apps
            .iter()
            .find(|app| app.id == id)
            .ok_or_else(|| DomainError::Invalid("Application is not installed".into()))?;
        source.launch_action(app, &action)?;
        // This is a native method/exec acknowledgement, not observed app state.
        println!("{}", serde_json::json!({"action_acknowledged":true}));
        return Ok(());
    }
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
            serde_json::json!({"items":apps.iter().map(|app|serde_json::json!({"id":app.id.as_str(),"name":app.name,"actions":app.actions.iter().map(|action|serde_json::json!({"id":action.id.as_str(),"name":action.name})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
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
