use super::*;
use std::sync::Mutex;
#[derive(Default)]
struct Adapter(Mutex<Vec<&'static str>>);
impl DisplayPort for Adapter {
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
#[test]
fn draft_changes_do_not_apply_until_explicit_action() {
    let adapter = Arc::new(Adapter::default());
    let mut child = Displays::new(adapter.clone());
    let mut effects = Effects::default();
    child.update(Message::Observed(Ok(fixture())), &mut effects);
    child.update(Message::Open, &mut effects);
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
    child.update(Message::Open, &mut effects);
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
        child.update(Message::Open, &mut effects);
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
