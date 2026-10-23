use super::*;
use lucent_domain::{
    AudioCommand, AudioDevice, AudioDeviceKey, AudioObjectId, AudioVolume, DomainError,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[test]
fn arguments_reject_controls_unbounded_durations_and_duplicate_options() {
    for args in [
        vec!["--stdio"],
        vec!["--seconds", "0"],
        vec!["--seconds", "601"],
        vec!["--seconds", "inf"],
        vec!["--seconds"],
        vec!["--seconds", "1", "--seconds", "2"],
        vec!["--observe", "--observe"],
        vec!["--volume", "0.5"],
    ] {
        assert!(arguments(args.into_iter().map(str::to_owned)).is_err());
    }
    assert_eq!(arguments([]).unwrap(), Duration::from_secs(10));
    assert_eq!(
        arguments(["--observe", "--seconds", "600"].map(str::to_owned)).unwrap(),
        Duration::from_secs(600)
    );
}

fn snapshot() -> AudioSnapshot {
    let output = AudioObjectId::new(1).unwrap();
    AudioSnapshot {
        devices: vec![AudioDevice {
            id: output,
            key: AudioDeviceKey::new("private-device-key").unwrap(),
            name: "private device name".into(),
            direction: AudioDirection::Playback,
            volume: AudioVolume::new(1.25).unwrap(),
            muted: true,
            monitor: false,
        }],
        default_output: Some(output),
        ..Default::default()
    }
}

#[test]
fn summaries_preserve_amplification_and_absence_without_private_metadata() {
    let ready = summary(Ok(snapshot()));
    assert_eq!(ready["outputs"], 1);
    assert_eq!(ready["inputs"], 0);
    assert_eq!(ready["default_output"], json!({"volume":1.25,"muted":true}));
    assert!(ready["default_input"].is_null());
    assert!(!ready.to_string().contains("private"));
    let unavailable = summary(Err(DomainError::Unavailable(
        "private backend address".into(),
    )));
    assert_eq!(unavailable, json!({"kind":"unavailable"}));
    assert!(unavailable.get("outputs").is_none());
}

struct Cancel(AtomicBool);
impl StopSignal for Cancel {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn wait(&self, _: Duration) {}
}
struct FakeAudio {
    watches: AtomicUsize,
}
impl AudioPort for FakeAudio {
    fn watch(&self, emit: &mut dyn FnMut(Result<AudioSnapshot>), stop: &dyn StopSignal) {
        self.watches.fetch_add(1, Ordering::Relaxed);
        emit(Err(DomainError::Unavailable(
            "private backend address".into(),
        )));
        emit(Ok(snapshot()));
        assert!(
            stop.cancelled(),
            "consumer cancellation must reach the owning watch"
        );
    }
    fn control(&self, _: AudioCommand) -> Result<()> {
        panic!("observer must never control audio")
    }
}

#[test]
fn observation_keeps_unavailable_evidence_then_reports_cancelled_subscription() {
    let adapter = Arc::new(FakeAudio {
        watches: AtomicUsize::new(0),
    });
    let stop = Cancel(AtomicBool::new(false));
    let mut events = Vec::new();
    let result = observe(adapter.clone(), &stop, |event| {
        if event["kind"] == "state" {
            stop.0.store(true, Ordering::Release);
        }
        events.push(event);
        Ok(())
    })
    .unwrap();
    assert_eq!(adapter.watches.load(Ordering::Relaxed), 1);
    assert_eq!(
        (result.states, result.unavailable, result.cancelled),
        (1, 1, true)
    );
    assert_eq!(events[0], json!({"kind":"unavailable"}));
    assert_eq!(
        events.last().unwrap(),
        &json!({"kind":"finished","states":1,"unavailable":1,
                                            "watch_returned_after_cancellation":true})
    );
}

#[test]
fn failed_output_cancels_watch_and_emits_no_later_state_or_success() {
    let adapter = Arc::new(FakeAudio {
        watches: AtomicUsize::new(0),
    });
    let stop = Cancel(AtomicBool::new(false));
    let mut writes = 0;
    let result = observe(adapter.clone(), &stop, |_| {
        writes += 1;
        Err(io::Error::from(io::ErrorKind::BrokenPipe))
    });
    let Err(error) = result else {
        panic!("failed output must fail observation");
    };
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(adapter.watches.load(Ordering::Relaxed), 1);
    assert_eq!(writes, 1, "no state or completion follows failed output");
    assert!(!stop.cancelled(), "output failure has its own cancellation");
}
