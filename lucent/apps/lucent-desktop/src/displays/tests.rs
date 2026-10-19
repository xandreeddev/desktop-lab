use super::*;
use std::sync::Mutex;
#[derive(Default)]
struct Adapter(Mutex<Vec<&'static str>>);
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
        self.0.lock().unwrap().push("apply");
        DisplayTransactionId::new(7)
    }
    fn keep(&self, _: DisplayTransactionId) -> Result<()> {
        self.0.lock().unwrap().push("keep");
        Ok(())
    }
    fn revert(&self, _: DisplayTransactionId) -> Result<()> {
        self.0.lock().unwrap().push("revert");
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
    assert_eq!(*adapter.0.lock().unwrap(), ["apply"]);
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
    assert_eq!(*adapter.0.lock().unwrap(), ["apply", "revert"]);
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
    assert_eq!(*adapter.0.lock().unwrap(), ["apply", "revert"]);
}
