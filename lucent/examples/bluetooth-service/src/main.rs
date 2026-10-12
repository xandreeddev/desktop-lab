//! Independent consumer of BluetoothPort. Observation prints counts only.
//! --stdio opts into local metadata and accepts explicit controls on stdin.
use lucent_domain::*;
use lucent_services::bluetooth::BlueZ;
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Stop(AtomicBool);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn wait(&self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while !self.cancelled() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
    }
}
fn send(value: Value) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{value}");
    let _ = stdout.flush();
}
fn id(value: &Value) -> Result<BluetoothObjectId> {
    value
        .as_u64()
        .ok_or_else(|| DomainError::Invalid("Expected a Bluetooth handle".into()))
        .and_then(BluetoothObjectId::new)
}
fn boolean(value: &Value) -> Result<bool> {
    value
        .as_bool()
        .ok_or_else(|| DomainError::Invalid("Expected a boolean flag".into()))
}
fn command(value: &Value) -> Result<BluetoothCommand> {
    Ok(match value["command"].as_str().unwrap_or("") {
        "power" => BluetoothCommand::SetPowered {
            adapter: id(&value["adapter"])?,
            powered: boolean(&value["powered"])?,
        },
        "discovery" => BluetoothCommand::SetDiscovery {
            adapter: id(&value["adapter"])?,
            active: boolean(&value["active"])?,
        },
        "connect" => BluetoothCommand::Connect(id(&value["device"])?),
        "disconnect" => BluetoothCommand::Disconnect(id(&value["device"])?),
        "trust" => BluetoothCommand::SetTrusted {
            device: id(&value["device"])?,
            trusted: boolean(&value["trusted"])?,
        },
        "forget" => BluetoothCommand::Forget(id(&value["device"])?),
        _ => return Err(DomainError::Invalid("Unknown Bluetooth command".into())),
    })
}
fn state(value: &BluetoothSnapshot, detailed: bool) -> Value {
    if !detailed {
        return json!({"adapters":value.adapters.len(),"devices":value.devices.len(),
            "connected":value.devices.iter().filter(|d|d.connected).count()});
    }
    json!({
        "adapters":value.adapters.iter().map(|a|json!({
            "id":a.id.value(),"address":a.address.to_string(),"name":a.name,"powered":a.powered,
            "discoverable":a.discoverable,"discovering":a.discovering,"discovery_owned":a.discovery_owned,
        })).collect::<Vec<_>>(),
        "devices":value.devices.iter().map(|d|json!({
            "id":d.id.value(),"adapter":d.adapter.value(),"address":d.address.to_string(),
            "name":d.name,"icon":d.icon,"paired":d.paired,"bonded":d.bonded,"trusted":d.trusted,
            "blocked":d.blocked,"connected":d.connected,"services_resolved":d.services_resolved,
            "rssi":d.rssi,"battery_percent":d.battery_percent,
        })).collect::<Vec<_>>(),
    })
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let mut detailed = false;
    let mut seconds = 10_u64;
    let mut address = None;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--stdio"=>detailed=true,
            "--observe"=>(),
            "--seconds"=>seconds=arguments.next().ok_or("Missing duration")?.parse()?,
            "--bus-address"=>address=Some(arguments.next().ok_or("Missing test bus address")?),
            _=>return Err("Use --observe [--seconds 1..600] or --stdio; --bus-address selects an isolated bus".into()),
        }
    }
    if !(1..=600).contains(&seconds) {
        return Err("Duration must be 1..600 seconds".into());
    }
    let adapter: Arc<dyn BluetoothPort> = Arc::new(match address {
        Some(value) => BlueZ::with_address(value),
        None => BlueZ::default(),
    });
    let stop = Arc::new(Stop::default());
    let background = adapter.clone();
    let signal = stop.clone();
    let observer = thread::spawn(move || {
        background.watch(
            &mut |result| match result {
                Ok(snapshot) => send(json!({"kind":"state","value":state(&snapshot,detailed)})),
                Err(error) => send(json!({"kind":"unavailable","error":error.to_string()})),
            },
            signal.as_ref(),
        )
    });
    let input_result: io::Result<()> = if detailed {
        (|| {
            let mut stdin = io::stdin().lock();
            loop {
                // Bound each admitted command instead of allocating an unbounded line.
                let mut line = vec![];
                let size = stdin.by_ref().take(4097).read_until(b'\n', &mut line)?;
                if size == 0 {
                    break;
                }
                if size > 4096 {
                    send(json!({"kind":"error","error":"Bluetooth command exceeds the limit"}));
                    break;
                }
                let result = serde_json::from_slice::<Value>(&line)
                    .map_err(|_| DomainError::Invalid("Invalid Bluetooth command JSON".into()))
                    .and_then(|v| command(&v))
                    .and_then(|c| adapter.control(c));
                match result {
                    Ok(()) => send(json!({"kind":"acknowledged"})),
                    Err(error) => send(json!({"kind":"error","error":error.to_string()})),
                }
            }
            Ok(())
        })()
    } else {
        stop.wait(Duration::from_secs(seconds));
        Ok(())
    };
    stop.0.store(true, Ordering::Release);
    observer.join().map_err(|_| "Bluetooth observer failed")?;
    input_result?;
    Ok(())
}
