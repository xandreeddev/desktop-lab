//! Independent media consumer. Selecting a bus is explicit; the default/help
//! path cannot accidentally connect to a desktop user's real media players.
use lucent_domain::*;
use lucent_services::media::Mpris;
use std::{
    io::{self, BufRead},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const HELP: &str = "media-service [--observe|--stdio] [--seconds 1..600] (--session-bus|--bus-address OWNED_FIXTURE_ADDRESS)";
struct Stop(AtomicBool);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn wait(&self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while !self.cancelled() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
    }
}
fn print(value: serde_json::Value) {
    println!("{value}");
}
fn usage() {
    eprintln!("{HELP}");
}
fn parse_command(line: &[u8]) -> Result<Option<MediaCommand>> {
    let value: serde_json::Value = serde_json::from_slice(line)
        .map_err(|_| DomainError::Invalid("Expected a JSON media command".into()))?;
    let command = value["command"]
        .as_str()
        .ok_or_else(|| DomainError::Invalid("Media command is required".into()))?;
    if command == "quit" {
        return Ok(None);
    }
    let id = value["player"]
        .as_u64()
        .ok_or_else(|| DomainError::Invalid("Current player identity required".into()))?;
    let id = MediaPlayerId::new(id)?;
    Ok(Some(match command {
        "play" => MediaCommand::Play(id),
        "pause" => MediaCommand::Pause(id),
        "next" => MediaCommand::Next(id),
        "previous" => MediaCommand::Previous(id),
        "stop" => MediaCommand::Stop(id),
        "raise" => MediaCommand::Raise(id),
        "seek" => {
            MediaCommand::SetPosition {
                player: id,
                track: MediaTrackId::new(value["track"].as_u64().ok_or_else(|| {
                    DomainError::Invalid("Current track identity required".into())
                })?)?,
                position: MediaTime::new(value["position_us"].as_u64().ok_or_else(|| {
                    DomainError::Invalid("Nonnegative position_us required".into())
                })?)?,
            }
        }
        _ => return Err(DomainError::Invalid("Unknown media command".into())),
    }))
}
fn main() {
    let mut args = std::env::args().skip(1);
    let mut seconds = 5u64;
    let mut address = None;
    let mut session_bus = false;
    let mut stdio = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => {
                usage();
                return;
            }
            "--observe" => (),
            "--stdio" => stdio = true,
            "--session-bus" => session_bus = true,
            "--bus-address" => {
                let Some(value) = args.next().filter(|value| !value.is_empty()) else {
                    usage();
                    return;
                };
                address = Some(value);
            }
            "--seconds" => {
                let Some(value) = args
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .filter(|value| (1..=600).contains(value))
                else {
                    usage();
                    return;
                };
                seconds = value;
            }
            _ => {
                usage();
                return;
            }
        }
    }
    if session_bus == address.is_some() {
        usage();
        return;
    }
    let adapter = Arc::new(address.map(Mpris::with_bus_address).unwrap_or_default());
    let stop = Arc::new(Stop(AtomicBool::new(false)));
    let current = Arc::new(Mutex::new(None::<MediaSnapshot>));
    let (a, s, c) = (adapter.clone(), stop.clone(), current.clone());
    let worker = thread::spawn(move || {
        a.watch(&mut |result| match result {
        Ok(snapshot) => {
            let players = stdio.then(|| snapshot.players.iter().map(|player| {
                match &player.state {
                    MediaPlayerState::Ready(playback) => serde_json::json!({
                        "id":player.id.value(),"name":player.name,"state":"ready",
                        "status":format!("{:?}",playback.status),
                        "track":playback.track.as_ref().map(|track|serde_json::json!({
                            "id":track.id.value(),"title":track.title,
                            "length_us":track.length.map(MediaTime::micros),
                        })),
                        "capabilities":{"play":playback.capabilities.can_play,
                            "pause":playback.capabilities.can_pause,"next":playback.capabilities.can_next,
                            "previous":playback.capabilities.can_previous,"seek":playback.capabilities.can_seek,
                            "control":playback.capabilities.can_control,"raise":playback.capabilities.can_raise},
                        "position_us":playback.position.map(|sample| sample.position.micros()),
                    }),
                    MediaPlayerState::Unavailable(_) => serde_json::json!({
                        "id":player.id.value(),"name":player.name,"state":"unavailable",
                    }),
                }
            }).collect::<Vec<_>>());
            print(serde_json::json!({"state":"ready","count":snapshot.players.len(),"players":players}));
            *c.lock().unwrap() = Some(snapshot);
        }
        Err(_) => { *c.lock().unwrap() = None; print(serde_json::json!({"state":"unavailable"})); }
    }, s.as_ref())
    });
    let deadline = Instant::now() + Duration::from_secs(seconds);
    if stdio {
        let (send, lines) = mpsc::sync_channel(1);
        // A blocked terminal read never holds media observation or shutdown.
        // Input storage and handoff each have a fixed limit.
        let _input = thread::spawn(move || {
            let mut input = io::stdin().lock();
            loop {
                let mut bytes = Vec::new();
                let mut overflow = false;
                loop {
                    let Ok(available) = input.fill_buf() else {
                        return;
                    };
                    if available.is_empty() {
                        break;
                    }
                    let count = available
                        .iter()
                        .position(|byte| *byte == b'\n')
                        .map_or(available.len(), |index| index + 1);
                    let complete = available[count - 1] == b'\n';
                    if bytes.len() + count <= 4096 {
                        bytes.extend_from_slice(&available[..count]);
                    } else {
                        overflow = true;
                    }
                    input.consume(count);
                    if complete {
                        break;
                    }
                }
                if bytes.is_empty() && !overflow {
                    break;
                }
                if send
                    .send(if overflow { Err(()) } else { Ok(bytes) })
                    .is_err()
                {
                    break;
                }
            }
        });
        while Instant::now() < deadline {
            let Ok(line) = lines.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            else {
                break;
            };
            let result = match line {
                Ok(bytes) => parse_command(&bytes),
                Err(()) => Err(DomainError::Invalid("Command exceeds 4096 bytes".into())),
            };
            match result {
                Ok(None) => break,
                Ok(Some(command)) => {
                    let known = current.lock().unwrap().as_ref().is_some_and(|snapshot| {
                        snapshot
                            .players
                            .iter()
                            .any(|player| player.id == command.player())
                    });
                    let acknowledged = known && adapter.control(command).is_ok();
                    print(serde_json::json!({"acknowledged":acknowledged}));
                }
                Err(_) => print(serde_json::json!({"error":"Invalid media command"})),
            }
        }
    } else {
        stop.wait(deadline.saturating_duration_since(Instant::now()));
    }
    stop.0.store(true, Ordering::Release);
    let _ = worker.join();
}
