//! Independent read-only AudioPort consumer. No stdin, actions or rendering.
use lucent_domain::{AudioDirection, AudioPort, AudioSnapshot, Result, StopSignal};
use lucent_services::audio::PulseAudio;
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const USAGE: &str = "Use audio-observer [--observe] [--seconds 1..600]";

fn arguments(
    args: impl IntoIterator<Item = String>,
) -> std::result::Result<Duration, &'static str> {
    let mut args = args.into_iter();
    let mut observe = false;
    let mut seconds = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--observe" if !observe => observe = true,
            "--seconds" if seconds.is_none() => {
                seconds = Some(
                    args.next()
                        .ok_or(USAGE)?
                        .parse::<u64>()
                        .map_err(|_| USAGE)?,
                );
            }
            _ => return Err(USAGE),
        }
    }
    match seconds.unwrap_or(10) {
        seconds @ 1..=600 => Ok(Duration::from_secs(seconds)),
        _ => Err(USAGE),
    }
}

struct Stop(Instant);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        Instant::now() >= self.0
    }
    fn wait(&self, duration: Duration) {
        thread::sleep(duration.min(self.0.saturating_duration_since(Instant::now())));
    }
}

fn summary(result: Result<AudioSnapshot>) -> Value {
    match result {
        // Native error bodies, names, keys and application metadata stay private.
        Err(_) => json!({"kind": "unavailable"}),
        Ok(state) => {
            let count = |direction| {
                state
                    .devices
                    .iter()
                    .filter(|d| d.direction == direction)
                    .count()
            };
            let default = |direction| {
                state
                    .default_device(direction)
                    .map(|device| json!({"volume": device.volume.ratio(), "muted": device.muted}))
            };
            json!({
                "kind": "state", "availability": "ready",
                "outputs": count(AudioDirection::Playback),
                "inputs": count(AudioDirection::Capture),
                "monitor_inputs": state.devices.iter().filter(|d| d.direction == AudioDirection::Capture && d.monitor).count(),
                "playback_streams": state.streams.iter().filter(|s| s.direction == AudioDirection::Playback).count(),
                "capture_streams": state.streams.iter().filter(|s| s.direction == AudioDirection::Capture).count(),
                "default_output": default(AudioDirection::Playback),
                "default_input": default(AudioDirection::Capture),
            })
        }
    }
}

struct Observation {
    states: usize,
    unavailable: usize,
    cancelled: bool,
}

struct WatchStop<'a> {
    parent: &'a dyn StopSignal,
    output_failed: AtomicBool,
}
impl StopSignal for WatchStop<'_> {
    fn cancelled(&self) -> bool {
        self.output_failed.load(Ordering::Acquire) || self.parent.cancelled()
    }
    fn wait(&self, duration: Duration) {
        if !self.cancelled() {
            self.parent.wait(duration);
        }
    }
}

fn observe(
    adapter: Arc<dyn AudioPort>,
    stop: &dyn StopSignal,
    mut emit: impl FnMut(Value) -> io::Result<()>,
) -> io::Result<Observation> {
    let watch_stop = WatchStop {
        parent: stop,
        output_failed: AtomicBool::new(false),
    };
    let mut output_error = None;
    let mut result = Observation {
        states: 0,
        unavailable: 0,
        cancelled: false,
    };
    adapter.watch(
        &mut |snapshot| {
            if output_error.is_some() {
                return;
            }
            if snapshot.is_ok() {
                result.states += 1;
            } else {
                result.unavailable += 1;
            }
            if let Err(error) = emit(summary(snapshot)) {
                output_error = Some(error);
                watch_stop.output_failed.store(true, Ordering::Release);
            }
        },
        &watch_stop,
    );
    // Let the adapter retire its native subscription/connection normally before
    // returning a failed stdout write. No completion record can imply success.
    if let Some(error) = output_error {
        return Err(error);
    }
    result.cancelled = stop.cancelled();
    emit(
        json!({"kind": "finished", "states": result.states, "unavailable": result.unavailable,
                "watch_returned_after_cancellation": result.cancelled}),
    )?;
    Ok(result)
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let duration = arguments(std::env::args().skip(1))?;
    let adapter: Arc<dyn AudioPort> = Arc::new(PulseAudio::default());
    let stdout = io::stdout();
    let mut output = stdout.lock();
    // The existing adapter owns connection/subscription teardown. Its event
    // loop checks this deadline; the consumer does not poll native state.
    let result = observe(adapter, &Stop(Instant::now() + duration), |value| {
        writeln!(output, "{value}")
    })?;
    if !result.cancelled {
        return Err("Audio observation ended before cancellation".into());
    }
    if result.states == 0 {
        return Err("No ready audio snapshot was observed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
