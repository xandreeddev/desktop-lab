//! Independent native consumer. Observation is read-only; --stdio opts into control.
use lucent_domain::*;
use lucent_services::power::PowerServices;
use serde::Deserialize;
use serde_json::json;
use std::{
    io::{self, BufRead, Write},
    sync::{
        Arc, Mutex,
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
        let end = Instant::now() + duration;
        while !self.cancelled() && Instant::now() < end {
            thread::sleep(Duration::from_millis(20));
        }
    }
}
#[derive(Deserialize)]
struct Input {
    command: String,
    target: Option<u64>,
    value: Option<f64>,
    action: Option<String>,
}
fn decode(input: Input) -> Result<PowerCommand> {
    let id = || {
        input
            .target
            .ok_or_else(|| DomainError::Invalid("Expected a current power handle".into()))
            .and_then(PowerObjectId::new)
    };
    Ok(match input.command.as_str() {
        "profile" => PowerCommand::SetProfile(id()?),
        "brightness" => PowerCommand::SetBrightness(
            id()?,
            Percentage::new(
                input
                    .value
                    .ok_or_else(|| DomainError::Invalid("Expected a percentage".into()))?,
            )?,
        ),
        "session" => PowerCommand::Session(match input.action.as_deref() {
            Some("suspend") => PowerAction::Suspend,
            Some("hibernate") => PowerAction::Hibernate,
            Some("hybrid-sleep") => PowerAction::HybridSleep,
            Some("reboot") => PowerAction::Reboot,
            Some("power-off") => PowerAction::PowerOff,
            _ => return Err(DomainError::Invalid("Unknown session action".into())),
        }),
        _ => return Err(DomainError::Invalid("Unknown power command".into())),
    })
}
fn phase(phase: &PowerOperationPhase) -> &'static str {
    match phase {
        PowerOperationPhase::InProgress => "in-progress",
        PowerOperationPhase::Succeeded => "succeeded",
        PowerOperationPhase::Failed(_) => "failed",
    }
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut detailed = false;
    let mut seconds = 10u64;
    let mut address = None;
    let mut root = None;
    while let Some(arg) = args.next() {
        match arg.as_str(){"--observe"=>(),"--stdio"=>detailed=true,"--seconds"=>seconds=args.next().ok_or("Missing duration")?.parse()?,"--bus-address"=>address=Some(args.next().ok_or("Missing fixture bus")?),"--backlight-root"=>root=Some(args.next().ok_or("Missing fixture directory")?),_=>return Err("Use --observe [--seconds 1..600] or --stdio; fixture requires --bus-address and --backlight-root".into())}
    }
    if !(1..=600).contains(&seconds) || address.is_some() != root.is_some() {
        return Err("Invalid duration or incomplete fixture".into());
    }
    let adapter: Arc<dyn PowerPort> = Arc::new(match (address, root) {
        (Some(address), Some(root)) => PowerServices::with_fixture(address, root.into()),
        _ => PowerServices::default(),
    });
    let stop = Arc::new(Stop::default());
    let lock = Arc::new(Mutex::new(()));
    let emit = {
        let lock = lock.clone();
        move |value: serde_json::Value| {
            let _lock = lock.lock().unwrap();
            let mut out = io::stdout().lock();
            let _ = writeln!(out, "{value}");
            let _ = out.flush();
        }
    };
    let watcher = {
        let adapter = adapter.clone();
        let stop = stop.clone();
        let emit = emit.clone();
        thread::spawn(move || {
            adapter.watch(&mut|result|{
        emit(match result {
            Err(error)=>json!({"kind":"unavailable","error":error.to_string()}),
            Ok(state)=>{
                let profiles=match &state.profiles {PowerCapability::Ready(p)=>p.profiles.len(),_=>0};
                let backlights=match &state.brightness {PowerCapability::Ready(d)=>d.len(),_=>0};
                if detailed {
                    json!({"kind":"state","battery":match &state.battery{PowerCapability::Ready(Some(b))=>json!({"charge":b.charge.value(),"level":format!("{:?}",b.level),"state":format!("{:?}",b.state)}),PowerCapability::Ready(None)=>json!(null),PowerCapability::Unavailable(error)=>json!({"unavailable":error})},"profiles":match &state.profiles{PowerCapability::Ready(p)=>json!({"active":p.active.map(|id|id.value()),"items":p.profiles.iter().map(|p|json!({"id":p.id.value(),"name":p.name})).collect::<Vec<_>>()}),PowerCapability::Unavailable(error)=>json!({"unavailable":error})},"brightness":match &state.brightness{PowerCapability::Ready(d)=>json!(d.iter().map(|d|json!({"id":d.id.value(),"name":d.name,"value":d.brightness.value()})).collect::<Vec<_>>()),PowerCapability::Unavailable(error)=>json!({"unavailable":error})},"operation":state.operation.as_ref().map(|o|json!({"id":o.id.value(),"phase":phase(&o.phase)}))})
                }else{json!({"kind":"state","battery_present":matches!(state.battery,PowerCapability::Ready(Some(_))),"profiles":profiles,"backlights":backlights,"operation":state.operation.as_ref().map(|o|phase(&o.phase))})}
            }
        });
    },stop.as_ref())
        })
    };
    if detailed {
        let mut input = io::stdin().lock();
        loop {
            let mut line = Vec::new();
            let mut oversized = false;
            let mut eof = false;
            loop {
                let buffer = input.fill_buf()?;
                if buffer.is_empty() {
                    eof = true;
                    break;
                }
                let end = buffer
                    .iter()
                    .position(|b| *b == b'\n')
                    .map(|p| p + 1)
                    .unwrap_or(buffer.len());
                if line.len() + end > 4096 {
                    oversized = true;
                } else if !oversized {
                    line.extend_from_slice(&buffer[..end]);
                }
                let complete = buffer[end - 1] == b'\n';
                input.consume(end);
                if complete {
                    break;
                }
            }
            if eof && line.is_empty() && !oversized {
                break;
            }
            let result = if oversized {
                Err(DomainError::Invalid(
                    "Power input exceeds 4096 bytes".into(),
                ))
            } else {
                serde_json::from_slice::<Input>(&line)
                    .map_err(|_| DomainError::Invalid("Invalid power JSON".into()))
                    .and_then(decode)
                    .and_then(|command| adapter.control(command))
            };
            emit(match result {
                Ok(id) => json!({"kind":"admitted","operation":id.value()}),
                Err(error) => json!({"kind":"failed","error":error.to_string()}),
            });
            if eof {
                break;
            }
        }
    } else {
        stop.wait(Duration::from_secs(seconds));
    }
    stop.0.store(true, Ordering::Release);
    watcher.join().map_err(|_| "Power observer failed")?;
    Ok(())
}
