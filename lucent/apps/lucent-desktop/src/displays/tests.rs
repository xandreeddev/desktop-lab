use super::*;
use std::sync::Mutex;
#[derive(Debug, PartialEq, Eq)]
enum Call {
    Apply(DisplayTransactionId),
    Keep(DisplayTransactionId),
    Revert(DisplayTransactionId),
}
#[derive(Default)]
struct Adapter(Mutex<Vec<Call>>);
impl DisplayPort for Adapter {
    fn read(&self) -> Result<DisplaySnapshot> {
        Ok(fixture())
    }
    fn watch(&self, _: &mut dyn FnMut(Result<DisplaySnapshot>), _: &dyn StopSignal) {}
    fn apply(
        &self,
        _: Vec<DisplayConfiguration>,
        _: Vec<DisplayConfiguration>,
    ) -> Result<DisplayTransactionId> {
        let mut calls = self.0.lock().unwrap();
        let id = DisplayTransactionId::new(
            7 + calls
                .iter()
                .filter(|call| matches!(call, Call::Apply(_)))
                .count() as u64,
        )?;
        calls.push(Call::Apply(id));
        Ok(id)
    }
    fn keep(&self, id: DisplayTransactionId) -> Result<()> {
        self.0.lock().unwrap().push(Call::Keep(id));
        Ok(())
    }
    fn revert(&self, id: DisplayTransactionId) -> Result<()> {
        self.0.lock().unwrap().push(Call::Revert(id));
        Ok(())
    }
}
pub fn fixture() -> DisplaySnapshot {
    let mode = DisplayMode::new(1920, 1080, 60000).unwrap();
    DisplaySnapshot {
        revision: 1,
        outputs: ["DP-1", "DP-2"]
            .into_iter()
            .map(|id| DisplayDevice {
                current: DisplayConfiguration {
                    output: MonitorId::new(id).unwrap(),
                    enabled: true,
                    mode,
                    scale: DisplayScale::new(1000).unwrap(),
                    position: DisplayPosition::new(0, 0).unwrap(),
                    transform: 0,
                },
                modes: vec![mode, DisplayMode::new(1280, 720, 60000).unwrap()],
                description: "Example display".into(),
                editable: true,
            })
            .collect(),
        transaction: None,
    }
}
fn open(child: &mut Displays, effects: &mut Effects<Message>) {
    child.update(Message::Open, effects);
    while !effects.tasks.is_empty() {
        let task = effects.tasks.remove(0);
        child.update(task.run(), effects);
    }
}
fn transaction(value: u64) -> DisplayTransactionId {
    DisplayTransactionId::new(value).unwrap()
}
fn observe_transaction(child: &mut Displays, id: u64, phase: DisplayTransactionPhase) {
    let mut snapshot = fixture();
    snapshot.revision = child.snapshot.as_ref().map_or(1, |s| s.revision + 1);
    snapshot.transaction = Some(DisplayTransaction {
        id: transaction(id),
        phase,
    });
    child.update(Message::Observed(Ok(snapshot)), &mut Effects::default());
}
fn awaiting(seconds: u32) -> DisplayTransactionPhase {
    DisplayTransactionPhase::AwaitingConfirmation {
        remaining_seconds: seconds,
    }
}
fn confirmation_click(child: &Displays, id: &str) -> Message {
    fn find(element: &Element<Message>, id: &str) -> Option<Message> {
        if element.id == id {
            element.click.clone()
        } else {
            element.children.iter().find_map(|child| find(child, id))
        }
    }
    let theme = lucent_design::Theme::from_palette(&lucent_design::palettes()[0]);
    find(
        &child.view(
            &ViewContext {
                surface: "displays",
                instance: None,
                width: 640.,
                height: 720.,
                now: 1.,
                density: Density::ONE,
            },
            theme,
        ),
        id,
    )
    .expect("rendered confirmation action")
}
fn begin_apply(child: &mut Displays, effects: &mut Effects<Message>) {
    child.update(
        Message::Mode(
            MonitorId::new("DP-1").unwrap(),
            DisplayMode::new(1280, 720, 60000).unwrap(),
        ),
        effects,
    );
    child.update(Message::Apply, effects);
    assert_eq!(effects.tasks.len(), 1);
}

#[test]
fn rendered_confirmations_cannot_keep_or_revert_a_replacement_transaction() {
    for terminal in [
        DisplayTransactionPhase::Kept,
        DisplayTransactionPhase::Reverted {
            reason: "Timed out".into(),
        },
        DisplayTransactionPhase::Failed {
            reason: "Preview failed".into(),
        },
    ] {
        for (control, keep) in [("display-keep", true), ("display-revert", false)] {
            let adapter = Arc::new(Adapter::default());
            let mut child = Displays::new(adapter.clone());
            let mut effects = Effects::default();
            open(&mut child, &mut effects);
            observe_transaction(&mut child, 7, awaiting(12));
            let retired = confirmation_click(&child, control);
            observe_transaction(&mut child, 7, terminal.clone());
            observe_transaction(&mut child, 8, awaiting(12));
            let current = confirmation_click(&child, control);
            // A watched confirmation is authoritative even without a locally
            // initiated Apply, as in the existing native visual fixture.
            assert!(child.transaction.is_none());
            let before = child.snapshot.clone();
            child.update(retired, &mut effects);
            assert!(effects.tasks.is_empty());
            assert!(adapter.0.lock().unwrap().is_empty());
            assert_eq!(child.snapshot, before);
            child.update(current.clone(), &mut effects);
            child.update(current, &mut effects);
            assert_eq!(
                effects.tasks.len(),
                1,
                "busy callbacks cannot duplicate work"
            );
            let task = effects.tasks.pop().unwrap();
            assert_eq!(task.queue, crate::work::CONTROLS);
            child.update(task.run(), &mut effects);
            assert_eq!(
                *adapter.0.lock().unwrap(),
                [if keep {
                    Call::Keep(transaction(8))
                } else {
                    Call::Revert(transaction(8))
                }]
            );
            assert_eq!(
                child.snapshot, before,
                "acknowledgement does not invent native state"
            );
            assert!(child.pending());
        }
    }
}

#[test]
fn reopening_and_rollback_acknowledgement_cannot_revive_a_cancelled_confirmation() {
    for control in ["display-keep", "display-revert"] {
        let adapter = Arc::new(Adapter::default());
        let mut child = Displays::new(adapter.clone());
        let mut effects = Effects::default();
        open(&mut child, &mut effects);
        observe_transaction(&mut child, 7, awaiting(12));
        let old_generation = child.close_generation;
        let old_click = confirmation_click(&child, control);
        child.update(Message::Close, &mut effects);
        let rollback = effects.tasks.pop().unwrap();
        child.update(Message::Open, &mut effects);
        assert_ne!(child.close_generation, old_generation);
        child.update(old_click.clone(), &mut effects);
        assert!(effects.tasks.is_empty());
        child.update(rollback.run(), &mut effects);
        assert!(child.active.is_none());
        assert_eq!(child.retired_confirmation, Some(transaction(7)));
        // Even a newer countdown can still precede rollback's terminal watch.
        observe_transaction(&mut child, 7, awaiting(8));
        child.update(old_click, &mut effects);
        let new_click = confirmation_click(&child, control);
        assert!(matches!(new_click, Message::Ignore));
        child.update(new_click, &mut effects);
        child.update(
            Message::Keep(child.close_generation, transaction(7)),
            &mut effects,
        );
        child.update(
            Message::Revert(child.close_generation, transaction(7)),
            &mut effects,
        );
        assert!(effects.tasks.is_empty());
        assert_eq!(*adapter.0.lock().unwrap(), [Call::Revert(transaction(7))]);
        observe_transaction(
            &mut child,
            7,
            DisplayTransactionPhase::Reverted {
                reason: "Closed".into(),
            },
        );
        assert!(child.retired_confirmation.is_none());
        observe_transaction(&mut child, 8, awaiting(12));
        let fresh = confirmation_click(&child, control);
        child.update(fresh, &mut effects);
        assert_eq!(
            effects.tasks.len(),
            1,
            "a different preview still admits current input"
        );
    }
}

#[test]
fn explicit_revert_retires_confirmation_until_terminal_observation() {
    let adapter = Arc::new(Adapter::default());
    let mut child = Displays::new(adapter.clone());
    let mut effects = Effects::default();
    open(&mut child, &mut effects);
    observe_transaction(&mut child, 7, awaiting(12));
    let keep = confirmation_click(&child, "display-keep");
    let revert = confirmation_click(&child, "display-revert");
    child.update(revert.clone(), &mut effects);
    let task = effects.tasks.pop().unwrap();
    child.update(task.run(), &mut effects);
    child.update(keep, &mut effects);
    child.update(revert, &mut effects);
    assert!(effects.tasks.is_empty());
    assert!(matches!(
        confirmation_click(&child, "display-keep"),
        Message::Ignore
    ));
    assert_eq!(*adapter.0.lock().unwrap(), [Call::Revert(transaction(7))]);
}

#[test]
fn confirmation_callbacks_capture_the_panel_lifetime_as_well_as_transaction() {
    let mut child = Displays::new(Arc::new(Adapter::default()));
    let mut effects = Effects::default();
    open(&mut child, &mut effects);
    observe_transaction(&mut child, 7, awaiting(12));
    let generation = child.close_generation;
    assert!(matches!(
        confirmation_click(&child, "display-keep"),
        Message::Keep(captured, id) if captured == generation && id == transaction(7)
    ));
    assert!(matches!(
        confirmation_click(&child, "display-revert"),
        Message::Revert(captured, id) if captured == generation && id == transaction(7)
    ));
    for stale in [
        Message::Keep(generation.wrapping_sub(1), transaction(7)),
        Message::Revert(generation.wrapping_sub(1), transaction(7)),
    ] {
        child.update(stale, &mut effects);
        assert!(effects.tasks.is_empty());
    }
}

#[test]
fn countdown_updates_preserve_the_same_transaction_callback() {
    for (control, keep) in [("display-keep", true), ("display-revert", false)] {
        let adapter = Arc::new(Adapter::default());
        let mut child = Displays::new(adapter.clone());
        let mut effects = Effects::default();
        open(&mut child, &mut effects);
        observe_transaction(&mut child, 7, awaiting(12));
        let click = confirmation_click(&child, control);
        observe_transaction(&mut child, 7, awaiting(8));
        child.update(click, &mut effects);
        assert_eq!(effects.tasks.len(), 1);
        child.update(effects.tasks.pop().unwrap().run(), &mut Effects::default());
        assert_eq!(
            *adapter.0.lock().unwrap(),
            [if keep {
                Call::Keep(transaction(7))
            } else {
                Call::Revert(transaction(7))
            }]
        );
    }
}

#[test]
fn confirmation_requires_an_open_idle_current_confirmation_phase() {
    for control in ["display-keep", "display-revert"] {
        for state in [
            "closed",
            "preparing",
            "kept",
            "reverted",
            "failed",
            "missing",
            "refreshing",
            "active",
        ] {
            let adapter = Arc::new(Adapter::default());
            let mut child = Displays::new(adapter.clone());
            let mut effects = Effects::default();
            open(&mut child, &mut effects);
            observe_transaction(&mut child, 7, awaiting(12));
            let click = confirmation_click(&child, control);
            match state {
                "closed" => child.set_open(false, 1.),
                "preparing" => {
                    observe_transaction(&mut child, 7, DisplayTransactionPhase::Preparing)
                }
                "kept" => observe_transaction(&mut child, 7, DisplayTransactionPhase::Kept),
                "reverted" => observe_transaction(
                    &mut child,
                    7,
                    DisplayTransactionPhase::Reverted {
                        reason: "Timed out".into(),
                    },
                ),
                "failed" => observe_transaction(
                    &mut child,
                    7,
                    DisplayTransactionPhase::Failed {
                        reason: "Failed".into(),
                    },
                ),
                "missing" => child.snapshot.as_mut().unwrap().transaction = None,
                "refreshing" => child.refreshing = Some((child.revision, false)),
                "active" => child.active = Some(child.revision),
                _ => unreachable!(),
            }
            child.update(click, &mut effects);
            assert!(effects.tasks.is_empty(), "{control} acted while {state}");
            assert!(adapter.0.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn close_rolls_back_a_watched_preparation_without_ui_confirmation_authority() {
    let adapter = Arc::new(Adapter::default());
    let mut child = Displays::new(adapter.clone());
    let mut effects = Effects::default();
    open(&mut child, &mut effects);
    observe_transaction(&mut child, 9, DisplayTransactionPhase::Preparing);
    assert!(child.transaction.is_none());
    child.update(Message::Close, &mut effects);
    assert!(!child.open());
    assert_eq!(effects.tasks.len(), 1);
    child.update(effects.tasks.pop().unwrap().run(), &mut Effects::default());
    assert_eq!(*adapter.0.lock().unwrap(), [Call::Revert(transaction(9))]);
}

#[test]
fn reopening_cannot_restore_authority_to_an_apply_retired_by_close() {
    for close in [Message::Close, Message::Toggle] {
        let adapter = Arc::new(Adapter::default());
        let mut child = Displays::new(adapter.clone());
        let mut effects = Effects::default();
        open(&mut child, &mut effects);
        begin_apply(&mut child, &mut effects);
        let apply = effects.tasks.pop().unwrap();
        child.update(close, &mut effects);
        assert!(!child.open());
        child.update(Message::Open, &mut effects);
        assert!(child.open() && child.pending());
        assert!(effects.tasks.is_empty());
        child.update(apply.run(), &mut effects);
        assert_eq!(
            effects.tasks.len(),
            1,
            "retired admission must roll back even after reopening"
        );
        let rollback = effects.tasks.pop().unwrap();
        child.update(rollback.run(), &mut effects);
        assert_eq!(
            *adapter.0.lock().unwrap(),
            [Call::Apply(transaction(7)), Call::Revert(transaction(7))]
        );
        observe_transaction(
            &mut child,
            7,
            DisplayTransactionPhase::Reverted {
                reason: "Panel closed".into(),
            },
        );
        assert!(child.open() && !child.pending());
        begin_apply(&mut child, &mut effects);
        child.update(effects.tasks.pop().unwrap().run(), &mut Effects::default());
        assert_eq!(
            child.transaction,
            Some(transaction(8)),
            "a fresh Apply after rollback remains usable"
        );
        assert_eq!(
            *adapter.0.lock().unwrap(),
            [
                Call::Apply(transaction(7)),
                Call::Revert(transaction(7)),
                Call::Apply(transaction(8))
            ]
        );
    }
}

#[test]
fn rejected_retired_admission_does_not_stick_busy_or_start_rollback() {
    let adapter = Arc::new(Adapter::default());
    let mut child = Displays::new(adapter.clone());
    let mut effects = Effects::default();
    open(&mut child, &mut effects);
    begin_apply(&mut child, &mut effects);
    let revision = child.active.unwrap();
    effects.tasks.clear();
    child.update(Message::Close, &mut effects);
    child.update(Message::Open, &mut effects);
    child.update(
        Message::Admitted(revision, Err(DomainError::Failed("Apply refused".into()))),
        &mut effects,
    );
    assert!(child.open() && !child.pending());
    assert!(child.error.contains("Apply refused"));
    assert!(effects.tasks.is_empty());
    assert!(adapter.0.lock().unwrap().is_empty());
}

#[test]
fn stale_successful_admission_rolls_back_its_id_without_retiring_a_new_apply() {
    let adapter = Arc::new(Adapter::default());
    let mut child = Displays::new(adapter.clone());
    let mut effects = Effects::default();
    open(&mut child, &mut effects);
    begin_apply(&mut child, &mut effects);
    let old = effects.tasks.pop().unwrap().run();
    child.update(old.clone(), &mut effects);
    observe_transaction(
        &mut child,
        7,
        DisplayTransactionPhase::Reverted {
            reason: "Retired".into(),
        },
    );
    begin_apply(&mut child, &mut effects);
    let current = effects.tasks.pop().unwrap();
    let active = child.active;
    child.update(old, &mut effects);
    assert_eq!(child.active, active);
    let rollback = effects.tasks.pop().unwrap();
    child.update(rollback.run(), &mut effects);
    assert_eq!(child.active, active);
    child.update(current.run(), &mut effects);
    assert_eq!(child.transaction, Some(transaction(8)));
    assert_eq!(
        *adapter.0.lock().unwrap(),
        [
            Call::Apply(transaction(7)),
            Call::Revert(transaction(7)),
            Call::Apply(transaction(8))
        ]
    );
}
#[test]
fn reset_reads_current_native_state_without_a_watch_event() {
    let mut child = Displays::new(Arc::new(Adapter::default()));
    let mut effects = Effects::default();
    let mut old = fixture();
    old.revision = 0;
    old.outputs[0].current.position = DisplayPosition::new(16, 0).unwrap();
    old.outputs[0].current.mode = DisplayMode::new(1280, 720, 60000).unwrap();
    child.update(Message::Observed(Ok(old)), &mut effects);
    child.open = true;
    child.dirty = true;
    child.update(Message::Refresh, &mut effects);
    assert!(child.pending());
    assert_eq!(child.baseline[0].position.x(), 16);
    let result = effects.tasks.remove(0).run();
    child.update(result, &mut effects);
    assert!(!child.pending() && !child.dirty);
    assert_eq!(child.baseline, fixture().configuration());
    assert_eq!(child.rows[0].values[0], "0");
}
#[test]
fn opening_reads_fresh_state_and_closed_or_replaced_reads_have_no_authority() {
    let mut child = Displays::new(Arc::new(Adapter::default()));
    let mut effects = Effects::default();
    child.update(Message::Open, &mut effects);
    let late = effects.tasks.remove(0).run();
    child.update(Message::Close, &mut effects);
    child.update(late.clone(), &mut effects);
    assert!(child.snapshot.is_none());
    child.update(Message::Open, &mut effects);
    child.update(late, &mut effects);
    assert!(child.refreshing.is_some() && child.snapshot.is_none());
    let result = effects.tasks.remove(0).run();
    child.update(result, &mut effects);
    assert!(!child.pending());
    assert_eq!(child.baseline, fixture().configuration());
}
#[test]
fn old_watch_or_read_cannot_replace_a_newer_captured_observation() {
    let mut child = Displays::new(Arc::new(Adapter::default()));
    let mut effects = Effects::default();
    child.update(Message::Open, &mut effects);
    let older_read = effects.tasks.remove(0).run();
    let mut fresh = fixture();
    fresh.revision = 3;
    fresh.outputs[0].current.position = DisplayPosition::new(64, 0).unwrap();
    child.update(Message::Observed(Ok(fresh.clone())), &mut effects);
    child.update(older_read, &mut effects);
    assert_eq!(child.baseline, fresh.configuration());
    child.update(Message::Observed(Ok(fixture())), &mut effects);
    assert_eq!(child.baseline, fresh.configuration());
}
#[test]
fn failed_refresh_preserves_unsaved_draft_and_blocks_apply_until_recovery() {
    let mut child = Displays::new(Arc::new(Adapter::default()));
    let mut effects = Effects::default();
    open(&mut child, &mut effects);
    child.rows[0].values[0] = "32".into();
    child.dirty = true;
    child.update(Message::Refresh, &mut effects);
    let revision = child.refreshing.unwrap().0;
    effects.tasks.clear();
    child.update(
        Message::Refreshed(
            revision,
            Err(DomainError::Unavailable("Disconnected".into())),
        ),
        &mut effects,
    );
    assert_eq!(child.rows[0].values[0], "32");
    assert!(child.dirty && !child.observation_error.is_empty());
    child.update(Message::Apply, &mut effects);
    assert!(effects.tasks.is_empty());
    child.update(Message::Refresh, &mut effects);
    let result = effects.tasks.remove(0).run();
    child.update(result, &mut effects);
    assert!(!child.dirty && child.observation_error.is_empty());
}
#[test]
fn a_late_reset_after_close_cannot_keep_the_next_open_permanently_busy() {
    let mut child = Displays::new(Arc::new(Adapter::default()));
    let mut effects = Effects::default();
    open(&mut child, &mut effects);
    child.update(Message::Close, &mut effects);
    child.update(Message::Refresh, &mut effects);
    assert!(effects.tasks.is_empty());
    assert!(child.refreshing.is_none());
    child.update(Message::Open, &mut effects);
    assert!(child.pending());
    let result = effects.tasks.remove(0).run();
    child.update(result, &mut effects);
    assert!(child.open() && !child.pending());
    assert_eq!(child.baseline, fixture().configuration());
}
#[test]
fn draft_changes_do_not_apply_until_explicit_action() {
    let adapter = Arc::new(Adapter::default());
    let mut child = Displays::new(adapter.clone());
    let mut effects = Effects::default();
    child.update(Message::Observed(Ok(fixture())), &mut effects);
    open(&mut child, &mut effects);
    let id = MonitorId::new("DP-1").unwrap();
    child.update(
        Message::Mode(id, DisplayMode::new(1280, 720, 60000).unwrap()),
        &mut effects,
    );
    assert!(effects.tasks.is_empty());
    assert!(adapter.0.lock().unwrap().is_empty());
    child.update(Message::Apply, &mut effects);
    let task = effects.tasks.remove(0);
    assert_eq!(task.queue, crate::work::CONTROLS);
    child.update(task.run(), &mut effects);
    assert_eq!(
        child.transaction,
        Some(DisplayTransactionId::new(7).unwrap())
    );
    assert_eq!(*adapter.0.lock().unwrap(), [Call::Apply(transaction(7))]);
    assert!(child.snapshot.as_ref().unwrap().transaction.is_none());
}
#[test]
fn close_before_admission_reverts_the_exact_late_transaction() {
    let adapter = Arc::new(Adapter::default());
    let mut child = Displays::new(adapter.clone());
    let mut effects = Effects::default();
    child.update(Message::Observed(Ok(fixture())), &mut effects);
    open(&mut child, &mut effects);
    child.update(
        Message::Mode(
            MonitorId::new("DP-1").unwrap(),
            DisplayMode::new(1280, 720, 60000).unwrap(),
        ),
        &mut effects,
    );
    child.update(Message::Apply, &mut effects);
    let apply = effects.tasks.remove(0);
    child.update(Message::Close, &mut effects);
    child.update(apply.run(), &mut effects);
    while !effects.tasks.is_empty() {
        let task = effects.tasks.remove(0);
        child.update(task.run(), &mut effects);
    }
    assert_eq!(
        *adapter.0.lock().unwrap(),
        [Call::Apply(transaction(7)), Call::Revert(transaction(7))]
    );
    assert!(!child.open());
}
#[test]
fn disabling_last_output_or_changed_native_baseline_blocks_apply() {
    for stale in [false, true] {
        let adapter = Arc::new(Adapter::default());
        let mut child = Displays::new(adapter.clone());
        let mut effects = Effects::default();
        child.update(Message::Observed(Ok(fixture())), &mut effects);
        open(&mut child, &mut effects);
        for id in ["DP-1", "DP-2"] {
            child.update(Message::Enable(MonitorId::new(id).unwrap()), &mut effects);
        }
        if stale {
            let mut snapshot = fixture();
            snapshot.outputs[0].current.position = DisplayPosition::new(200, 0).unwrap();
            child.update(Message::Observed(Ok(snapshot)), &mut effects);
        }
        child.update(Message::Apply, &mut effects);
        assert!(effects.tasks.is_empty());
        assert!(!child.error.is_empty());
    }
}

#[test]
fn terminal_watch_before_admission_cannot_leave_a_stuck_transaction() {
    let adapter = Arc::new(Adapter::default());
    let mut child = Displays::new(adapter);
    let mut effects = Effects::default();
    child.update(Message::Observed(Ok(fixture())), &mut effects);
    open(&mut child, &mut effects);
    child.update(
        Message::Mode(
            MonitorId::new("DP-1").unwrap(),
            DisplayMode::new(1280, 720, 60000).unwrap(),
        ),
        &mut effects,
    );
    child.update(Message::Apply, &mut effects);
    let task = effects.tasks.remove(0);
    let admission = task.run();
    let mut snapshot = fixture();
    snapshot.transaction = Some(DisplayTransaction {
        id: DisplayTransactionId::new(7).unwrap(),
        phase: DisplayTransactionPhase::Reverted {
            reason: "Preview failed".into(),
        },
    });
    child.update(Message::Observed(Ok(snapshot)), &mut effects);
    child.update(admission, &mut effects);
    assert!(!child.pending());
    assert!(!child.dirty);
    assert!(child.transaction.is_none());
}
#[test]
fn admitted_preview_remains_pending_until_watch_or_explicit_close() {
    let adapter = Arc::new(Adapter::default());
    let mut child = Displays::new(adapter.clone());
    let mut effects = Effects::default();
    child.update(Message::Observed(Ok(fixture())), &mut effects);
    open(&mut child, &mut effects);
    child.update(
        Message::Mode(
            MonitorId::new("DP-1").unwrap(),
            DisplayMode::new(1280, 720, 60000).unwrap(),
        ),
        &mut effects,
    );
    child.update(Message::Apply, &mut effects);
    let task = effects.tasks.remove(0);
    child.update(task.run(), &mut effects);
    assert!(child.pending());
    child.update(Message::Close, &mut effects);
    let revert = effects.tasks.remove(0);
    child.update(revert.run(), &mut effects);
    assert_eq!(
        *adapter.0.lock().unwrap(),
        [Call::Apply(transaction(7)), Call::Revert(transaction(7))]
    );
}
