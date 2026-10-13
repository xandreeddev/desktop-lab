//! Headless consumer of NetworkPort, independent of the desktop and renderer.
//! --observe emits counts only. --stdio exposes local network metadata and
//! accepts explicit commands, including credentials through stdin, never argv.
use lucent_domain::*;
use lucent_services::network::NetworkManager;
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
use zeroize::{Zeroize, Zeroizing};

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
    let mut output = io::stdout().lock();
    let _ = writeln!(output, "{value}");
    let _ = output.flush();
}
fn id(value: &Value) -> Result<NetworkObjectId> {
    value
        .as_u64()
        .ok_or_else(|| DomainError::Invalid("Expected a numeric network handle".into()))
        .and_then(NetworkObjectId::new)
}
fn credentials(value: &mut Value) -> Result<WifiCredentials> {
    let security = match value["security"].as_str() {
        Some("open") => WifiSecurity::Open,
        Some("wpa") => WifiSecurity::WpaPersonal,
        Some("sae") => WifiSecurity::Wpa3Personal,
        Some("owe") => WifiSecurity::EnhancedOpen,
        _ => {
            return Err(DomainError::Invalid(
                "Unknown supported security kind".into(),
            ));
        }
    };
    let password = match value["password"].take() {
        Value::Null => None,
        Value::String(password) => Some(Secret::new(password)),
        _ => return Err(DomainError::Invalid("Password must be text".into())),
    };
    WifiCredentials::new(security, password)
}
fn command(value: &mut Value) -> Result<NetworkCommand> {
    Ok(match value["command"].as_str().unwrap_or("") {
        "wifi" => NetworkCommand::SetWifiEnabled(
            value["enabled"]
                .as_bool()
                .ok_or_else(|| DomainError::Invalid("Expected enabled flag".into()))?,
        ),
        "scan" => NetworkCommand::Scan(id(&value["device"])?),
        "activate" => NetworkCommand::Activate {
            profile: id(&value["profile"])?,
            device: if value["device"].is_null() {
                None
            } else {
                Some(id(&value["device"])?)
            },
        },
        "disconnect" => NetworkCommand::Disconnect(id(&value["connection"])?),
        "forget" => NetworkCommand::Forget(id(&value["profile"])?),
        "join" => NetworkCommand::Join {
            target: NetworkJoinTarget::AccessPoint(id(&value["access_point"])?),
            credentials: credentials(value)?,
            remember: value["remember"].as_bool().unwrap_or(false),
        },
        "hidden" => NetworkCommand::Join {
            target: NetworkJoinTarget::Hidden {
                device: id(&value["device"])?,
                ssid: NetworkSsid::new(
                    value["ssid"]
                        .as_str()
                        .ok_or_else(|| DomainError::Invalid("Expected a network name".into()))?
                        .as_bytes()
                        .to_vec(),
                )?,
            },
            credentials: credentials(value)?,
            remember: value["remember"].as_bool().unwrap_or(false),
        },
        _ => return Err(DomainError::Invalid("Unknown network command".into())),
    })
}
fn snapshot(value: &NetworkSnapshot, detailed: bool) -> Value {
    if !detailed {
        return json!({"state":format!("{:?}",value.state),"connectivity":format!("{:?}",value.connectivity),"wifi_enabled":value.wifi_enabled,"devices":value.devices.len(),"profiles":value.profiles.len(),"access_points":value.access_points.len(),"connections":value.connections.len()});
    }
    json!({
        "state":format!("{:?}",value.state),"connectivity":format!("{:?}",value.connectivity),"wifi_enabled":value.wifi_enabled,"wifi_hardware_enabled":value.wifi_hardware_enabled,
        "devices":value.devices.iter().map(|v| json!({"id":v.id.value(),"interface":v.interface,"kind":format!("{:?}",v.kind),"state":format!("{:?}",v.state),"failure":format!("{:?}",v.failure),"scan_generation":v.scan_generation})).collect::<Vec<_>>(),
        "profiles":value.profiles.iter().map(|v| json!({"id":v.id.value(),"key":v.key.as_str(),"name":v.name,"kind":format!("{:?}",v.kind),"ssid":v.ssid.as_ref().map(|v| v.bytes())})).collect::<Vec<_>>(),
        "access_points":value.access_points.iter().map(|v| json!({"id":v.id.value(),"device":v.device.value(),"ssid":v.ssid.as_ref().map(|v| v.bytes()),"strength":v.strength.percent(),"security":format!("{:?}",v.security)})).collect::<Vec<_>>(),
        "connections":value.connections.iter().map(|v| json!({"id":v.id.value(),"profile":v.profile.map(NetworkObjectId::value),"devices":v.devices.iter().map(|v| v.value()).collect::<Vec<_>>(),"state":format!("{:?}",v.state)})).collect::<Vec<_>>(),
    })
}
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (detailed, seconds) = match args.as_slice() {
        [mode] if mode == "--stdio" => (true, None),
        [mode, seconds] if mode == "--observe" => match seconds.parse::<u64>() {
            Ok(n) if (1..=600).contains(&n) => (false, Some(n)),
            _ => {
                eprintln!("Observation duration must be 1–600 seconds");
                return std::process::ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!("Usage: network-service --observe SECONDS | --stdio");
            return std::process::ExitCode::FAILURE;
        }
    };
    let adapter = Arc::new(NetworkManager::default());
    let stop = Arc::new(Stop::default());
    let (a, s) = (adapter.clone(), stop.clone());
    let worker = thread::spawn(move || {
        a.watch(
            &mut |v| match v {
                Ok(v) => send(json!({"event":"state","snapshot":snapshot(&v,detailed)})),
                Err(e) => send(json!({"event":"unavailable","error":e.to_string()})),
            },
            &*s,
        )
    });
    if let Some(seconds) = seconds {
        stop.wait(Duration::from_secs(seconds));
    } else {
        let mut input = io::stdin().lock();
        loop {
            let mut line = Zeroizing::new(String::new());
            // Bound a command line before allocating/decoding its JSON.
            let read = (&mut input).take(16385).read_line(&mut line);
            match read {
                Ok(0) | Err(_) => break,
                Ok(_) if line.len() > 16384 => {
                    send(
                        json!({"event":"result","ok":false,"error":"Command line exceeds 16 KiB"}),
                    );
                    break;
                }
                Ok(_) => (),
            }
            let mut data: Value = match serde_json::from_str::<Value>(&line) {
                Ok(v) if v.is_object() => v,
                _ => {
                    send(json!({"event":"result","ok":false,"error":"Invalid command JSON"}));
                    continue;
                }
            };
            let request_id = data["request_id"].as_u64();
            let result = command(&mut data).and_then(|c| adapter.control(c));
            // Invalid requests may retain an unused JSON password. Erase it too.
            if let Value::String(mut password) = data["password"].take() {
                password.zeroize();
            }
            send(
                json!({"event":"result","request_id":request_id,"ok":result.is_ok(),"error":result.err().map(|e| e.to_string())}),
            );
        }
    }
    stop.0.store(true, Ordering::Release);
    worker.join().unwrap();
    std::process::ExitCode::SUCCESS
}
