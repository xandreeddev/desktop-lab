//! Current-capability policy. Authorization remains with the native services.
use lucent_domain::*;

pub fn validate_power_command(state: &PowerSnapshot, command: &PowerCommand) -> Result<()> {
    if state
        .operation
        .as_ref()
        .is_some_and(|o| o.phase == PowerOperationPhase::InProgress)
    {
        return Err(DomainError::Failed(
            "A power operation is already in progress".into(),
        ));
    }
    let current = match command {
        PowerCommand::Session(action) => return validate_power_action(state, *action),
        PowerCommand::SetProfile(id) => match &state.profiles {
            PowerCapability::Ready(profiles) => profiles.profiles.iter().any(|p| p.id == *id),
            PowerCapability::Unavailable(_) => {
                return Err(DomainError::Unavailable(
                    "Power profiles are unavailable".into(),
                ));
            }
        },
        PowerCommand::SetBrightness(id, _) => match &state.brightness {
            PowerCapability::Ready(devices) => devices.iter().any(|d| d.id == *id),
            PowerCapability::Unavailable(_) => {
                return Err(DomainError::Unavailable(
                    "Brightness controls are unavailable".into(),
                ));
            }
        },
    };
    if current {
        Ok(())
    } else {
        Err(DomainError::Invalid(
            "Choose a current power control".into(),
        ))
    }
}
pub fn validate_power_action(state: &PowerSnapshot, action: PowerAction) -> Result<()> {
    if state
        .operation
        .as_ref()
        .is_some_and(|o| o.phase == PowerOperationPhase::InProgress)
    {
        return Err(DomainError::Failed(
            "A power action is already in progress".into(),
        ));
    }
    match &state.session {
        PowerCapability::Ready(capabilities)
            if capabilities.iter().any(|c| {
                c.action == action
                    && matches!(
                        c.permission,
                        PowerPermission::Allowed | PowerPermission::Authentication
                    )
            }) =>
        {
            Ok(())
        }
        PowerCapability::Ready(_) => Err(DomainError::Invalid(
            "This power action is not currently available".into(),
        )),
        PowerCapability::Unavailable(_) => Err(DomainError::Unavailable(
            "Session controls are unavailable".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn power_policy_requires_current_targets_and_native_permission() {
        let id = PowerObjectId::new(1).unwrap();
        let mut state = PowerSnapshot {
            profiles: PowerCapability::Ready(PowerProfiles {
                profiles: vec![PowerProfile {
                    id,
                    name: "balanced".into(),
                }],
                ..Default::default()
            }),
            session: PowerCapability::Ready(vec![PowerActionCapability {
                action: PowerAction::Suspend,
                permission: PowerPermission::Authentication,
            }]),
            ..Default::default()
        };
        assert!(validate_power_command(&state, &PowerCommand::SetProfile(id)).is_ok());
        assert!(
            validate_power_command(
                &state,
                &PowerCommand::SetProfile(PowerObjectId::new(2).unwrap())
            )
            .is_err()
        );
        assert!(
            validate_power_command(
                &state,
                &PowerCommand::SetBrightness(id, Percentage::new(50.).unwrap())
            )
            .is_err()
        );
        assert!(validate_power_action(&state, PowerAction::Suspend).is_ok());
        assert!(validate_power_action(&state, PowerAction::PowerOff).is_err());
        state.operation = Some(PowerOperation {
            id: PowerOperationId::new(2).unwrap(),
            command: PowerCommand::Session(PowerAction::Suspend),
            phase: PowerOperationPhase::InProgress,
        });
        assert!(validate_power_action(&state, PowerAction::Suspend).is_err());
    }
}
