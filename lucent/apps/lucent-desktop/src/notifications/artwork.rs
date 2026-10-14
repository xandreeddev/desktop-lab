//! Current visible demand, one active batch and one latest pending batch.
//! IO/mip preparation belongs to the injected presentation adapter.
use super::{Center, Message};
use crate::ports::{AssetPort, Images, NotificationAsset};
use lucent_api::{Effects, ImageData, Motion};
use lucent_domain::{Notification, NotificationImage, NotificationImageSource, NotificationTarget};
use lucent_usecases::notifications::body::BodyBlock;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(super) struct Artwork {
    adapter: Arc<dyn AssetPort>,
    revision: u64,
    active: Option<u64>,
    pending: Option<Vec<NotificationAsset>>,
    wanted: BTreeSet<String>,
    images: BTreeMap<String, Arc<ImageData>>,
    reveals: BTreeMap<String, Motion>,
}
const PREPARED_BYTES: usize = 8 * 1024 * 1024;
impl Artwork {
    pub fn new(adapter: Arc<dyn AssetPort>) -> Self {
        Self {
            adapter,
            revision: 0,
            active: None,
            pending: None,
            wanted: Default::default(),
            images: Default::default(),
            reveals: Default::default(),
        }
    }
    pub fn image(&self, key: &str) -> Option<&Arc<ImageData>> {
        self.images.get(key)
    }
    pub fn opacity(&self, key: &str, now: f64) -> f32 {
        self.reveals.get(key).map_or(1., |motion| motion.value(now))
    }
    pub fn animating(&self, now: f64) -> bool {
        self.reveals.values().any(|motion| motion.active(now))
    }
    pub fn inspect(&self) -> serde_json::Value {
        let mut identities = BTreeSet::new();
        let bytes: usize = self
            .images
            .values()
            .filter(|image| identities.insert(image.cache_id()))
            .map(|image| image.bytes())
            .sum();
        serde_json::json!({"preparing":self.active.is_some(),"pending":self.pending.is_some(),"wanted":self.wanted.len(),"prepared":self.images.len(),"prepared_bytes":bytes,"prepared_budget_bytes":PREPARED_BYTES})
    }
    pub fn loading(&self) -> bool {
        self.active.is_some() || self.pending.is_some()
    }
    pub fn failed_for(&self, target: NotificationTarget) -> bool {
        let prefix = key(target, "");
        !self.loading()
            && self
                .wanted
                .iter()
                .any(|key| key.starts_with(&prefix) && !self.images.contains_key(key))
    }
    pub fn complete(&mut self, revision: u64, images: Images, effects: &mut Effects<Message>) {
        if self.active != Some(revision) {
            return;
        }
        self.active = None;
        // A retained key includes the immutable received revision and role.
        // Useful overlap can survive supersession without restoring old demand.
        let mut bytes = self
            .images
            .values()
            .map(|image| image.bytes())
            .fold(0usize, usize::saturating_add);
        for (key, image) in images {
            let size = image.bytes();
            if self.wanted.contains(&key)
                && !self.images.contains_key(&key)
                && image.width() <= 400
                && image.height() <= 200
                && size <= PREPARED_BYTES.saturating_sub(bytes)
            {
                bytes += size;
                let mut reveal = Motion::fixed(0.);
                reveal.target(
                    1.,
                    effects.now,
                    lucent_design::motion::ENTER,
                    lucent_design::motion::DECELERATE,
                );
                self.reveals.insert(key.clone(), reveal);
                self.images.insert(key, image);
            }
        }
        if let Some(requests) = self.pending.take() {
            self.start(requests, effects);
        }
    }
    fn start(&mut self, requests: Vec<NotificationAsset>, effects: &mut Effects<Message>) {
        let requests: Vec<_> = requests
            .into_iter()
            .filter(|request| {
                self.wanted.contains(&request.key) && !self.images.contains_key(&request.key)
            })
            .collect();
        if requests.is_empty() {
            return;
        }
        let revision = self.revision;
        self.active = Some(revision);
        let adapter = self.adapter.clone();
        effects.task_on_with_failure(
            crate::work::ASSETS,
            move || Message::Artwork(revision, adapter.notification_images(&requests)),
            move |_| Message::Artwork(revision, vec![]),
        );
    }
    pub fn demand(
        &mut self,
        requests: Vec<NotificationAsset>,
        retry: bool,
        effects: &mut Effects<Message>,
    ) {
        let wanted: BTreeSet<_> = requests.iter().map(|request| request.key.clone()).collect();
        if !retry && wanted == self.wanted {
            return;
        }
        self.wanted = wanted;
        self.reveals.retain(|key, _| self.wanted.contains(key));
        self.images.retain(|key, _| self.wanted.contains(key));
        let Some(revision) = self.revision.checked_add(1) else {
            return;
        };
        self.revision = revision;
        if self.active.is_some() {
            self.pending = Some(requests);
        } else {
            self.start(requests, effects);
        }
    }
}
pub(super) fn key(target: NotificationTarget, role: &str) -> String {
    format!("notification:{}:{}:{role}", target.id(), target.revision())
}
impl Center {
    fn artwork_requests(&self) -> Vec<NotificationAsset> {
        let notes: Vec<&Notification> = if self.history_open || self.closing {
            self.groups()
                .iter()
                .find(|group| self.selected_group.as_ref() == Some(&group.key))
                .and_then(|group| {
                    group
                        .entries
                        .get(self.page.min(group.entries.len().saturating_sub(1)))
                        .map(|(note, _)| *note)
                })
                .into_iter()
                .collect()
        } else {
            self.snapshot
                .active
                .iter()
                .filter(|note| !self.snapshot.do_not_disturb || note.critical)
                .take(lucent_design::component::notification::TOAST_LIMIT as usize)
                .collect()
        };
        let mut requests = vec![];
        for note in notes {
            let Some(target) = note.target() else {
                continue;
            };
            if let Some(image) = &note.image {
                requests.push(NotificationAsset {
                    key: key(target, "image"),
                    image: image.clone(),
                });
            }
            if let Some(body) = self.bodies.get(&target) {
                for (index, block) in body.blocks.iter().enumerate() {
                    if let BodyBlock::Image { source, .. } = block
                        && let Ok(source) = NotificationImageSource::new(source)
                        && source.local()
                    {
                        requests.push(NotificationAsset {
                            key: key(target, &format!("body-{index}")),
                            image: NotificationImage::Source(source),
                        });
                    }
                }
            }
        }
        requests
    }
    pub(super) fn refresh_artwork(&mut self, retry: bool, effects: &mut Effects<Message>) {
        self.artwork.demand(self.artwork_requests(), retry, effects);
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use lucent_domain::{Application, DomainError, NotificationPixels, Result};
    #[derive(Default)]
    struct Adapter {
        calls: std::sync::Mutex<Vec<Vec<String>>>,
    }
    impl AssetPort for Adapter {
        fn initial(&self) -> BTreeMap<String, Arc<ImageData>> {
            BTreeMap::new()
        }
        fn application_icons(&self, _: &[Application]) -> Images {
            vec![]
        }
        fn thumbnail(&self, _: &str) -> Result<Arc<ImageData>> {
            Err(DomainError::Unavailable("fixture".into()))
        }
        fn preview(&self, image: &ImageData) -> Arc<ImageData> {
            Arc::new(image.clone())
        }
        fn background(
            &self,
            _: &str,
            _: Option<crate::ports::BackgroundTarget>,
        ) -> Result<Arc<ImageData>> {
            Err(DomainError::Unavailable("fixture".into()))
        }
        fn notification_images(&self, requests: &[NotificationAsset]) -> Images {
            self.calls
                .lock()
                .unwrap()
                .push(requests.iter().map(|request| request.key.clone()).collect());
            requests
                .iter()
                .filter_map(|request| match &request.image {
                    NotificationImage::Pixels(pixels) => Some((
                        request.key.clone(),
                        Arc::new(
                            ImageData::from_rgba(
                                &request.key,
                                pixels.width(),
                                pixels.height(),
                                pixels.rgba().to_vec(),
                            )
                            .unwrap(),
                        ),
                    )),
                    _ => None,
                })
                .collect()
        }
    }
    pub(crate) fn adapter() -> Arc<dyn AssetPort> {
        Arc::new(Adapter::default())
    }
    fn request(key: &str) -> NotificationAsset {
        NotificationAsset {
            key: key.into(),
            image: NotificationImage::Pixels(NotificationPixels::new(1, 1, vec![255; 4]).unwrap()),
        }
    }
    fn complete(artwork: &mut Artwork, effects: &mut Effects<Message>) {
        let Message::Artwork(revision, images) = effects.tasks.remove(0).run() else {
            panic!()
        };
        artwork.complete(revision, images, effects);
    }
    #[test]
    fn latest_demand_coalesces_and_retirement_cannot_restore_old_pixels() {
        let adapter = Arc::new(Adapter::default());
        let mut artwork = Artwork::new(adapter.clone());
        let mut effects = Effects::default();
        artwork.demand(vec![request("first")], false, &mut effects);
        for index in 0..1000 {
            artwork.demand(vec![request(&format!("last-{index}"))], false, &mut effects);
        }
        assert_eq!(effects.tasks.len(), 1);
        assert_eq!(artwork.pending.as_ref().unwrap().len(), 1);
        complete(&mut artwork, &mut effects);
        assert!(artwork.image("first").is_none());
        assert_eq!(effects.tasks.len(), 1);
        complete(&mut artwork, &mut effects);
        assert!(artwork.image("last-999").is_some());
        assert_eq!(artwork.opacity("last-999", 0.), 0.);
        assert!(artwork.animating(0.));
        let halfway = artwork.opacity("last-999", lucent_design::motion::ENTER / 2.);
        assert!(halfway > 0. && halfway < 1.);
        assert!(!artwork.animating(10.));
        assert_eq!(adapter.calls.lock().unwrap().len(), 2);
        artwork.demand(vec![request("retiring")], false, &mut effects);
        artwork.demand(vec![], false, &mut effects);
        complete(&mut artwork, &mut effects);
        assert!(artwork.images.is_empty());
        assert!(effects.tasks.is_empty());
        assert!(!artwork.loading());
    }
    #[test]
    fn unavailable_assets_wait_for_explicit_retry_and_duplicate_completions_do_nothing() {
        let adapter = Arc::new(Adapter::default());
        let mut artwork = Artwork::new(adapter);
        let mut effects = Effects::default();
        let target = NotificationTarget::new(1, 1).unwrap();
        let key = key(target, "image");
        let request = NotificationAsset {
            key: key.clone(),
            image: NotificationImage::Source(
                NotificationImageSource::new("missing-fixture").unwrap(),
            ),
        };
        artwork.demand(vec![request.clone()], false, &mut effects);
        complete(&mut artwork, &mut effects);
        assert!(artwork.failed_for(target));
        artwork.demand(vec![request.clone()], false, &mut effects);
        assert!(effects.tasks.is_empty());
        artwork.complete(
            1,
            vec![(
                key,
                Arc::new(ImageData::from_rgba("late", 1, 1, vec![255; 4]).unwrap()),
            )],
            &mut effects,
        );
        assert!(artwork.failed_for(target));
        artwork.demand(vec![request], true, &mut effects);
        assert_eq!(effects.tasks.len(), 1);
    }
}
