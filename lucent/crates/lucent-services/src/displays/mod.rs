//! Native display adapter and independent transactional rollback guardian.
mod guard;
mod native;
mod persistence;
pub use guard::main as run_guard;
use lucent_domain::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{ChildStdin, Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
fn failed(message: &str) -> DomainError {
    DomainError::Failed(message.into())
}
fn unavailable(message: &str) -> DomainError {
    DomainError::Unavailable(message.into())
}
fn boot_millis() -> Result<u64> {
    let mut stamp = std::mem::MaybeUninit::<libc::timespec>::uninit();
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, stamp.as_mut_ptr()) } != 0 {
        return Err(unavailable("Display confirmation clock is unavailable"));
    }
    let stamp = unsafe { stamp.assume_init() };
    Ok((stamp.tv_sec as u64).saturating_mul(1000) + (stamp.tv_nsec as u64) / 1_000_000)
}
struct Guardian {
    input: ChildStdin,
}
#[derive(Default)]
struct State {
    transaction: Option<DisplayTransaction>,
    guardian: Option<Guardian>,
    next_id: u64,
    revision: u64,
}
#[derive(Default)]
struct Core {
    state: Mutex<State>,
    changed: Condvar,
}
/// The executable is selected by the composition root, never from an IPC request.
/// It must dispatch `--display-guard` before creating any Wayland/GPU objects.
#[derive(Clone)]
pub struct HyprlandDisplays {
    core: Arc<Core>,
    executable: std::path::PathBuf,
}
impl Default for HyprlandDisplays {
    fn default() -> Self {
        Self {
            core: Arc::default(),
            executable: std::env::current_exe().unwrap_or_default(),
        }
    }
}
impl HyprlandDisplays {
    fn command(&self, id: DisplayTransactionId, command: guard::Command) -> Result<()> {
        let mut state = self
            .core
            .state
            .lock()
            .map_err(|_| failed("Display transaction state is unavailable"))?;
        if !state.transaction.as_ref().is_some_and(|t| {
            t.id == id
                && (matches!(
                    t.phase,
                    DisplayTransactionPhase::AwaitingConfirmation { .. }
                ) || matches!(command, guard::Command::Revert) && t.phase.pending())
        }) {
            return Err(failed("This display confirmation is no longer active"));
        }
        let guardian = state
            .guardian
            .as_mut()
            .ok_or_else(|| failed("Display restoration guard is unavailable"))?;
        serde_json::to_writer(&mut guardian.input, &command)
            .map_err(|_| failed("Display restoration guard disconnected"))?;
        guardian
            .input
            .write_all(b"\n")
            .and_then(|_| guardian.input.flush())
            .map_err(|_| failed("Display restoration guard disconnected"))
    }
    fn transaction(&self) -> Option<DisplayTransaction> {
        self.core
            .state
            .lock()
            .ok()
            .and_then(|s| s.transaction.clone())
    }
    fn snapshot(&self, native: &native::Native) -> Result<DisplaySnapshot> {
        Ok(DisplaySnapshot {
            outputs: native.outputs()?,
            transaction: self.transaction(),
        })
    }
}
impl DisplayPort for HyprlandDisplays {
    fn watch(&self, emit: &mut dyn FnMut(Result<DisplaySnapshot>), stop: &dyn StopSignal) {
        while !stop.cancelled() {
            let Ok(native) = native::Native::from_env() else {
                emit(Err(unavailable("Hyprland displays are unavailable")));
                stop.wait(Duration::from_secs(2));
                continue;
            };
            let Ok(mut events) = native.event_socket() else {
                emit(Err(unavailable("Display observation is unavailable")));
                stop.wait(Duration::from_secs(2));
                continue;
            };
            let mut previous = None;
            let mut refresh = true;
            let mut revision = u64::MAX;
            let mut bytes = [0; 4096];
            while !stop.cancelled() {
                let next_revision = self.core.state.lock().map(|s| s.revision).unwrap_or(0);
                if refresh || next_revision != revision {
                    let next = self.snapshot(&native);
                    if previous.as_ref() != Some(&next) {
                        emit(next.clone());
                        previous = Some(next);
                    }
                    revision = next_revision;
                    refresh = false;
                }
                match events.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&bytes[..n]);
                        refresh = text.contains("monitor")
                            || text.contains("configreloaded")
                            || text.contains("focusedmon");
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) => {}
                    Err(_) => break,
                }
            }
            if !stop.cancelled() {
                emit(Err(unavailable("Display observation disconnected")));
                stop.wait(Duration::from_secs(1));
            }
        }
    }
    fn apply(
        &self,
        baseline: Vec<DisplayConfiguration>,
        draft: Vec<DisplayConfiguration>,
    ) -> Result<DisplayTransactionId> {
        let native = native::Native::from_env()?;
        lucent_usecases::displays::validate_display_draft(&native.outputs()?, &baseline, &draft)?;
        if baseline == draft {
            return Err(failed("The display draft has no changes"));
        }
        let mut state = self
            .core
            .state
            .lock()
            .map_err(|_| failed("Display transaction state is unavailable"))?;
        if state.guardian.is_some()
            || state
                .transaction
                .as_ref()
                .is_some_and(|t| t.phase.pending())
        {
            return Err(failed("A display preview is already active"));
        }
        state.next_id = state
            .next_id
            .checked_add(1)
            .ok_or_else(|| failed("Display transaction identity exhausted"))?;
        let id = DisplayTransactionId::new(state.next_id)?;
        let mut child = Command::new(&self.executable)
            .arg("--display-guard")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| unavailable("Display restoration guard could not start"))?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| failed("Display guard input is unavailable"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| failed("Display guard output is unavailable"))?;
        let request = guard::Request {
            id,
            baseline,
            draft,
        };
        if serde_json::to_writer(&mut input, &request).is_err()
            || input.write_all(b"\n").and_then(|_| input.flush()).is_err()
        {
            drop(input);
            let _ = child.wait();
            return Err(failed(
                "Display restoration guard could not receive the draft",
            ));
        }
        state.guardian = Some(Guardian { input });
        state.transaction = Some(DisplayTransaction {
            id,
            phase: DisplayTransactionPhase::Preparing,
        });
        state.revision = state.revision.wrapping_add(1);
        drop(state);
        let weak = Arc::downgrade(&self.core);
        std::thread::spawn(move || {
            let mut terminal = false;
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if line.len() > 4096 {
                    break;
                }
                let Ok(transaction) = serde_json::from_str::<DisplayTransaction>(&line) else {
                    break;
                };
                if transaction.id != id {
                    break;
                }
                let Some(core) = weak.upgrade() else { break };
                let Ok(mut state) = core.state.lock() else {
                    break;
                };
                if state.transaction.as_ref().is_none_or(|t| t.id != id) {
                    break;
                }
                terminal = !transaction.phase.pending();
                state.transaction = Some(transaction);
                state.revision = state.revision.wrapping_add(1);
                core.changed.notify_all();
            }
            let _ = child.wait();
            if let Some(core) = weak.upgrade()
                && let Ok(mut state) = core.state.lock()
                && state.transaction.as_ref().is_some_and(|t| t.id == id)
            {
                state.guardian = None;
                if !terminal {
                    state.transaction=Some(DisplayTransaction{id,phase:DisplayTransactionPhase::Failed{reason:"Display guard ended without verified completion. Inspect the current displays before retrying.".into()}});
                }
                state.revision = state.revision.wrapping_add(1);
                core.changed.notify_all();
            }
        });
        Ok(id)
    }
    fn keep(&self, id: DisplayTransactionId) -> Result<()> {
        self.command(id, guard::Command::Keep)
    }
    fn revert(&self, id: DisplayTransactionId) -> Result<()> {
        self.command(id, guard::Command::Revert)
    }
}
#[cfg(test)]
mod tests;
