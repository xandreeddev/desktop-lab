//! Independent consumer of BluetoothPort. Observation prints counts only.
//! --stdio opts into local metadata and accepts explicit controls on stdin.
use lucent_domain::*;
use lucent_services::bluetooth::BlueZ;
use serde::Deserialize;
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
use zeroize::Zeroizing;

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
fn id(value: Option<u64>) -> Result<BluetoothObjectId> {
    value
        .ok_or_else(|| DomainError::Invalid("Expected a Bluetooth handle".into()))
        .and_then(BluetoothObjectId::new)
}
fn boolean(value: Option<bool>) -> Result<bool> {
    value.ok_or_else(|| DomainError::Invalid("Expected a boolean flag".into()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    command: String,
    adapter: Option<u64>,
    device: Option<u64>,
    powered: Option<bool>,
    discoverable: Option<bool>,
    pairable: Option<bool>,
    active: Option<bool>,
    trusted: Option<bool>,
    enabled: Option<bool>,
    request: Option<u64>,
    attempt: Option<u64>,
    connection: Option<u64>,
    answer: Option<String>,
    #[serde(default, deserialize_with = "secret_input")]
    input: Option<Secret>,
}
fn secret_input<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Secret>, D::Error> {
    Option::<String>::deserialize(d).map(|input| input.map(Secret::new))
}
fn command(value: &Input) -> Result<BluetoothCommand> {
    Ok(match value.command.as_str() {
        "power" => BluetoothCommand::SetPowered {
            adapter: id(value.adapter)?,
            powered: boolean(value.powered)?,
        },
        "discovery" => BluetoothCommand::SetDiscovery {
            adapter: id(value.adapter)?,
            active: boolean(value.active)?,
        },
        "visibility" => BluetoothCommand::SetDiscoverable {
            adapter: id(value.adapter)?,
            discoverable: boolean(value.discoverable)?,
        },
        "pairable" => BluetoothCommand::SetPairable {
            adapter: id(value.adapter)?,
            pairable: boolean(value.pairable)?,
        },
        "disconnect" => BluetoothCommand::Disconnect(id(value.device)?),
        "trust" => BluetoothCommand::SetTrusted {
            device: id(value.device)?,
            trusted: boolean(value.trusted)?,
        },
        "forget" => BluetoothCommand::Forget(id(value.device)?),
        _ => return Err(DomainError::Invalid("Unknown Bluetooth command".into())),
    })
}
fn pairing_id(value: Option<u64>) -> Result<BluetoothPairingId> {
    value
        .ok_or_else(|| DomainError::Invalid("Expected a current pairing handle".into()))
        .and_then(BluetoothPairingId::new)
}
fn answer(value: &mut Input) -> Result<BluetoothPairingAnswer> {
    Ok(match value.answer.as_deref() {
        Some("accept") => BluetoothPairingAnswer::Accept,
        Some("reject") => BluetoothPairingAnswer::Reject,
        Some("pin") => BluetoothPairingAnswer::PinCode(BluetoothPin::new(
            value
                .input
                .take()
                .ok_or_else(|| DomainError::Invalid("PIN input is required".into()))?,
        )?),
        Some("passkey") => BluetoothPairingAnswer::Passkey(BluetoothPasskeyInput::new(
            value
                .input
                .take()
                .ok_or_else(|| DomainError::Invalid("Passkey input is required".into()))?,
        )?),
        _ => return Err(DomainError::Invalid("Unknown pairing answer".into())),
    })
}
fn report(result: Result<()>) {
    match result {
        Ok(()) => send(json!({"kind":"acknowledged"})),
        Err(error) => send(json!({"kind":"error","error":error.to_string()})),
    }
}
fn prompt_json(request: &BluetoothPairingRequest) -> Value {
    let mut value = json!({"id":request.id.value(),"device":request.device.value()});
    let (kind, code) = match &request.prompt {
        BluetoothPairingPrompt::PinCode => ("pin", None),
        BluetoothPairingPrompt::Passkey => ("passkey", None),
        BluetoothPairingPrompt::ConfirmPasskey(c) => ("confirmation", Some(*c)),
        BluetoothPairingPrompt::DisplayPinCode(c) => ("display-pin", Some(*c)),
        BluetoothPairingPrompt::DisplayPasskey { code, entered } => {
            value["entered"] = json!(entered);
            ("display-passkey", Some(*code))
        }
        BluetoothPairingPrompt::AuthorizePairing => ("authorization", None),
        BluetoothPairingPrompt::AuthorizeService(service) => {
            value["service"] = json!(service.as_str());
            ("service", None)
        }
    };
    value["kind"] = json!(kind);
    if let Some(code) = code {
        value["code"] = json!(code.digits());
    }
    value
}
fn state(value: &BluetoothSnapshot, detailed: bool) -> Value {
    if !detailed {
        return json!({"adapters":value.adapters.len(),"devices":value.devices.len(),
            "connected":value.devices.iter().filter(|d|d.connected).count(),"pairing_requests":value.pairing_requests.len()});
    }
    json!({
        "incoming_pairing_enabled":value.incoming_pairing_enabled,
        "pairing":value.pairing.as_ref().map(|a|json!({"id":a.id.value(),"device":a.device.value(),"phase":format!("{:?}",a.phase)})),
        "pairing_requests":value.pairing_requests.iter().map(prompt_json).collect::<Vec<_>>(),
        "adapters":value.adapters.iter().map(|a|json!({
            "id":a.id.value(),"address":a.address.to_string(),"name":a.name,"powered":a.powered,
            "discoverable":a.discoverable,"pairable":a.pairable,"discovering":a.discovering,"discovery_owned":a.discovery_owned,
        })).collect::<Vec<_>>(),
        "connection":value.connection.as_ref().map(|a|json!({"id":a.id.value(),"device":a.device.value(),"phase":match &a.phase {
            BluetoothConnectionPhase::Connecting => "connecting", BluetoothConnectionPhase::Succeeded => "succeeded", BluetoothConnectionPhase::Cancelled => "cancelled", BluetoothConnectionPhase::Failed(_) => "failed",
        }})),
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
    let mut worker: Option<thread::JoinHandle<()>> = None;
    let input_result: io::Result<()> = if detailed {
        (|| {
            let mut stdin = io::stdin().lock();
            loop {
                // Bound each admitted command instead of allocating an unbounded line.
                let mut line = Zeroizing::new(vec![]);
                let size = stdin.by_ref().take(4097).read_until(b'\n', &mut line)?;
                if size == 0 {
                    break;
                }
                if size > 4096 {
                    send(json!({"kind":"error","error":"Bluetooth command exceeds the limit"}));
                    break;
                }
                let mut input = match serde_json::from_slice::<Input>(&line) {
                    Ok(value) => value,
                    Err(_) => {
                        report(Err(DomainError::Invalid(
                            "Invalid Bluetooth command JSON".into(),
                        )));
                        continue;
                    }
                };
                // One ordinary worker leaves stdin free to answer/cancel a
                // callback or cancel an owned connection independently of ordinary writes.
                if worker.as_ref().is_some_and(|w| w.is_finished())
                    && worker.take().unwrap().join().is_err()
                {
                    report(Err(DomainError::Failed(
                        "Bluetooth command worker failed".into(),
                    )));
                }
                match input.command.as_str() {
                    "connect" => match id(input.device).and_then(|id| adapter.start_connection(id))
                    {
                        Ok(id) => {
                            send(json!({"kind":"connection-started","connection":id.value()}))
                        }
                        Err(error) => report(Err(error)),
                    },
                    "cancel-connect" => report(
                        input
                            .connection
                            .ok_or_else(|| {
                                DomainError::Invalid("Expected a connection attempt".into())
                            })
                            .and_then(BluetoothConnectionId::new)
                            .and_then(|id| adapter.cancel_connection(id)),
                    ),
                    "pair" => match id(input.device).and_then(|id| adapter.start_pairing(id)) {
                        Ok(id) => send(json!({"kind":"pairing-started","attempt":id.value()})),
                        Err(error) => report(Err(error)),
                    },
                    "answer" => report(pairing_id(input.request).and_then(|id| {
                        answer(&mut input).and_then(|answer| adapter.answer_pairing(id, answer))
                    })),
                    "cancel" => {
                        report(pairing_id(input.attempt).and_then(|id| adapter.cancel_pairing(id)))
                    }
                    _ if worker.is_some() => report(Err(DomainError::Failed(
                        "Bluetooth command is already pending".into(),
                    ))),
                    _ => {
                        let adapter = adapter.clone();
                        worker = Some(thread::spawn(move || {
                            report(if input.command == "incoming" {
                                boolean(input.enabled)
                                    .and_then(|enabled| adapter.set_incoming_pairing(enabled))
                            } else {
                                command(&input).and_then(|command| adapter.control(command))
                            });
                        }));
                    }
                };
            }
            Ok(())
        })()
    } else {
        stop.wait(Duration::from_secs(seconds));
        Ok(())
    };
    stop.0.store(true, Ordering::Release);
    observer.join().map_err(|_| "Bluetooth observer failed")?;
    if let Some(worker) = worker {
        worker
            .join()
            .map_err(|_| "Bluetooth command worker failed")?;
    }
    input_result?;
    Ok(())
}
