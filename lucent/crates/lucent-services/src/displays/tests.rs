use super::*;
use std::{
    cell::{Cell, RefCell},
    sync::mpsc,
};
fn device(name: &str) -> DisplayDevice {
    let mode = DisplayMode::new(1920, 1080, 60000).unwrap();
    DisplayDevice {
        current: DisplayConfiguration {
            output: MonitorId::new(name).unwrap(),
            enabled: true,
            mode,
            scale: DisplayScale::new(1000).unwrap(),
            position: DisplayPosition::new(0, 0).unwrap(),
            transform: 0,
        },
        description: String::new(),
        modes: vec![mode, DisplayMode::new(1280, 720, 60000).unwrap()],
        editable: true,
    }
}
struct Fake {
    outputs: RefCell<Vec<DisplayDevice>>,
    calls: RefCell<Vec<Vec<DisplayConfiguration>>>,
    fail_apply: Cell<bool>,
}
impl Fake {
    fn new() -> Self {
        Self {
            outputs: RefCell::new(vec![device("DP-1"), device("DP-2")]),
            calls: RefCell::new(vec![]),
            fail_apply: Cell::new(false),
        }
    }
}
impl guard::Driver for Fake {
    fn identity(&self) -> Result<(u64, u64)> {
        Ok((1, 2))
    }
    fn outputs(&self) -> Result<Vec<DisplayDevice>> {
        Ok(self.outputs.borrow().clone())
    }
    fn apply(&self, layout: &[DisplayConfiguration]) -> Result<()> {
        self.calls.borrow_mut().push(layout.to_vec());
        for config in layout {
            if let Some(output) = self
                .outputs
                .borrow_mut()
                .iter_mut()
                .find(|o| o.current.output == config.output)
            {
                output.current = config.clone();
            }
        }
        if self.fail_apply.replace(false) {
            Err(failed("Injected partial native failure"))
        } else {
            Ok(())
        }
    }
}
fn request(driver: &Fake) -> guard::Request {
    let baseline = driver
        .outputs
        .borrow()
        .iter()
        .map(|o| o.current.clone())
        .collect::<Vec<_>>();
    let mut draft = baseline.clone();
    draft[0].mode = DisplayMode::new(1280, 720, 60000).unwrap();
    draft[1].position = DisplayPosition::new(1280, 0).unwrap();
    guard::Request {
        id: DisplayTransactionId::new(1).unwrap(),
        instance: (1, 2),
        baseline,
        draft,
    }
}
fn final_phase(bytes: &[u8]) -> DisplayTransactionPhase {
    serde_json::from_slice::<DisplayTransaction>(
        bytes
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .next_back()
            .unwrap(),
    )
    .unwrap()
    .phase
}
#[test]
fn explicit_revert_restores_the_entire_captured_layout() {
    let driver = Fake::new();
    let request = request(&driver);
    let baseline = request.baseline.clone();
    let (tx, rx) = mpsc::channel();
    tx.send(guard::Command::Revert).unwrap();
    let mut out = vec![];
    guard::run_transaction(&driver, request, rx, &mut out, &|| Ok(0), &|_| {
        panic!("Revert must not persist")
    })
    .unwrap();
    assert!(matches!(
        final_phase(&out),
        DisplayTransactionPhase::Reverted { .. }
    ));
    assert_eq!(
        driver
            .outputs
            .borrow()
            .iter()
            .map(|o| o.current.clone())
            .collect::<Vec<_>>(),
        baseline
    );
    assert_eq!(driver.calls.borrow().len(), 2);
}
#[test]
fn client_crash_eof_restores_without_client_updates_or_frames() {
    let driver = Fake::new();
    let request = request(&driver);
    let baseline = request.baseline.clone();
    let (tx, rx) = mpsc::channel();
    drop(tx);
    guard::run_transaction(&driver, request, rx, &mut vec![], &|| Ok(0), &|_| {
        panic!("EOF must not persist")
    })
    .unwrap();
    assert_eq!(*driver.calls.borrow().last().unwrap(), baseline);
}
#[test]
fn boot_clock_timeout_restores_without_a_client_command() {
    let driver = Fake::new();
    let request = request(&driver);
    let baseline = request.baseline.clone();
    let (_tx, rx) = mpsc::channel();
    let calls = Cell::new(0);
    let mut out = vec![];
    guard::run_transaction(
        &driver,
        request,
        rx,
        &mut out,
        &|| {
            let next = calls.get() + 1;
            calls.set(next);
            Ok(if next < 3 { 0 } else { 16_000 })
        },
        &|_| panic!("Timeout must not persist"),
    )
    .unwrap();
    assert!(matches!(
        final_phase(&out),
        DisplayTransactionPhase::Reverted { .. }
    ));
    assert_eq!(*driver.calls.borrow().last().unwrap(), baseline);
}
#[test]
fn keep_persists_only_after_observed_preview_and_does_not_rollback() {
    let driver = Fake::new();
    let request = request(&driver);
    let draft = request.draft.clone();
    let (tx, rx) = mpsc::channel();
    tx.send(guard::Command::Keep).unwrap();
    let saved = Cell::new(false);
    let mut out = vec![];
    guard::run_transaction(&driver, request, rx, &mut out, &|| Ok(0), &|layout| {
        assert_eq!(layout, draft);
        saved.set(true);
        Ok(())
    })
    .unwrap();
    assert!(saved.get());
    assert_eq!(final_phase(&out), DisplayTransactionPhase::Kept);
    assert_eq!(driver.calls.borrow().len(), 1);
}
#[test]
fn failed_keep_and_partial_apply_restore_baseline() {
    for fail_native in [false, true] {
        let driver = Fake::new();
        driver.fail_apply.set(fail_native);
        let request = request(&driver);
        let baseline = request.baseline.clone();
        let (tx, rx) = mpsc::channel();
        tx.send(guard::Command::Keep).unwrap();
        let mut out = vec![];
        guard::run_transaction(&driver, request, rx, &mut out, &|| Ok(0), &|_| {
            Err(failed("Disk failure"))
        })
        .unwrap();
        assert!(matches!(
            final_phase(&out),
            DisplayTransactionPhase::Reverted { .. }
        ));
        assert_eq!(*driver.calls.borrow().last().unwrap(), baseline);
    }
}
#[test]
fn clock_failure_after_mutation_also_restores() {
    let driver = Fake::new();
    let request = request(&driver);
    let baseline = request.baseline.clone();
    let (_tx, rx) = mpsc::channel();
    let calls = Cell::new(0);
    assert!(
        guard::run_transaction(
            &driver,
            request,
            rx,
            &mut vec![],
            &|| {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    Ok(0)
                } else {
                    Err(failed("Clock failure"))
                }
            },
            &|_| Ok(())
        )
        .is_err()
    );
    assert_eq!(*driver.calls.borrow().last().unwrap(), baseline);
}
#[test]
fn stale_baseline_is_rejected_before_any_mutation() {
    let driver = Fake::new();
    let request = request(&driver);
    driver.outputs.borrow_mut()[0].current.position = DisplayPosition::new(-1920, 0).unwrap();
    let (_tx, rx) = mpsc::channel();
    assert!(
        guard::run_transaction(&driver, request, rx, &mut vec![], &|| Ok(0), &|_| Ok(())).is_err()
    );
    assert!(driver.calls.borrow().is_empty());
}
#[test]
fn native_decoder_handles_disabled_modes_and_literal_lua() {
    let value = serde_json::json!([{"name":"DP-1","width":1920,"height":1080,"refreshRate":60.,"x":-1920,"y":0,"scale":1.,"transform":0,"availableModes":["1920x1080@60.00Hz","1280x720@59.94Hz"],"mirrorOf":"none"},{"name":"DP-2","width":0,"height":0,"refreshRate":0.,"disabled":true,"availableModes":["1920x1080@60.00Hz"]}]);
    let outputs = native::decode(&value).unwrap();
    assert_eq!(outputs.len(), 2);
    assert!(!outputs[1].current.enabled);
    let mut config = outputs[0].current.clone();
    config.output = MonitorId::new("DP-\"\\literal").unwrap();
    let lua = native::lua(&config);
    assert!(lua.contains("DP-\\\"\\\\literal"));
    assert!(lua.contains("position = \"-1920x0\""));
    assert_eq!(
        native::parse_mode("1280x720@59.94Hz").unwrap().millihertz(),
        59940
    );
    assert!(native::parse_mode("1920x1080@NaNHz").is_none());
}

#[test]
fn new_compositor_instance_rejects_even_an_identical_layout() {
    let driver = Fake::new();
    let mut request = request(&driver);
    request.instance = (9, 9);
    let (_tx, rx) = mpsc::channel();
    assert!(
        guard::run_transaction(&driver, request, rx, &mut vec![], &|| Ok(0), &|_| Ok(())).is_err()
    );
    assert!(driver.calls.borrow().is_empty());
}

#[test]
fn event_framing_retains_partial_names_and_rejects_unbounded_records() {
    let mut framing = EventFrames::default();
    assert_eq!(framing.push(b"monitoradd"), Some(false));
    assert_eq!(
        framing.push(b"edv2>>1,DP-1\nactivewindow>>a,b\n"),
        Some(true)
    );
    assert_eq!(
        framing.push(b"activewindow>>monitor in a title\n"),
        Some(false)
    );
    assert_eq!(framing.push(&vec![b'x'; 64 * 1024 + 1]), None);
}
