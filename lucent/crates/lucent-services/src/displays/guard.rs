//! Independent process: it owns mutations and rollback, never Wayland surfaces.
use super::*;
use std::{
    io::{BufRead, Read, Write},
    sync::mpsc,
    time::Duration,
};
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct Request {
    pub id: DisplayTransactionId,
    pub baseline: Vec<DisplayConfiguration>,
    pub draft: Vec<DisplayConfiguration>,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) enum Command {
    Keep,
    Revert,
}
pub(super) trait Driver {
    fn outputs(&self) -> Result<Vec<DisplayDevice>>;
    fn apply(&self, layout: &[DisplayConfiguration]) -> Result<()>;
}
impl Driver for native::Native {
    fn outputs(&self) -> Result<Vec<DisplayDevice>> {
        native::Native::outputs(self)
    }
    fn apply(&self, layout: &[DisplayConfiguration]) -> Result<()> {
        native::Native::apply(self, layout)
    }
}
fn emit(
    out: &mut dyn Write,
    id: DisplayTransactionId,
    phase: DisplayTransactionPhase,
) -> Result<()> {
    serde_json::to_writer(&mut *out, &DisplayTransaction { id, phase })
        .map_err(|_| failed("Display guard reporting failed"))?;
    out.write_all(b"\n")
        .and_then(|_| out.flush())
        .map_err(|_| failed("Display guard disconnected"))
}
fn rollback(driver: &dyn Driver, baseline: &[DisplayConfiguration]) -> Result<()> {
    let outputs = driver.outputs()?;
    let connected: Vec<_> = baseline
        .iter()
        .filter(|c| outputs.iter().any(|o| o.current.output == c.output))
        .cloned()
        .collect();
    if !connected.iter().any(|c| c.enabled) {
        return Err(failed(
            "Original displays are disconnected; automatic restoration could not be verified",
        ));
    }
    driver.apply(&connected)?;
    // Snapshot acknowledgement can lag a successful native command. Retry only
    // observation, never invent successful restoration from an `ok` response.
    for _ in 0..8 {
        let outputs = driver.outputs()?;
        let restored: Vec<_> = outputs
            .into_iter()
            .filter(|o| connected.iter().any(|c| c.output == o.current.output))
            .collect();
        if native::matches(&connected, &restored) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(failed(
        "Original display configuration could not be verified",
    ))
}
fn revert(
    driver: &dyn Driver,
    baseline: &[DisplayConfiguration],
    reason: &str,
) -> DisplayTransactionPhase {
    match rollback(driver, baseline) {
        Ok(()) => DisplayTransactionPhase::Reverted {
            reason: reason.into(),
        },
        Err(_) => DisplayTransactionPhase::Failed {
            reason:
                "Automatic restoration could not be verified. Use the independent recovery command."
                    .into(),
        },
    }
}
struct Safety<'a> {
    driver: &'a dyn Driver,
    baseline: &'a [DisplayConfiguration],
    armed: std::cell::Cell<bool>,
}
impl Safety<'_> {
    fn revert(&self, reason: &str) -> DisplayTransactionPhase {
        let phase = revert(self.driver, self.baseline, reason);
        if matches!(phase, DisplayTransactionPhase::Reverted { .. }) {
            self.armed.set(false);
        }
        phase
    }
}
impl Drop for Safety<'_> {
    fn drop(&mut self) {
        if self.armed.get() {
            let _ = rollback(self.driver, self.baseline);
        }
    }
}
pub(super) fn run_transaction(
    driver: &dyn Driver,
    request: Request,
    commands: mpsc::Receiver<Command>,
    out: &mut dyn Write,
    clock: &dyn Fn() -> Result<u64>,
    persist: &dyn Fn(&[DisplayConfiguration]) -> Result<()>,
) -> Result<()> {
    let outputs = driver.outputs()?;
    lucent_usecases::displays::validate_display_draft(&outputs, &request.baseline, &request.draft)?;
    let deadline = clock()?
        .checked_add(15_000)
        .ok_or_else(|| failed("Display confirmation deadline overflow"))?;
    let safety = Safety {
        driver,
        baseline: &request.baseline,
        armed: std::cell::Cell::new(true),
    };
    if driver.apply(&request.draft).is_err() {
        let phase = safety.revert("Apply failed; original settings restored");
        return emit(out, request.id, phase);
    }
    let mut observed = false;
    for _ in 0..8 {
        if let Ok(outputs) = driver.outputs()
            && native::matches(&request.draft, &outputs)
        {
            observed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !observed {
        let phase = safety.revert("The requested settings were not observed");
        return emit(out, request.id, phase);
    }
    let mut last_seconds = u32::MAX;
    let mut last_observation = clock()?;
    loop {
        let now = match clock() {
            Ok(v) => v,
            Err(_) => {
                let phase = safety.revert("The confirmation clock became unavailable");
                return emit(out, request.id, phase);
            }
        };
        if now >= deadline {
            let phase = safety.revert("Confirmation timed out; original settings restored");
            return emit(out, request.id, phase);
        }
        let seconds = ((deadline - now).div_ceil(1000)) as u32;
        if seconds != last_seconds {
            if emit(
                out,
                request.id,
                DisplayTransactionPhase::AwaitingConfirmation {
                    remaining_seconds: seconds,
                },
            )
            .is_err()
            {
                return Err(failed("Display client disconnected; restoration attempted"));
            }
            last_seconds = seconds;
        }
        if now.saturating_sub(last_observation) >= 250 {
            if !driver
                .outputs()
                .is_ok_and(|o| native::matches(&request.draft, &o))
            {
                let phase = safety.revert("Display state changed during confirmation");
                return emit(out, request.id, phase);
            }
            last_observation = now;
        }
        match commands.recv_timeout(Duration::from_millis(50)) {
            Ok(Command::Keep) => {
                if clock()? >= deadline
                    || !driver
                        .outputs()
                        .is_ok_and(|o| native::matches(&request.draft, &o))
                {
                    let phase = safety.revert("The preview expired or display state changed");
                    return emit(out, request.id, phase);
                }
                match persist(&request.draft) {
                    Ok(()) => {
                        safety.armed.set(false);
                        return emit(out, request.id, DisplayTransactionPhase::Kept);
                    }
                    Err(_) => {
                        let phase = safety
                            .revert("Settings could not be saved; original settings restored");
                        return emit(out, request.id, phase);
                    }
                }
            }
            Ok(Command::Revert) => {
                let phase = safety.revert("Original settings restored");
                return emit(out, request.id, phase);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Ok(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}
/// Called before the desktop runtime, only by a composition-root-selected child.
/// stdin closure restores baseline when the UI exits or crashes. The guardian's
/// boot-time deadline includes suspend and does not depend on compositor frames.
pub fn main() -> Result<()> {
    let input = std::io::stdin();
    let mut input = input.lock();
    let mut first = String::new();
    input
        .by_ref()
        .take(128 * 1024)
        .read_line(&mut first)
        .map_err(|_| failed("Display guard request is unavailable"))?;
    let request: Request =
        serde_json::from_str(&first).map_err(|_| failed("Invalid display guard request"))?;
    drop(input);
    let native = native::Native::from_env()?;
    // Establish file safety and recovery before the first display mutation.
    let config = persistence::Captured::capture()?;
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let input = std::io::stdin();
        let mut input = input.lock();
        loop {
            let mut line = String::new();
            match input.by_ref().take(128).read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let Ok(c) = serde_json::from_str::<Command>(&line) else {
                        break;
                    };
                    if tx.send(c).is_err() {
                        break;
                    }
                }
            }
        }
    });
    run_transaction(
        &native,
        request,
        rx,
        &mut std::io::stdout().lock(),
        &boot_millis,
        &|layout| config.keep(layout),
    )
}
