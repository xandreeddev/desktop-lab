//! Client command scheduling: one accepted operation plus bounded pending intent.
//! Transport acknowledgement never mutates the observed audio snapshot.
use lucent_api::Effects;
use lucent_domain::*;
use std::{collections::VecDeque, sync::Arc};

const MAX_PENDING: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Revision(u64);

#[derive(Clone)]
pub enum Message {
    Finished(Revision, Result<()>),
    WorkerFailed(String),
    Retry,
    DismissFailure,
}

struct Request {
    revision: Revision,
    command: AudioCommand,
}
struct Failure {
    revision: Revision,
    command: AudioCommand,
    reason: String,
}
pub struct Commands {
    adapter: Arc<dyn AudioPort>,
    revision: u64,
    active: Option<Request>,
    pending: VecDeque<Request>,
    failure: Option<Failure>,
}

fn valid(command: &AudioCommand, snapshot: Option<&AudioSnapshot>) -> bool {
    if matches!(
        command,
        AudioCommand::Raise | AudioCommand::Lower | AudioCommand::ToggleMute
    ) {
        return true;
    }
    let Some(snapshot) = snapshot else {
        return false;
    };
    match command {
        AudioCommand::SetVolume(id, _) => {
            snapshot.device(*id).is_some()
                || snapshot
                    .streams
                    .iter()
                    .any(|s| s.id == *id && s.volume_writable && s.volume.is_some())
        }
        AudioCommand::SetMute(id, _) => {
            snapshot.device(*id).is_some() || snapshot.streams.iter().any(|s| s.id == *id)
        }
        AudioCommand::SetDefault(id) => snapshot.device(*id).is_some(),
        AudioCommand::Route { stream, device } => snapshot.streams.iter().any(|s| {
            s.id == *stream
                && snapshot
                    .device(*device)
                    .is_some_and(|d| d.direction == s.direction)
        }),
        _ => true,
    }
}

fn same_control(a: &AudioCommand, b: &AudioCommand) -> bool {
    match (a, b) {
        (AudioCommand::SetVolume(a, _), AudioCommand::SetVolume(b, _))
        | (AudioCommand::SetMute(a, _), AudioCommand::SetMute(b, _)) => a == b,
        (AudioCommand::SetDefault(a), AudioCommand::SetDefault(b)) => a == b,
        (AudioCommand::Route { stream: a, .. }, AudioCommand::Route { stream: b, .. }) => a == b,
        // Relative default-output commands share the keyboard-control channel.
        (
            AudioCommand::Raise | AudioCommand::Lower | AudioCommand::ToggleMute,
            AudioCommand::Raise | AudioCommand::Lower | AudioCommand::ToggleMute,
        ) => true,
        _ => false,
    }
}

impl Commands {
    pub fn new(adapter: Arc<dyn AudioPort>) -> Self {
        Self {
            adapter,
            revision: 0,
            active: None,
            pending: VecDeque::new(),
            failure: None,
        }
    }
    pub fn error(&self) -> Option<&str> {
        self.failure.as_ref().map(|f| f.reason.as_str())
    }
    pub fn can_retry(&self, snapshot: Option<&AudioSnapshot>) -> bool {
        self.failure
            .as_ref()
            .is_some_and(|failure| valid(&failure.command, snapshot))
    }
    pub fn busy(&self) -> bool {
        self.active.is_some() || !self.pending.is_empty()
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    pub fn pending_volume(&self, id: AudioObjectId) -> Option<AudioVolume> {
        self.pending
            .iter()
            .rev()
            .chain(self.active.iter())
            .find_map(|r| match r.command {
                AudioCommand::SetVolume(target, value) if target == id => Some(value),
                _ => None,
            })
    }
    pub fn pending_mute(&self, id: AudioObjectId) -> Option<bool> {
        self.pending
            .iter()
            .rev()
            .chain(self.active.iter())
            .find_map(|r| match r.command {
                AudioCommand::SetMute(target, value) if target == id => Some(value),
                _ => None,
            })
    }

    pub fn request(
        &mut self,
        command: AudioCommand,
        snapshot: Option<&AudioSnapshot>,
        effects: &mut Effects<Message>,
    ) {
        let Some(next) = self.revision.checked_add(1) else {
            self.failure = Some(Failure {
                revision: Revision(u64::MAX),
                command,
                reason: "Audio command revision limit reached; restart before changing controls"
                    .into(),
            });
            return;
        };
        self.revision = next;
        let revision = Revision(next);
        let reason = if !valid(&command, snapshot) {
            Some("This audio control is no longer available".to_string())
        } else {
            None
        };
        if let Some(reason) = reason {
            self.failure = Some(Failure {
                revision,
                command,
                reason,
            });
            return;
        }
        // Replace only adjacent absolute volume samples. Never reorder across
        // mute/default/routing or relative keyboard operations.
        let replace = self.pending.back().is_some_and(|last| {
            matches!((&last.command, &command), (AudioCommand::SetVolume(a, _), AudioCommand::SetVolume(b, _)) if a == b)
        });
        if !replace && self.pending.len() == MAX_PENDING {
            self.failure = Some(Failure {
                revision,
                command,
                reason: "Audio controls are busy; retry the last change".into(),
            });
            return;
        }
        if replace {
            self.pending.pop_back();
        }
        self.pending.push_back(Request { revision, command });
        self.start(effects);
    }
    fn start(&mut self, effects: &mut Effects<Message>) {
        if self.active.is_some() {
            return;
        }
        let Some(request) = self.pending.pop_front() else {
            return;
        };
        let revision = request.revision;
        let command = request.command.clone();
        let adapter = self.adapter.clone();
        self.active = Some(request);
        effects.task_on("audio", move || {
            Message::Finished(revision, adapter.control(command))
        });
    }
    pub fn reconcile(&mut self, snapshot: Option<&AudioSnapshot>) {
        if snapshot.is_none() {
            self.pending.clear();
            return;
        }
        self.pending
            .retain(|request| valid(&request.command, snapshot));
    }
    pub fn update(
        &mut self,
        message: Message,
        snapshot: Option<&AudioSnapshot>,
        effects: &mut Effects<Message>,
    ) {
        match message {
            Message::Finished(revision, result) => {
                if self.active.as_ref().map(|r| r.revision) != Some(revision) {
                    return;
                }
                let request = self.active.take().unwrap();
                match result {
                    Ok(()) => {
                        if self.failure.as_ref().is_some_and(|f| {
                            f.revision.0 <= revision.0 && same_control(&f.command, &request.command)
                        }) {
                            self.failure = None;
                        }
                    }
                    Err(error) => {
                        if self
                            .failure
                            .as_ref()
                            .is_none_or(|f| f.revision.0 <= revision.0)
                        {
                            self.failure = Some(Failure {
                                revision,
                                command: request.command,
                                reason: error.to_string(),
                            });
                        }
                    }
                }
                self.pending.retain(|r| valid(&r.command, snapshot));
                self.start(effects);
            }
            Message::WorkerFailed(reason) => {
                let Some(active) = &self.active else { return };
                self.update(
                    Message::Finished(active.revision, Err(DomainError::Failed(reason))),
                    snapshot,
                    effects,
                );
            }
            Message::Retry => {
                if let Some(command) = self.failure.as_ref().map(|f| f.command.clone()) {
                    self.request(command, snapshot, effects);
                }
            }
            Message::DismissFailure => self.failure = None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Adapter {
        calls: Mutex<Vec<AudioCommand>>,
        results: Mutex<VecDeque<Result<()>>>,
    }
    impl AudioPort for Adapter {
        fn watch(&self, _: &mut dyn FnMut(Result<AudioSnapshot>), _: &dyn StopSignal) {}
        fn control(&self, command: AudioCommand) -> Result<()> {
            self.calls.lock().unwrap().push(command);
            self.results.lock().unwrap().pop_front().unwrap_or(Ok(()))
        }
    }
    fn volume(value: f64) -> AudioCommand {
        AudioCommand::SetVolume(
            AudioObjectId::new(1).unwrap(),
            AudioVolume::new(value).unwrap(),
        )
    }
    fn request(commands: &mut Commands, command: AudioCommand) -> Effects<Message> {
        let mut effects = Effects::default();
        commands.request(command, Some(&super::super::fixture()), &mut effects);
        effects
    }
    fn finish(commands: &mut Commands, mut effects: Effects<Message>) -> Effects<Message> {
        assert_eq!(effects.tasks.len(), 1);
        let task = effects.tasks.pop().unwrap();
        assert_eq!(task.queue, "audio");
        let mut next = Effects::default();
        commands.update(task.run(), Some(&super::super::fixture()), &mut next);
        next
    }
    #[test]
    fn drag_bursts_retain_latest_volume_without_flooding_runtime_or_reordering_barriers() {
        let adapter = Arc::new(Adapter::default());
        let mut commands = Commands::new(adapter.clone());
        let first = request(&mut commands, volume(0.1));
        for i in 0..1_000 {
            assert!(
                request(&mut commands, volume(f64::from(i) / 1000.))
                    .tasks
                    .is_empty()
            );
        }
        assert_eq!(commands.pending_count(), 1);
        assert!(
            request(&mut commands, AudioCommand::ToggleMute)
                .tasks
                .is_empty()
        );
        assert!(request(&mut commands, volume(0.5)).tasks.is_empty());
        assert_eq!(commands.pending_count(), 3);
        let mut work = first;
        while !work.tasks.is_empty() {
            work = finish(&mut commands, work);
        }
        assert_eq!(
            *adapter.calls.lock().unwrap(),
            [
                volume(0.1),
                volume(0.999),
                AudioCommand::ToggleMute,
                volume(0.5)
            ]
        );
        assert!(!commands.busy());
    }
    #[test]
    fn saturation_is_explicit_and_unrelated_success_cannot_clear_the_failure() {
        let adapter = Arc::new(Adapter::default());
        let mut commands = Commands::new(adapter);
        let first = request(&mut commands, AudioCommand::Raise);
        for _ in 0..MAX_PENDING {
            request(&mut commands, AudioCommand::Raise);
        }
        assert!(request(&mut commands, volume(0.7)).tasks.is_empty());
        assert_eq!(commands.pending_count(), MAX_PENDING);
        let mut work = first;
        while !work.tasks.is_empty() {
            work = finish(&mut commands, work);
        }
        assert!(commands.error().unwrap().contains("busy"));
        commands.update(Message::Retry, Some(&super::super::fixture()), &mut work);
        assert!(commands.error().is_some());
        assert!(finish(&mut commands, work).tasks.is_empty());
        assert!(commands.error().is_none());
    }
    #[test]
    fn disconnect_discards_pending_intent_and_stale_completions_do_not_finish_new_requests() {
        let adapter = Arc::new(Adapter::default());
        let mut commands = Commands::new(adapter.clone());
        let mut first = request(&mut commands, volume(0.1));
        let old = commands.active.as_ref().unwrap().revision;
        request(&mut commands, volume(0.9));
        commands.reconcile(None);
        assert_eq!(commands.pending_count(), 0);
        let mut next = Effects::default();
        commands.update(first.tasks.pop().unwrap().run(), None, &mut next);
        assert!(next.tasks.is_empty());
        assert!(!commands.busy());
        let new = request(&mut commands, volume(0.5));
        commands.update(
            Message::Finished(old, Ok(())),
            Some(&super::super::fixture()),
            &mut next,
        );
        assert!(commands.busy());
        assert!(next.tasks.is_empty());
        finish(&mut commands, new);
        assert_eq!(*adapter.calls.lock().unwrap(), [volume(0.1), volume(0.5)]);
    }
    #[test]
    fn stale_controls_are_rejected_and_pending_controls_pruned_after_hotplug() {
        let adapter = Arc::new(Adapter::default());
        let mut commands = Commands::new(adapter.clone());
        let first = request(&mut commands, volume(0.1));
        request(&mut commands, volume(0.9));
        commands.reconcile(Some(&AudioSnapshot::default()));
        assert_eq!(commands.pending_count(), 0);
        finish(&mut commands, first);
        let mut effects = Effects::default();
        commands.request(volume(0.4), Some(&AudioSnapshot::default()), &mut effects);
        assert!(effects.tasks.is_empty());
        assert!(commands.error().unwrap().contains("no longer"));
        commands.request(
            AudioCommand::Route {
                stream: AudioObjectId::new(2).unwrap(),
                device: AudioObjectId::new(1).unwrap(),
            },
            Some(&super::super::fixture()),
            &mut effects,
        );
        assert!(effects.tasks.is_empty());
        assert_eq!(adapter.calls.lock().unwrap().len(), 1);
    }
    #[test]
    fn worker_failure_retries_without_claiming_observed_success() {
        let adapter = Arc::new(Adapter::default());
        let mut commands = Commands::new(adapter.clone());
        drop(request(&mut commands, volume(0.1)));
        let mut effects = Effects::default();
        commands.update(
            Message::WorkerFailed("admission rejected".into()),
            Some(&super::super::fixture()),
            &mut effects,
        );
        assert!(!commands.busy());
        assert!(effects.tasks.is_empty());
        assert!(commands.error().is_some());
        commands.update(Message::Retry, Some(&super::super::fixture()), &mut effects);
        finish(&mut commands, effects);
        assert_eq!(*adapter.calls.lock().unwrap(), [volume(0.1)]);
        assert!(commands.error().is_none());
    }
    #[test]
    fn older_success_cannot_erase_a_newer_failure_for_the_same_control() {
        let adapter = Arc::new(Adapter::default());
        let mut commands = Commands::new(adapter);
        let first = request(&mut commands, volume(0.1));
        for _ in 0..MAX_PENDING {
            request(&mut commands, AudioCommand::Raise);
        }
        request(&mut commands, volume(0.9));
        let mut work = finish(&mut commands, first);
        assert!(commands.error().unwrap().contains("busy"));
        while !work.tasks.is_empty() {
            work = finish(&mut commands, work);
        }
        assert!(commands.error().unwrap().contains("busy"));
        commands.update(Message::Retry, Some(&super::super::fixture()), &mut work);
        finish(&mut commands, work);
        assert!(commands.error().is_none());
    }
    #[test]
    fn reconnected_device_is_not_an_implicit_retry_target_and_failure_can_be_dismissed() {
        let adapter = Arc::new(Adapter::default());
        let mut commands = Commands::new(adapter.clone());
        drop(request(&mut commands, volume(0.1)));
        commands.update(
            Message::WorkerFailed("connection reset".into()),
            Some(&super::super::fixture()),
            &mut Effects::default(),
        );
        assert!(commands.can_retry(Some(&super::super::fixture())));
        assert!(!commands.can_retry(None));
        let mut reconnected = super::super::fixture();
        let id = AudioObjectId::new(2).unwrap();
        reconnected.devices[0].id = id;
        reconnected.default_output = Some(id);
        assert!(!commands.can_retry(Some(&reconnected)));
        let mut effects = Effects::default();
        commands.update(Message::Retry, Some(&reconnected), &mut effects);
        assert!(effects.tasks.is_empty());
        assert!(adapter.calls.lock().unwrap().is_empty());
        commands.update(Message::DismissFailure, Some(&reconnected), &mut effects);
        assert!(commands.error().is_none());
        commands.request(
            AudioCommand::SetVolume(id, AudioVolume::new(0.4).unwrap()),
            Some(&reconnected),
            &mut effects,
        );
        let result = effects.tasks.pop().unwrap().run();
        commands.update(result, Some(&reconnected), &mut effects);
        assert!(!commands.busy());
        assert_eq!(
            *adapter.calls.lock().unwrap(),
            [AudioCommand::SetVolume(id, AudioVolume::new(0.4).unwrap())]
        );
    }
    #[test]
    fn dismissing_failure_does_not_cancel_accepted_or_pending_commands() {
        let adapter = Arc::new(Adapter::default());
        let mut commands = Commands::new(adapter.clone());
        drop(request(&mut commands, volume(0.1)));
        commands.update(
            Message::WorkerFailed("deadline".into()),
            Some(&super::super::fixture()),
            &mut Effects::default(),
        );
        let first = request(&mut commands, volume(0.5));
        request(&mut commands, AudioCommand::ToggleMute);
        commands.update(
            Message::DismissFailure,
            Some(&super::super::fixture()),
            &mut Effects::default(),
        );
        assert!(commands.error().is_none());
        assert!(commands.busy());
        assert_eq!(commands.pending_count(), 1);
        let second = finish(&mut commands, first);
        finish(&mut commands, second);
        assert_eq!(
            *adapter.calls.lock().unwrap(),
            [volume(0.5), AudioCommand::ToggleMute]
        );
    }
}
