//! Normalize the compositor's session-lock observation. Do not infer security
//! from process existence, window focus or a successful lock command.
use super::*;

impl Hyprland {
    pub(crate) fn session_lock(&self) -> Result<SessionLockState> {
        decode(&self.json_before("j/monitors", Instant::now() + wire::REQUEST_TIMEOUT)?)
    }
}

fn decode(monitors: &Value) -> Result<SessionLockState> {
    let unavailable = || DomainError::Unavailable("Session lock observation is unavailable".into());
    let monitors = monitors.as_array().ok_or_else(unavailable)?;
    let mut readable = false;
    for monitor in monitors {
        match monitor.get("solitaryBlockedBy") {
            Some(Value::Null) => readable = true,
            Some(Value::Array(reasons)) => {
                // LOCK is global and remains observable if the locker dies.
                if reasons.iter().any(|r| r.as_str() == Some("LOCK")) {
                    return Ok(SessionLockState::Locked);
                }
                // Without a workspace Hyprland returns before checking lock.
                // Missing/invalid fields are never evidence of an unlock.
                readable |= reasons.iter().all(Value::is_string)
                    && !reasons
                        .iter()
                        .any(|r| matches!(r.as_str(), Some("WORKSPACE" | "UNKNOWN")));
            }
            _ => {}
        }
    }
    if readable {
        Ok(SessionLockState::Unlocked)
    } else {
        Err(unavailable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lock_observation_requires_explicit_compositor_evidence() {
        for monitors in [
            json!([{"solitaryBlockedBy":["LOCK"]}]),
            json!([{"solitaryBlockedBy":null},{"solitaryBlockedBy":["WORKSPACE","LOCK"]}]),
        ] {
            assert_eq!(decode(&monitors), Ok(SessionLockState::Locked));
        }
        for reasons in [Value::Null, json!([]), json!(["WINDOWED", "CANDIDATE"])] {
            assert_eq!(
                decode(&json!([{"solitaryBlockedBy":["WORKSPACE"]},{"solitaryBlockedBy":reasons}])),
                Ok(SessionLockState::Unlocked)
            );
        }
        for monitors in [
            Value::Null,
            json!({}),
            json!([]),
            json!([{}]),
            json!([{"solitaryBlockedBy":false}]),
            json!([{"solitaryBlockedBy":["WORKSPACE"]}]),
            json!([{"solitaryBlockedBy":["UNKNOWN"]}]),
            json!([{"solitaryBlockedBy":["WINDOWED",false]}]),
        ] {
            assert!(decode(&monitors).is_err());
        }
    }
}
