use super::*;
use std::sync::Mutex;
pub fn fixture() -> PowerSnapshot {
    PowerSnapshot {
        battery: PowerCapability::Ready(Some(Battery {
            charge: Percentage::new(72.).unwrap(),
            state: BatteryState::Discharging,
            level: BatteryLevel::None,
            on_battery: true,
            seconds_to_empty: Some(5400),
            seconds_to_full: None,
        })),
        profiles: PowerCapability::Ready(PowerProfiles {
            profiles: vec![
                PowerProfile {
                    id: PowerObjectId::new(1).unwrap(),
                    name: "balanced".into(),
                },
                PowerProfile {
                    id: PowerObjectId::new(2).unwrap(),
                    name: "power-saver".into(),
                },
            ],
            active: Some(PowerObjectId::new(1).unwrap()),
            performance_degraded: None,
        }),
        brightness: PowerCapability::Ready(vec![BrightnessDevice {
            id: PowerObjectId::new(3).unwrap(),
            name: "Example display".into(),
            brightness: Percentage::new(50.).unwrap(),
        }]),
        session: PowerCapability::Ready(vec![
            PowerActionCapability {
                action: PowerAction::Suspend,
                permission: PowerPermission::Allowed,
            },
            PowerActionCapability {
                action: PowerAction::PowerOff,
                permission: PowerPermission::Authentication,
            },
        ]),
        ..Default::default()
    }
}
#[derive(Default)]
struct Adapter(Mutex<Vec<PowerCommand>>);
impl PowerPort for Adapter {
    fn watch(&self, emit: &mut dyn FnMut(Result<PowerSnapshot>), _: &dyn StopSignal) {
        emit(Ok(fixture()));
    }
    fn control(&self, command: PowerCommand) -> Result<PowerOperationId> {
        let mut calls = self.0.lock().unwrap();
        calls.push(command);
        PowerOperationId::new(9 + calls.len() as u64)
    }
}
#[test]
fn session_actions_require_confirmation_and_never_fabricate_completion() {
    let adapter = Arc::new(Adapter::default());
    let mut power = Power::new(adapter.clone());
    let mut effects = Effects::default();
    power.update(Message::Observed(Ok(fixture())), &mut effects);
    power.update(Message::Open, &mut effects);
    power.update(
        Message::Control(PowerCommand::Session(PowerAction::PowerOff)),
        &mut effects,
    );
    assert!(effects.tasks.is_empty());
    power.update(Message::Ask(PowerAction::Suspend), &mut effects);
    assert!(effects.tasks.is_empty());
    power.update(Message::Confirm(PowerAction::PowerOff), &mut effects);
    assert!(effects.tasks.is_empty());
    power.update(Message::Confirm(PowerAction::Suspend), &mut effects);
    let task = effects.tasks.remove(0);
    assert_eq!(task.queue, crate::work::CONTROLS);
    power.update(task.run(), &mut effects);
    assert!(power.busy());
    assert!(power.snapshot().unwrap().operation.is_none());
    let mut state = fixture();
    state.operation = Some(PowerOperation {
        id: PowerOperationId::new(10).unwrap(),
        command: PowerCommand::Session(PowerAction::Suspend),
        phase: PowerOperationPhase::Succeeded,
    });
    power.update(Message::Observed(Ok(state)), &mut effects);
    assert!(!power.busy());
    assert_eq!(
        *adapter.0.lock().unwrap(),
        vec![PowerCommand::Session(PowerAction::Suspend)]
    );
}
#[test]
fn unavailable_then_late_admission_does_not_replay_or_stick_busy() {
    let adapter = Arc::new(Adapter::default());
    let mut power = Power::new(adapter.clone());
    let mut effects = Effects::default();
    power.update(Message::Observed(Ok(fixture())), &mut effects);
    power.update(
        Message::Control(PowerCommand::SetProfile(PowerObjectId::new(2).unwrap())),
        &mut effects,
    );
    let task = effects.tasks.remove(0);
    power.update(
        Message::Observed(Err(DomainError::Unavailable("Owner changed".into()))),
        &mut effects,
    );
    power.update(Message::Observed(Ok(fixture())), &mut effects);
    power.update(task.run(), &mut effects);
    assert!(!power.busy());
    assert!(effects.tasks.is_empty());
    assert_eq!(adapter.0.lock().unwrap().len(), 1);
}

#[test]
fn brightness_keeps_only_latest_demand_and_close_discards_it() {
    for close in [false, true] {
        let adapter = Arc::new(Adapter::default());
        let mut power = Power::new(adapter.clone());
        let mut effects = Effects::default();
        let target = PowerObjectId::new(3).unwrap();
        power.update(Message::Observed(Ok(fixture())), &mut effects);
        power.update(Message::Open, &mut effects);
        let event = |value| {
            Message::Brightness(
                target,
                SliderEvent {
                    value,
                    finished: false,
                },
            )
        };
        power.update(event(0.4), &mut effects);
        let first = effects.tasks.remove(0);
        power.update(first.run(), &mut effects);
        for value in [0.2, 0.6, 0.8] {
            power.update(event(value), &mut effects);
        }
        assert!(effects.tasks.is_empty());
        assert_eq!(
            power.snapshot().unwrap().brightness,
            fixture().brightness,
            "Preview must not rewrite observation"
        );
        if close {
            power.update(Message::Close, &mut effects);
        }
        let mut complete = fixture();
        complete.operation = Some(PowerOperation {
            id: PowerOperationId::new(10).unwrap(),
            command: PowerCommand::SetBrightness(target, Percentage::new(40.).unwrap()),
            phase: PowerOperationPhase::Succeeded,
        });
        power.update(Message::Observed(Ok(complete)), &mut effects);
        if close {
            assert!(effects.tasks.is_empty());
            assert!(power.brightness.is_none());
        } else {
            assert_eq!(effects.tasks.len(), 1);
            let latest = effects.tasks.remove(0);
            power.update(latest.run(), &mut effects);
            let calls = adapter.0.lock().unwrap();
            assert_eq!(calls.len(), 2);
            assert!(
                matches!(calls[1],PowerCommand::SetBrightness(id,value) if id==target&&(value.value()-80.).abs()<0.00001)
            );
        }
    }
}
