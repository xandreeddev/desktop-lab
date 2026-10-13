//! Ordered preference intent. One accepted write finishes before the latest
//! pending snapshot starts. Adapter calls run through the existing effect queue.
use lucent_api::Effects;
use lucent_domain::{DesktopSettings, DomainError, Result, SettingsPort};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Revision(u64);

#[derive(Clone)]
pub enum Message {
    Finished(Revision, Result<()>),
    WorkerFailed(String),
    Retry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Unavailable,
    Saving,
    Retrying,
    Saved,
    Failed,
}
impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Saving => "saving",
            Self::Retrying => "retrying",
            Self::Saved => "saved",
            Self::Failed => "failed",
        }
    }
}

struct Snapshot {
    revision: Revision,
    settings: DesktopSettings,
}
pub struct Persistence {
    adapter: Arc<dyn SettingsPort>,
    latest: Option<Arc<Snapshot>>,
    active: Option<Revision>,
    acknowledged: Option<Revision>,
    error: String,
}
impl Persistence {
    pub fn new(adapter: Arc<dyn SettingsPort>) -> Self {
        Self {
            adapter,
            latest: None,
            active: None,
            acknowledged: None,
            error: String::new(),
        }
    }
    pub fn error(&self) -> &str {
        &self.error
    }
    pub fn status(&self, writable: bool) -> Status {
        if !writable {
            Status::Unavailable
        } else if !self.error.is_empty() {
            if self.active.is_some() {
                Status::Retrying
            } else {
                Status::Failed
            }
        } else if self.active.is_some() {
            Status::Saving
        } else {
            Status::Saved
        }
    }
    pub fn request(&mut self, settings: &DesktopSettings, effects: &mut Effects<Message>) {
        let revision = self
            .latest
            .as_ref()
            .map_or(0, |snapshot| snapshot.revision.0);
        let Some(next) = revision.checked_add(1) else {
            self.error = "Preference revision limit reached; restart before editing again".into();
            Self::redraw(effects);
            return;
        };
        self.latest = Some(Arc::new(Snapshot {
            revision: Revision(next),
            settings: settings.clone(),
        }));
        // An in-flight write is never cancelled or overtaken. Pending snapshots
        // replace each other without adding tasks to the runtime queue.
        self.start(effects);
        Self::redraw(effects);
    }
    fn start(&mut self, effects: &mut Effects<Message>) {
        if self.active.is_some() {
            return;
        }
        let Some(snapshot) = self.latest.clone() else {
            return;
        };
        if self.acknowledged == Some(snapshot.revision) {
            return;
        }
        self.active = Some(snapshot.revision);
        let adapter = self.adapter.clone();
        effects.task_on(crate::work::SETTINGS, move || {
            Message::Finished(snapshot.revision, adapter.save(&snapshot.settings))
        });
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Finished(revision, result) => {
                if self.active != Some(revision) {
                    return;
                }
                self.active = None;
                match result {
                    Ok(()) => {
                        self.acknowledged = Some(revision);
                        self.error.clear();
                    }
                    Err(error) => self.error = error.to_string(),
                }
                // An error with no newer intent waits for an explicit retry.
                // This avoids a disk-failure loop while preserving edited data.
                if self
                    .latest
                    .as_ref()
                    .is_some_and(|latest| latest.revision != revision)
                {
                    self.start(effects);
                }
            }
            Message::WorkerFailed(reason) => {
                // This is the sole producer for the settings queue, with one
                // outstanding task. Admission failure/panic belongs to that task.
                let Some(revision) = self.active else {
                    return;
                };
                self.update(
                    Message::Finished(revision, Err(DomainError::Failed(reason))),
                    effects,
                );
                return;
            }
            Message::Retry => self.start(effects),
        }
        Self::redraw(effects);
    }
    fn redraw(effects: &mut Effects<Message>) {
        effects.redraw("widgets");
        effects.redraw("dock");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};
    #[derive(Default)]
    struct Adapter {
        writes: Mutex<Vec<String>>,
        results: Mutex<VecDeque<Result<()>>>,
    }
    impl SettingsPort for Adapter {
        fn load(&self) -> Result<DesktopSettings> {
            Ok(DesktopSettings::default())
        }
        fn save(&self, settings: &DesktopSettings) -> Result<()> {
            self.writes.lock().unwrap().push(settings.notes.clone());
            self.results.lock().unwrap().pop_front().unwrap_or(Ok(()))
        }
    }
    fn request(store: &mut Persistence, text: &str) -> Effects<Message> {
        let mut effects = Effects::default();
        store.request(
            &DesktopSettings {
                notes: text.into(),
                ..Default::default()
            },
            &mut effects,
        );
        effects
    }
    fn finish(store: &mut Persistence, mut effects: Effects<Message>) -> Effects<Message> {
        assert_eq!(effects.tasks.len(), 1);
        let task = effects.tasks.pop().unwrap();
        assert_eq!(task.queue, "settings");
        let mut followup = Effects::default();
        store.update(task.run(), &mut followup);
        followup
    }
    #[test]
    fn edit_bursts_coalesce_while_an_accepted_write_finishes() {
        let adapter = Arc::new(Adapter::default());
        let mut store = Persistence::new(adapter.clone());
        let first = request(&mut store, "first");
        for index in 0..1000 {
            assert!(request(&mut store, &index.to_string()).tasks.is_empty());
        }
        assert_eq!(store.status(true), Status::Saving);
        assert_eq!(store.status(false), Status::Unavailable);
        let last = finish(&mut store, first);
        assert_eq!(store.status(true), Status::Saving);
        assert!(finish(&mut store, last).tasks.is_empty());
        assert_eq!(*adapter.writes.lock().unwrap(), ["first", "999"]);
        assert_eq!(store.status(true), Status::Saved);
    }
    #[test]
    fn failed_write_retains_the_latest_intent_for_retry_without_spinning() {
        let adapter = Arc::new(Adapter::default());
        adapter
            .results
            .lock()
            .unwrap()
            .push_back(Err(DomainError::Failed("disk full".into())));
        let mut store = Persistence::new(adapter.clone());
        let first = request(&mut store, "keep me");
        assert!(finish(&mut store, first).tasks.is_empty());
        assert_eq!(store.status(true), Status::Failed);
        assert!(store.error().contains("disk full"));
        let mut retry = Effects::default();
        store.update(Message::Retry, &mut retry);
        // The failure stays visible while the retry is in flight.
        assert_eq!(store.status(true), Status::Retrying);
        assert!(store.error().contains("disk full"));
        assert!(finish(&mut store, retry).tasks.is_empty());
        assert_eq!(*adapter.writes.lock().unwrap(), ["keep me", "keep me"]);
        assert_eq!(store.status(true), Status::Saved);
        let mut duplicate = Effects::default();
        store.update(Message::Retry, &mut duplicate);
        assert!(duplicate.tasks.is_empty());
    }
    #[test]
    fn newer_edits_follow_a_failure_and_old_completions_cannot_clear_it() {
        let adapter = Arc::new(Adapter::default());
        adapter.results.lock().unwrap().extend([
            Err(DomainError::Failed("first failed".into())),
            Err(DomainError::Failed("latest failed".into())),
        ]);
        let mut store = Persistence::new(adapter.clone());
        let first = request(&mut store, "first");
        let old = store.active.unwrap();
        assert!(request(&mut store, "last").tasks.is_empty());
        let last = finish(&mut store, first);
        assert!(finish(&mut store, last).tasks.is_empty());
        let mut effects = Effects::default();
        store.update(Message::Finished(old, Ok(())), &mut effects);
        assert!(store.error().contains("latest failed"));
        assert!(effects.tasks.is_empty());
        let next = request(&mut store, "newer");
        assert!(finish(&mut store, next).tasks.is_empty());
        assert!(store.error().is_empty());
        assert_eq!(*adapter.writes.lock().unwrap(), ["first", "last", "newer"]);
    }
    #[test]
    fn rejected_work_can_retry_and_never_claims_a_saved_revision() {
        let adapter = Arc::new(Adapter::default());
        let mut store = Persistence::new(adapter.clone());
        let rejected = request(&mut store, "retained");
        drop(rejected); // Runtime admission rejected the task before execution.
        let mut effects = Effects::default();
        store.update(
            Message::WorkerFailed("queue rejected work".into()),
            &mut effects,
        );
        assert!(adapter.writes.lock().unwrap().is_empty());
        assert!(effects.tasks.is_empty());
        assert_eq!(store.status(true), Status::Failed);
        store.update(Message::Retry, &mut effects);
        assert!(finish(&mut store, effects).tasks.is_empty());
        assert_eq!(store.status(true), Status::Saved);
        assert_eq!(*adapter.writes.lock().unwrap(), ["retained"]);
    }
}
