//! Presentation-owned preview lifetime. Views borrow pixels; a single worker
//! prepares missing demand. No filesystem, decoder or desktop-root access here.
use crate::ports::{AssetPort, PREVIEW_DIMENSION};
use lucent_api::{Effects, ImageData};
use lucent_domain::{DomainError, Result};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

// Resource policy, not styling: visible demand is bounded separately from reuse.
pub const MAX_ENTRIES: usize = 16;
const REUSE_BYTES: usize = 64 * 1024 * 1024;
const CLOSED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub key: String,
    pub path: String,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Ticket(u64);
#[derive(Clone)]
pub enum Message {
    Finished(Ticket, Result<Arc<ImageData>>),
    Retry,
}
struct Active {
    ticket: Ticket,
    request: Request,
}
pub struct Previews {
    adapter: Arc<dyn AssetPort>,
    images: BTreeMap<String, Arc<ImageData>>,
    order: VecDeque<String>,
    wanted: Vec<Request>,
    failures: BTreeMap<String, String>,
    active: Option<Active>,
    next: u64,
}
/// Read-only prepared artwork; no adapter or scheduling capability reaches views.
#[derive(Clone, Copy)]
pub struct View<'a> {
    images: &'a BTreeMap<String, Arc<ImageData>>,
    failures: &'a BTreeMap<String, String>,
}
impl View<'_> {
    pub fn get(&self, key: &str) -> Option<&Arc<ImageData>> {
        self.images.get(key)
    }
    pub fn error(&self, key: &str) -> Option<&str> {
        self.failures.get(key).map(String::as_str)
    }
}
impl Previews {
    pub fn new(adapter: Arc<dyn AssetPort>) -> Self {
        Self {
            adapter,
            images: BTreeMap::new(),
            order: VecDeque::new(),
            wanted: vec![],
            failures: BTreeMap::new(),
            active: None,
            next: 0,
        }
    }
    pub fn view(&self) -> View<'_> {
        View {
            images: &self.images,
            failures: &self.failures,
        }
    }
    pub fn images(&self) -> impl Iterator<Item = &Arc<ImageData>> {
        self.images.values()
    }
    fn touch(&mut self, key: &str) {
        self.order.retain(|entry| entry != key);
        self.order.push_back(key.into());
    }
    /// Most important requests first. Bound and deduplicate caller demand.
    pub fn demand(
        &mut self,
        wanted: impl IntoIterator<Item = Request>,
        effects: &mut Effects<Message>,
    ) {
        let mut seen = BTreeSet::new();
        self.wanted = wanted
            .into_iter()
            .filter(|r| seen.insert(r.key.clone()))
            .take(MAX_ENTRIES)
            .collect();
        self.failures
            .retain(|key, _| self.wanted.iter().any(|r| &r.key == key));
        for key in self
            .wanted
            .iter()
            .map(|r| r.key.clone())
            .collect::<Vec<_>>()
        {
            if self.images.contains_key(&key) {
                self.touch(&key);
            }
        }
        self.trim();
        self.start(effects);
    }
    /// Accepted wallpaper transactions already have a validated prepared preview.
    /// A late speculative decode must not overwrite it.
    pub fn insert(&mut self, key: String, image: Arc<ImageData>) -> Result<()> {
        if image.width().max(image.height()) > PREVIEW_DIMENSION {
            return Err(DomainError::Invalid(
                "Preview exceeds the presentation dimensions".into(),
            ));
        }
        self.failures.remove(&key);
        self.touch(&key);
        self.images.insert(key, image);
        self.trim();
        Ok(())
    }
    fn trim(&mut self) {
        let budget = if self.wanted.is_empty() {
            CLOSED_BYTES
        } else {
            REUSE_BYTES
        };
        while self.images.len() > MAX_ENTRIES
            || self.images.values().map(|i| i.bytes()).sum::<usize>() > budget
        {
            let Some(index) = self
                .order
                .iter()
                .position(|key| !self.wanted.iter().any(|r| &r.key == key))
            else {
                break;
            };
            let key = self.order.remove(index).unwrap();
            self.images.remove(&key);
        }
    }
    fn start(&mut self, effects: &mut Effects<Message>) {
        if self.active.is_some() {
            return;
        }
        let Some(request) = self
            .wanted
            .iter()
            .find(|r| !self.images.contains_key(&r.key) && !self.failures.contains_key(&r.key))
            .cloned()
        else {
            return;
        };
        let Some(next) = self.next.checked_add(1) else {
            self.failures.insert(
                request.key,
                "Preview request limit reached; restart the desktop".into(),
            );
            return;
        };
        self.next = next;
        let ticket = Ticket(next);
        self.active = Some(Active {
            ticket,
            request: request.clone(),
        });
        let adapter = self.adapter.clone();
        effects.task_on_with_failure(
            crate::work::ASSETS,
            move || Message::Finished(ticket, adapter.thumbnail(&request.path)),
            move |failure| Message::Finished(ticket, Err(DomainError::Failed(failure.to_string()))),
        );
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Finished(ticket, result) => {
                if self.active.as_ref().is_none_or(|a| a.ticket != ticket) {
                    return;
                }
                let request = self.active.take().unwrap().request;
                // Drop obsolete work. Only one image can be in flight, even if
                // navigation changes hundreds of times while decoding.
                if self.wanted.iter().any(|r| r == &request)
                    && !self.images.contains_key(&request.key)
                {
                    let result = result.and_then(|image| self.insert(request.key.clone(), image));
                    if let Err(error) = result {
                        self.failures.insert(request.key, error.to_string());
                    }
                    effects.redraw("dock");
                }
            }
            Message::Retry => self.failures.clear(),
        }
        self.start(effects);
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"entries":self.images.len(), "bytes":usage(self.images()).bytes,
            "wanted":self.wanted.len(), "active":self.active.is_some(), "failed":self.failures.len(),
            "wanted_ready": self.wanted.iter().filter(|r| self.images.contains_key(&r.key)).count(),
            "reuse_budget_bytes":REUSE_BYTES, "closed_budget_bytes":CLOSED_BYTES, "entry_limit":MAX_ENTRIES,
            "pinned_bytes":usage(self.wanted.iter().filter_map(|r| self.images.get(&r.key))).bytes})
    }
}

#[derive(serde::Serialize)]
pub struct Usage {
    pub unique_images: usize,
    pub bytes: usize,
}
/// Count immutable pixel storage once, even when multiple keys/owners share it.
/// Excludes in-flight work, scenes, decoder scratch and GPU/driver allocations.
pub fn usage<'a>(images: impl IntoIterator<Item = &'a Arc<ImageData>>) -> Usage {
    let mut seen = BTreeSet::new();
    let bytes = images
        .into_iter()
        .filter(|i| seen.insert(i.cache_id()))
        .map(|i| i.bytes())
        .sum();
    Usage {
        unique_images: seen.len(),
        bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_domain::Application;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Adapter {
        calls: Mutex<Vec<String>>,
    }
    fn pixel(key: &str) -> Arc<ImageData> {
        Arc::new(ImageData::from_rgba(key, 1, 1, vec![255; 4]).unwrap())
    }
    impl AssetPort for Adapter {
        fn initial(&self) -> BTreeMap<String, Arc<ImageData>> {
            BTreeMap::new()
        }
        fn application_icons(&self, _: &[Application]) -> crate::ports::Images {
            vec![]
        }
        fn thumbnail(&self, path: &str) -> Result<Arc<ImageData>> {
            self.calls.lock().unwrap().push(path.into());
            if path == "bad" {
                Err(DomainError::Failed("Decode fixture".into()))
            } else {
                Ok(pixel(path))
            }
        }
        fn preview(&self, image: &ImageData) -> Arc<ImageData> {
            Arc::new(image.clone())
        }
        fn background(
            &self,
            path: &str,
            _: Option<crate::ports::BackgroundTarget>,
        ) -> Result<Arc<ImageData>> {
            self.thumbnail(path)
        }
    }
    fn request(key: &str) -> Request {
        Request {
            key: key.into(),
            path: key.into(),
        }
    }
    fn finish(previews: &mut Previews, effects: &mut Effects<Message>) {
        while let Some(task) = effects.tasks.pop() {
            previews.update(task.run(), effects);
        }
    }
    #[test]
    fn rapid_navigation_has_one_task_and_prepares_only_the_latest_demand() {
        let adapter = Arc::new(Adapter::default());
        let mut previews = Previews::new(adapter.clone());
        let mut effects = Effects::default();
        previews.demand([request("first")], &mut effects);
        for n in 0..1000 {
            previews.demand([request(&n.to_string())], &mut effects);
        }
        assert_eq!(effects.tasks.len(), 1);
        finish(&mut previews, &mut effects);
        assert_eq!(*adapter.calls.lock().unwrap(), ["first", "999"]);
        assert!(previews.view().get("first").is_none());
        assert!(previews.view().get("999").is_some());
    }
    #[test]
    fn failures_wait_for_explicit_retry_or_a_new_visit_and_rejected_work_recovers() {
        let mut previews = Previews::new(Arc::new(Adapter::default()));
        let mut effects = Effects::default();
        previews.demand([request("bad"), request("good")], &mut effects);
        finish(&mut previews, &mut effects);
        assert!(previews.view().error("bad").is_some());
        assert!(previews.view().get("good").is_some());
        for _ in 0..10 {
            previews.demand([request("bad")], &mut effects);
        }
        assert!(effects.tasks.is_empty());
        previews.update(Message::Retry, &mut effects);
        assert_eq!(effects.tasks.len(), 1);
        let ticket = previews.active.as_ref().unwrap().ticket;
        effects.tasks.clear();
        previews.update(
            Message::Finished(ticket, Err(DomainError::Failed("decode failed".into()))),
            &mut effects,
        );
        assert_eq!(previews.view().error("bad"), Some("decode failed"));
        assert!(effects.tasks.is_empty());
        previews.demand([], &mut effects);
        previews.demand([request("bad")], &mut effects);
        assert_eq!(effects.tasks.len(), 1);
    }
    #[test]
    fn accepted_transaction_preview_wins_over_an_older_speculative_decode() {
        let mut previews = Previews::new(Arc::new(Adapter::default()));
        let mut effects = Effects::default();
        previews.demand([request("same")], &mut effects);
        let old = effects.tasks.pop().unwrap().run();
        let committed = pixel("committed snapshot");
        previews.insert("same".into(), committed.clone()).unwrap();
        previews.update(old.clone(), &mut effects);
        previews.demand([request("next")], &mut effects);
        previews.update(old, &mut effects); // stale ticket cannot finish newer work
        assert!(previews.active.is_some());
        assert_eq!(effects.tasks.len(), 1);
        assert_eq!(
            previews.view().get("same").unwrap().cache_id(),
            committed.cache_id()
        );
        finish(&mut previews, &mut effects);
        assert!(previews.view().get("next").is_some());
    }
    #[test]
    fn cache_limits_retention_and_pins_wanted_images_then_shrinks_when_closed() {
        let mut previews = Previews::new(Arc::new(Adapter::default()));
        let mut effects = Effects::default();
        let square = Arc::new(
            ImageData::from_rgba("square", 1024, 1024, vec![255; 1024 * 1024 * 4]).unwrap(),
        );
        previews.demand((0..1000).map(|i| request(&i.to_string())), &mut effects);
        assert_eq!(previews.wanted.len(), MAX_ENTRIES);
        // Aliases intentionally share one immutable allocation; conservative
        // admission sums each entry, while reported usage counts storage once.
        for i in 0..MAX_ENTRIES {
            previews.insert(i.to_string(), square.clone()).unwrap();
        }
        assert_eq!(previews.images.len(), MAX_ENTRIES);
        assert!(previews.images.values().map(|i| i.bytes()).sum::<usize>() > REUSE_BYTES);
        assert_eq!(usage(previews.images()).bytes, square.bytes());
        for i in MAX_ENTRIES..1000 {
            previews.insert(i.to_string(), pixel("other")).unwrap();
        }
        assert_eq!(previews.images.len(), MAX_ENTRIES);
        assert!(previews.view().get("0").is_some());
        previews.demand([], &mut effects);
        assert!(previews.images.values().map(|i| i.bytes()).sum::<usize>() <= CLOSED_BYTES);
        finish(&mut previews, &mut effects); // obsolete load cannot repopulate
        assert!(previews.images.len() < MAX_ENTRIES);
        let too_large = Arc::new(
            ImageData::from_rgba("invalid adapter", 1025, 1, vec![255; 1025 * 4]).unwrap(),
        );
        assert!(previews.insert("invalid".into(), too_large).is_err());
        assert!(!previews.images.contains_key("invalid"));
    }
    #[test]
    fn usage_counts_shared_storage_once_but_replacements_separately() {
        let a = pixel("same-name");
        let b = Arc::new((*a).clone());
        let c = pixel("same-name");
        let stats = usage([&a, &b, &c]);
        assert_eq!(stats.unique_images, 2);
        assert_eq!(stats.bytes, 8);
    }
    #[test]
    fn evicting_unwanted_images_releases_their_cpu_storage() {
        let mut previews = Previews::new(Arc::new(Adapter::default()));
        let first = pixel("first");
        let old = Arc::downgrade(&first);
        previews.insert("first".into(), first).unwrap();
        for i in 0..1000 {
            previews.insert(i.to_string(), pixel("new")).unwrap();
        }
        assert_eq!(previews.images.len(), MAX_ENTRIES);
        assert!(old.upgrade().is_none());
    }
}
