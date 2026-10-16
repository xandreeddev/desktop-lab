//! Session adapter. Observations are advisory; authentication and secure unlock
//! remain in the independent locker and compositor protocol.
use crate::compositor::Hyprland;
use lucent_domain::*;
use std::time::Duration;

pub struct LucentSession;
impl SessionPort for LucentSession {
    fn lock(&self) -> Result<()> {
        let home = std::env::var("HOME")
            .map_err(|_| DomainError::Unavailable("Missing home directory".into()))?;
        crate::command("python3", &[&format!("{home}/.local/lib/lucent/lock.py")]).map(|_| ())
    }
    fn watch_lock(&self, emit: &mut dyn FnMut(Result<SessionLockState>), stop: &dyn StopSignal) {
        watch_lock(|| Hyprland::from_env()?.session_lock(), emit, stop);
    }
}

pub(crate) fn watch_lock(
    mut read: impl FnMut() -> Result<SessionLockState>,
    emit: &mut dyn FnMut(Result<SessionLockState>),
    stop: &dyn StopSignal,
) {
    let mut previous = None;
    while !stop.cancelled() {
        // The supported compositor has no lock-change IPC event. Read only the
        // bounded monitor query, on this worker, while a consumer subscribes.
        let observation = read();
        if stop.cancelled() {
            break;
        }
        if previous.as_ref() != Some(&observation) {
            previous = Some(observation.clone());
            emit(observation);
        }
        stop.wait(Duration::from_secs(1));
    }
}
