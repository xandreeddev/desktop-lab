//! Independent NotificationPort consumer/server; no desktop client or renderer.
use lucent_domain::*;
use lucent_services::notifications::{FreedesktopNotifications, JsonNotificationHistory};
use std::{
    io::{self, BufRead},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
struct Stop(AtomicBool);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    fn wait(&self, duration: Duration) {
        let deadline = std::time::Instant::now() + duration;
        while !self.cancelled() && std::time::Instant::now() < deadline {
            std::thread::sleep(
                Duration::from_millis(10)
                    .min(deadline.saturating_duration_since(std::time::Instant::now())),
            );
        }
    }
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help") {
        println!(
            "Use while the desktop notification server is stopped. Commands: dnd on|off, dismiss ID, invoke ID ACTION, clear, retry, quit. --memory disables durable history. Snapshot output contains counts and IDs, not notification text."
        );
        return Ok(());
    }
    let history = if args.iter().any(|a| a == "--memory") {
        None
    } else {
        Some(Arc::new(JsonNotificationHistory::default()) as Arc<dyn NotificationHistoryPort>)
    };
    let adapter: Arc<dyn NotificationPort> =
        Arc::new(FreedesktopNotifications::with_history(history));
    let stop = Arc::new(Stop(AtomicBool::new(false)));
    let current = Arc::new(Mutex::new(NotificationSnapshot::default()));
    let worker = {
        let adapter = adapter.clone();
        let stop = stop.clone();
        let current = current.clone();
        std::thread::spawn(move || {
            adapter.watch(
                &mut |value| match value {
                    Ok(snapshot) => {
                        println!(
                            "active={} history={} dnd={} storage={:?} ids={:?}",
                            snapshot.active.len(),
                            snapshot.history.len(),
                            snapshot.do_not_disturb,
                            snapshot.history_status,
                            snapshot.active.iter().map(|n| n.id).collect::<Vec<_>>()
                        );
                        *current.lock().unwrap() = snapshot;
                    }
                    Err(_) => eprintln!("Notification delivery unavailable"),
                },
                stop.as_ref(),
            )
        })
    };
    let commands = (|| -> io::Result<()> {
        for line in io::stdin().lock().lines() {
            let line = line?;
            let mut words = line.splitn(3, ' ');
            let command = words.next().unwrap_or("");
            if command == "quit" {
                break;
            }
            let result: Result<()> = match command {
                "clear" => adapter.clear_history(),
                "retry" => adapter.retry_history(),
                "dnd" => match words.next() {
                    Some("on") => adapter.set_do_not_disturb(true),
                    Some("off") => adapter.set_do_not_disturb(false),
                    _ => Err(DomainError::Invalid("Use dnd on or off".into())),
                },
                "dismiss" | "invoke" => {
                    let target =
                        words
                            .next()
                            .and_then(|id| id.parse::<u32>().ok())
                            .and_then(|id| {
                                current
                                    .lock()
                                    .unwrap()
                                    .active
                                    .iter()
                                    .find(|n| n.id == id)
                                    .and_then(Notification::target)
                            });
                    match target {
                        Some(target) if command == "dismiss" => adapter.dismiss(target),
                        Some(target) => adapter.invoke(target, words.next().unwrap_or("")),
                        None => Err(DomainError::Invalid(
                            "Current notification ID required".into(),
                        )),
                    }
                }
                _ => Err(DomainError::Invalid("Unknown command".into())),
            };
            if let Err(error) = result {
                eprintln!("{error}");
            }
        }
        Ok(())
    })();
    stop.0.store(true, Ordering::Relaxed);
    worker
        .join()
        .map_err(|_| io::Error::other("Notification observer failed"))?;
    commands?;
    Ok(())
}
