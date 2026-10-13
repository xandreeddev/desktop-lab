//! Independent library consumer; explicit stdio opts into application actions.
use lucent_domain::*;
use lucent_services::tray::StatusNotifier;
use std::{
    io::{self, BufRead},
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
fn print(value: serde_json::Value) {
    println!("{value}");
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let stdio = args.iter().any(|a| a == "--stdio");
    let mut seconds = 5u64;
    let mut address = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--stdio" | "--observe" => (),
            "--seconds" => {
                i += 1;
                seconds = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .filter(|s| *s > 0 && *s <= 600)
                    .expect("seconds must be 1..600");
            }
            "--bus-address" => {
                i += 1;
                address = Some(args.get(i).expect("bus address required").clone());
            }
            _ => {
                eprintln!(
                    "tray-service [--observe|--stdio] [--seconds 1..600] [--bus-address fixture-address]"
                );
                return;
            }
        }
        i += 1;
    }
    let adapter = Arc::new(
        address
            .map(StatusNotifier::with_bus_address)
            .unwrap_or_default(),
    );
    let stop = Arc::new(Stop(AtomicBool::new(false)));
    let (a, s) = (adapter.clone(), stop.clone());
    let worker = thread::spawn(move || {
        a.watch(&mut|result|match result{
  Ok(snapshot)=>print(serde_json::json!({"state":"ready","count":snapshot.items.len(),"items":if stdio{Some(snapshot.items.iter().map(|i|serde_json::json!({"id":i.id.value(),"title":i.title,"menu":i.has_menu})).collect::<Vec<_>>())}else{None}})),
  Err(error)=>print(serde_json::json!({"state":"unavailable","error":error.to_string()})),
 },s.as_ref())
    });
    if stdio {
        let mut input = io::stdin().lock();
        loop {
            let mut bytes = vec![];
            let mut oversized = false;
            loop {
                let available = input.fill_buf().unwrap();
                if available.is_empty() {
                    break;
                }
                let n = available
                    .iter()
                    .position(|b| *b == b'\n')
                    .map(|n| n + 1)
                    .unwrap_or(available.len());
                let done = available[n - 1] == b'\n';
                if bytes.len() + n <= 4096 {
                    bytes.extend_from_slice(&available[..n]);
                } else {
                    oversized = true;
                }
                input.consume(n);
                if done {
                    break;
                }
            }
            if bytes.is_empty() && !oversized {
                break;
            }
            if oversized {
                print(serde_json::json!({"error":"command exceeds 4096 bytes"}));
                continue;
            }
            let line = String::from_utf8_lossy(&bytes);
            let values: Vec<_> = line.split_whitespace().collect();
            if values.first() == Some(&"quit") {
                break;
            }
            let result = (|| -> Result<serde_json::Value> {
                let id = values
                    .get(1)
                    .and_then(|s| s.parse().ok())
                    .and_then(|id| TrayItemId::new(id).ok())
                    .ok_or_else(|| DomainError::Invalid("Current item ID required".into()))?;
                match values.first().copied() {
                    Some("activate") => {
                        adapter.act(id, TrayAction::Activate { x: 0, y: 0 })?;
                        Ok(serde_json::json!({"acknowledged":true}))
                    }
                    Some("menu") => {
                        let menu = adapter.menu(id, 0)?;
                        Ok(
                            serde_json::json!({"item":id.value(),"revision":menu.revision,"rows":menu.root.children.iter().map(|e|serde_json::json!({"id":e.id,"label":e.label,"enabled":e.enabled,"visible":e.visible})).collect::<Vec<_>>()}),
                        )
                    }
                    Some("invoke") => {
                        let revision = values
                            .get(2)
                            .and_then(|s| s.parse().ok())
                            .ok_or_else(|| DomainError::Invalid("Menu revision required".into()))?;
                        let entry = values
                            .get(3)
                            .and_then(|s| s.parse().ok())
                            .ok_or_else(|| DomainError::Invalid("Menu entry required".into()))?;
                        adapter.invoke(id, revision, entry, 0)?;
                        Ok(serde_json::json!({"acknowledged":true}))
                    }
                    _ => Err(DomainError::Invalid(
                        "Use activate ID, menu ID, invoke ID REVISION ENTRY or quit".into(),
                    )),
                }
            })();
            print(match result {
                Ok(value) => value,
                Err(e) => serde_json::json!({"error":e.to_string()}),
            });
        }
    } else {
        thread::sleep(Duration::from_secs(seconds));
    }
    stop.0.store(true, Ordering::Release);
    worker.join().unwrap();
}
