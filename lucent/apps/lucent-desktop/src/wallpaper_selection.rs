//! Wallpaper transaction coordinator. Preparation is supersedable; a commit is
//! accepted work and finishes before the latest queued choice starts preparing.
//! No filesystem or decoder implementation belongs in this client state machine.
use crate::ports::AssetPort;
use lucent_api::{Cancellation, Effects, ImageData};
use lucent_domain::*;
use std::sync::Arc;

#[derive(Clone)]
pub struct Choice {
    pub source: Wallpaper,
    pub colors: Option<crate::palette_intent::Revision>,
    pub browser_generation: Option<u64>,
}
#[derive(Clone)]
pub struct Prepared {
    pub asset: WallpaperAsset,
    pub image: Arc<ImageData>,
    pub preview: Arc<ImageData>,
    pub seed: Option<(crate::palette_intent::Revision, Rgb)>,
    pub browser_generation: Option<u64>,
}
#[derive(Clone)]
pub enum Message {
    Initial(Result<Option<Prepared>>),
    Prepared(u64, Result<Prepared>),
    Committed(u64, Prepared, Result<()>),
    Discarded(Result<()>),
}
pub enum Outcome {
    Applied(Prepared),
    Failed {
        browser_generation: Option<u64>,
        error: String,
    },
}
#[derive(PartialEq, Eq)]
enum Phase {
    Idle,
    Preparing(u64),
    Committing(u64),
}

pub struct Selection {
    pub current: Option<WallpaperAsset>,
    pub background: Option<Arc<ImageData>>,
    pub error: String,
    phase: Phase,
    generation: u64,
    preparing_origin: Option<u64>,
    pending: Option<(u64, Choice)>,
    awaiting_download: bool,
    cancellation: Cancellation,
    store: Arc<dyn WallpaperPort>,
    assets: Arc<dyn AssetPort>,
    palettes: Arc<dyn PaletteGenerationPort>,
}
impl Selection {
    pub fn new(
        store: Arc<dyn WallpaperPort>,
        assets: Arc<dyn AssetPort>,
        palettes: Arc<dyn PaletteGenerationPort>,
    ) -> Self {
        Self {
            current: None,
            background: None,
            error: String::new(),
            phase: Phase::Idle,
            generation: 0,
            preparing_origin: None,
            pending: None,
            awaiting_download: false,
            cancellation: Cancellation::default(),
            store,
            assets,
            palettes,
        }
    }
    pub fn source_path(&self) -> &str {
        self.current
            .as_ref()
            .map_or("", |asset| asset.source.path.as_str())
    }
    pub fn asset_path(&self) -> &str {
        self.current
            .as_ref()
            .map_or("", |asset| asset.path.as_str())
    }
    pub fn busy(&self) -> bool {
        self.phase != Phase::Idle || self.awaiting_download
    }

    pub fn load(
        &mut self,
        selected: Result<Option<WallpaperAsset>>,
        effects: &mut Effects<Message>,
    ) {
        let assets = self.assets.clone();
        effects.task_on("appearance", move || {
            Message::Initial(selected.and_then(|selected| {
                selected
                    .map(|asset| {
                        assets.background(&asset.path).map(|image| Prepared {
                            asset,
                            preview: assets.preview(&image),
                            image,
                            seed: None,
                            browser_generation: None,
                        })
                    })
                    .transpose()
            }))
        });
    }
    /// Reserve intent before a network download starts. A late network result
    /// cannot become a new selection after a more recent local/online choice.
    pub fn reserve(&mut self) -> u64 {
        self.cancellation.cancel();
        self.generation += 1;
        self.pending = None;
        self.awaiting_download = true;
        self.error.clear();
        self.generation
    }
    pub fn is_current_request(&self, id: u64) -> bool {
        id == self.generation
    }
    pub fn cancel_reserved(&mut self, id: u64) {
        if id == self.generation {
            self.reserve();
            self.awaiting_download = false;
        }
    }
    pub fn choose(&mut self, choice: Choice, effects: &mut Effects<Message>) {
        let id = self.reserve();
        self.submit(id, choice, effects);
    }
    pub fn submit(&mut self, id: u64, choice: Choice, effects: &mut Effects<Message>) {
        if id != self.generation {
            return;
        }
        self.awaiting_download = false;
        self.pending = Some((id, choice));
        self.start_pending(effects);
    }
    fn start_pending(&mut self, effects: &mut Effects<Message>) {
        if self.phase != Phase::Idle {
            return;
        }
        let Some((generation, choice)) = self.pending.take() else {
            return;
        };
        if generation != self.generation {
            return;
        }
        self.cancellation = Cancellation::default();
        let cancel = self.cancellation.clone();
        let (store, assets, palettes) = (
            self.store.clone(),
            self.assets.clone(),
            self.palettes.clone(),
        );
        self.phase = Phase::Preparing(generation);
        self.preparing_origin = choice.browser_generation;
        self.error.clear();
        effects.task_on("appearance", move || {
            let prepare = || -> Result<Prepared> {
                if cancel.cancelled() {
                    return Err(DomainError::Unavailable(
                        "Wallpaper selection superseded".into(),
                    ));
                }
                let asset = store.stage(&choice.source)?;
                let result = (|| {
                    let image = assets.background(&asset.path)?;
                    let seed = if let Some(revision) = choice.colors {
                        Some((revision, palettes.seed(&asset.path)?))
                    } else {
                        None
                    };
                    if cancel.cancelled() {
                        return Err(DomainError::Unavailable(
                            "Wallpaper selection superseded".into(),
                        ));
                    }
                    Ok(Prepared {
                        asset: asset.clone(),
                        preview: assets.preview(&image),
                        image,
                        seed,
                        browser_generation: choice.browser_generation,
                    })
                })();
                if result.is_err() {
                    store.discard(&asset)?;
                }
                result
            };
            Message::Prepared(generation, prepare())
        });
        effects.redraw("dock");
    }
    fn discard(&self, asset: WallpaperAsset, effects: &mut Effects<Message>) {
        let store = self.store.clone();
        effects.task_on("appearance", move || {
            Message::Discarded(store.discard(&asset))
        });
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) -> Option<Outcome> {
        let outcome = match message {
            Message::Initial(result) if self.current.is_none() => {
                match result {
                    Ok(Some(prepared)) => {
                        self.current = Some(prepared.asset);
                        self.background = Some(prepared.image);
                    }
                    Ok(None) => {}
                    Err(error) if self.generation == 0 => self.error = error.to_string(),
                    Err(_) => {}
                }
                effects.redraw("background");
                None
            }
            Message::Prepared(id, result) if self.phase == Phase::Preparing(id) => {
                self.phase = Phase::Idle;
                if id != self.generation {
                    if let Ok(prepared) = result {
                        self.discard(prepared.asset, effects);
                    }
                    self.start_pending(effects);
                    effects.redraw("dock");
                    return None;
                }
                match result {
                    Ok(prepared) => {
                        self.phase = Phase::Committing(id);
                        let store = self.store.clone();
                        effects.task_on("appearance", move || {
                            let result = store.commit(&prepared.asset);
                            Message::Committed(id, prepared, result)
                        });
                        None
                    }
                    Err(error) => {
                        self.phase = Phase::Idle;
                        self.error = error.to_string();
                        Some(Outcome::Failed {
                            browser_generation: self.preparing_origin,
                            error: self.error.clone(),
                        })
                    }
                }
            }
            Message::Prepared(_, Ok(prepared)) => {
                self.discard(prepared.asset, effects);
                None
            }
            Message::Committed(id, prepared, result) if self.phase == Phase::Committing(id) => {
                self.phase = Phase::Idle;
                let outcome = match result {
                    Ok(()) => {
                        self.current = Some(prepared.asset.clone());
                        self.background = Some(prepared.image.clone());
                        self.error.clear();
                        effects.redraw("background");
                        Some(Outcome::Applied(prepared))
                    }
                    Err(error) => {
                        self.error = error.to_string();
                        self.discard(prepared.asset, effects);
                        Some(Outcome::Failed {
                            browser_generation: prepared.browser_generation,
                            error: self.error.clone(),
                        })
                    }
                };
                self.start_pending(effects);
                outcome
            }
            Message::Discarded(Err(error)) => {
                self.error = error.to_string();
                None
            }
            _ => None,
        };
        effects.redraw("dock");
        outcome
    }
}
impl Drop for Selection {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, sync::Mutex};
    #[derive(Default)]
    struct State {
        current: Option<WallpaperAsset>,
        calls: Vec<String>,
        fail_decode: bool,
        fail_commit: bool,
    }
    #[derive(Default)]
    struct Fake(Mutex<State>);
    impl WallpaperPort for Fake {
        fn list(&self) -> Vec<Wallpaper> {
            vec![]
        }
        fn current(&self) -> Result<Option<WallpaperAsset>> {
            Ok(self.0.lock().unwrap().current.clone())
        }
        fn stage(&self, source: &Wallpaper) -> Result<WallpaperAsset> {
            self.0
                .lock()
                .unwrap()
                .calls
                .push(format!("stage:{}", source.name));
            Ok(WallpaperAsset {
                source: source.clone(),
                path: format!("/staged/{}", source.name),
            })
        }
        fn commit(&self, asset: &WallpaperAsset) -> Result<()> {
            let mut state = self.0.lock().unwrap();
            state.calls.push(format!("commit:{}", asset.source.name));
            if state.fail_commit {
                return Err(DomainError::Failed("commit fixture".into()));
            }
            state.current = Some(asset.clone());
            Ok(())
        }
        fn discard(&self, asset: &WallpaperAsset) -> Result<()> {
            self.0
                .lock()
                .unwrap()
                .calls
                .push(format!("discard:{}", asset.source.name));
            Ok(())
        }
    }
    impl AssetPort for Fake {
        fn initial(&self) -> BTreeMap<String, Arc<ImageData>> {
            BTreeMap::new()
        }
        fn load(&self, _: &[Application], _: &[Wallpaper]) -> crate::ports::Images {
            vec![]
        }
        fn preview(&self, image: &ImageData) -> Arc<ImageData> {
            Arc::new(image.downsample(1))
        }
        fn background(&self, path: &str) -> Result<Arc<ImageData>> {
            let mut state = self.0.lock().unwrap();
            state.calls.push(format!("decode:{path}"));
            if state.fail_decode {
                return Err(DomainError::Invalid("decode fixture".into()));
            }
            Ok(Arc::new(
                ImageData::from_rgba(path, 2, 2, vec![255; 16]).unwrap(),
            ))
        }
    }
    impl PaletteGenerationPort for Fake {
        fn seed(&self, path: &str) -> Result<Rgb> {
            self.0.lock().unwrap().calls.push(format!("seed:{path}"));
            Ok(Rgb::new(80, 120, 180))
        }
    }
    fn choice(name: &str) -> Choice {
        Choice {
            source: Wallpaper {
                name: name.into(),
                path: format!("/source/{name}"),
            },
            colors: None,
            browser_generation: None,
        }
    }
    struct Harness {
        selection: Selection,
        fake: Arc<Fake>,
        effects: Effects<Message>,
    }
    impl Harness {
        fn new() -> Self {
            let fake = Arc::new(Fake::default());
            Self {
                selection: Selection::new(fake.clone(), fake.clone(), fake.clone()),
                fake,
                effects: Effects::default(),
            }
        }
        fn choose(&mut self, name: &str) {
            self.selection.choose(choice(name), &mut self.effects);
        }
        fn step(&mut self) -> Option<Outcome> {
            let message = self.effects.tasks.remove(0).run();
            self.selection.update(message, &mut self.effects)
        }
        fn drain(&mut self) {
            let mut steps = 0;
            while !self.effects.tasks.is_empty() {
                self.step();
                steps += 1;
                assert!(steps < 30);
            }
        }
        fn assert_selected(&self, name: &str) {
            assert_eq!(self.selection.current.as_ref().unwrap().source.name, name);
            assert_eq!(self.selection.current, self.fake.current().unwrap());
            assert_eq!(
                self.selection.background.as_ref().unwrap().key,
                format!("/staged/{name}")
            );
        }
    }
    #[test]
    fn decode_and_commit_failures_keep_previous_background_and_clean_candidates() {
        let mut h = Harness::new();
        h.choose("good");
        h.drain();
        let before = h.selection.background.clone().unwrap();
        h.fake.0.lock().unwrap().fail_decode = true;
        h.choose("invalid");
        h.drain();
        h.assert_selected("good");
        assert!(h.selection.error.contains("decode fixture"));
        assert!(Arc::ptr_eq(
            &before,
            h.selection.background.as_ref().unwrap()
        ));
        h.fake.0.lock().unwrap().fail_decode = false;
        h.fake.0.lock().unwrap().fail_commit = true;
        h.choose("unwritable");
        h.drain();
        h.assert_selected("good");
        assert!(h.selection.error.contains("commit fixture"));
        assert!(!h.selection.busy());
        let state = h.fake.0.lock().unwrap();
        assert!(state.calls.contains(&"discard:invalid".into()));
        assert!(!state.calls.contains(&"commit:invalid".into()));
        assert!(state.calls.contains(&"discard:unwritable".into()));
    }
    #[test]
    fn rapid_choices_coalesce_and_stale_prepared_assets_never_commit() {
        let mut h = Harness::new();
        h.choose("first");
        let stale = h.effects.tasks.remove(0).run();
        for i in 0..100 {
            h.choose(&format!("next-{i}"));
        }
        assert!(h.effects.tasks.is_empty());
        h.selection.update(stale, &mut h.effects);
        h.drain();
        h.assert_selected("next-99");
        let calls = &h.fake.0.lock().unwrap().calls;
        assert_eq!(calls.iter().filter(|c| c.starts_with("stage:")).count(), 2);
        assert!(!calls.contains(&"commit:first".into()));
        assert!(calls.contains(&"discard:first".into()));
    }
    #[test]
    fn accepted_commit_finishes_before_newest_intent_and_decodes_once() {
        let mut h = Harness::new();
        h.choose("accepted");
        h.step();
        h.choose("superseded");
        h.choose("latest");
        assert!(matches!(h.step(), Some(Outcome::Applied(_))));
        h.assert_selected("accepted");
        assert!(h.selection.busy());
        h.drain();
        h.assert_selected("latest");
        let calls = &h.fake.0.lock().unwrap().calls;
        assert!(!calls.contains(&"stage:superseded".into()));
        assert_eq!(
            calls
                .iter()
                .filter(|c| *c == "decode:/staged/accepted")
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|c| *c == "decode:/staged/latest")
                .count(),
            1
        );
    }
    #[test]
    fn late_download_cannot_override_a_newer_local_choice_or_source_change() {
        let mut h = Harness::new();
        let ticket = h.selection.reserve();
        assert!(h.selection.busy());
        h.choose("local");
        h.drain();
        h.selection
            .submit(ticket, choice("late network"), &mut h.effects);
        h.drain();
        h.assert_selected("local");
        let ticket = h.selection.reserve();
        h.selection.cancel_reserved(ticket);
        h.selection
            .submit(ticket, choice("cancelled source"), &mut h.effects);
        h.drain();
        assert!(!h.selection.busy());
        h.assert_selected("local");
    }
    #[test]
    fn startup_result_cannot_replace_committed_selection() {
        let mut h = Harness::new();
        h.selection.load(
            Ok(Some(WallpaperAsset {
                source: choice("startup").source,
                path: "/startup".into(),
            })),
            &mut h.effects,
        );
        let initial = h.effects.tasks.remove(0).run();
        h.choose("new");
        h.drain();
        h.selection.update(initial, &mut h.effects);
        h.assert_selected("new");
    }
    #[test]
    fn startup_background_is_available_while_next_selection_prepares() {
        let mut h = Harness::new();
        h.selection.load(
            Ok(Some(WallpaperAsset {
                source: choice("startup").source,
                path: "/startup".into(),
            })),
            &mut h.effects,
        );
        let initial = h.effects.tasks.remove(0).run();
        h.choose("new");
        h.selection.update(initial, &mut h.effects);
        assert_eq!(h.selection.asset_path(), "/startup");
        h.drain();
        h.assert_selected("new");
    }
    #[test]
    fn cancellation_before_preparation_does_no_storage_or_decode_work() {
        let mut h = Harness::new();
        h.choose("obsolete");
        h.choose("new");
        h.drain();
        h.assert_selected("new");
        assert!(
            !h.fake
                .0
                .lock()
                .unwrap()
                .calls
                .contains(&"stage:obsolete".into())
        );
    }
    #[test]
    fn palette_sampling_uses_validated_snapshot_and_prepared_image_reaches_commit() {
        let mut h = Harness::new();
        let mut selection = choice("colors");
        selection.colors = Some(Default::default());
        h.selection.choose(selection, &mut h.effects);
        h.step();
        let Some(Outcome::Applied(prepared)) = h.step() else {
            panic!("missing apply outcome")
        };
        h.assert_selected("colors");
        assert!(prepared.seed.is_some());
        assert!(Arc::ptr_eq(
            &prepared.image,
            h.selection.background.as_ref().unwrap()
        ));
        assert!(prepared.preview.bytes() < prepared.image.bytes());
        assert_eq!(
            &h.fake.0.lock().unwrap().calls[..],
            &[
                "stage:colors",
                "decode:/staged/colors",
                "seed:/staged/colors",
                "commit:colors"
            ]
        );
    }
    fn desktop() -> crate::desktop::Desktop {
        let fake = Arc::new(Fake::default());
        let mut adapters = crate::test_adapters::adapters();
        adapters.wallpaper = fake.clone();
        adapters.assets = fake.clone();
        adapters.palette_generation = fake;
        crate::desktop::Desktop::new(adapters)
    }
    fn drain_desktop(
        app: &mut crate::desktop::Desktop,
        effects: &mut Effects<crate::desktop::Message>,
    ) {
        use lucent_api::Component;
        let mut steps = 0;
        while !effects.tasks.is_empty() {
            let result = effects.tasks.remove(0).run();
            app.update(result, effects);
            steps += 1;
            assert!(steps < 40);
        }
    }
    fn download(
        app: &mut crate::desktop::Desktop,
        effects: &mut Effects<crate::desktop::Message>,
    ) -> u64 {
        use {
            crate::{desktop::Message as D, wallpaper_browser::Message as B},
            lucent_api::Component,
        };
        app.update(
            D::Browser(B::Source(Some(WallpaperProvider::Wallhaven))),
            effects,
        );
        app.browser.items = vec![RemoteWallpaper {
            provider: WallpaperProvider::Wallhaven,
            id: "fixture".into(),
            title: "Fixture".into(),
            page_url: String::new(),
            thumbnail_url: String::new(),
            image_url: String::new(),
            resolution: "2x2".into(),
        }];
        app.update(D::Browser(B::Download(true)), effects);
        app.browser.generation
    }
    #[test]
    fn desktop_ignores_download_after_local_selection_and_provider_change() {
        use {
            crate::{desktop::Message as D, wallpaper_browser::Message as B},
            lucent_api::Component,
        };
        let mut app = desktop();
        let mut effects = Effects::default();
        let id = download(&mut app, &mut effects);
        app.update(D::SetWallpaper("/source/local".into()), &mut effects);
        drain_desktop(&mut app, &mut effects);
        app.update(
            D::Browser(B::Ready(id, Ok((choice("old network").source, true)))),
            &mut effects,
        );
        drain_desktop(&mut app, &mut effects);
        assert_eq!(app.wallpaper.source_path(), "/source/local");
        assert_eq!(app.browser.status, "Downloaded · selection superseded");
        let id = download(&mut app, &mut effects);
        app.update(D::Browser(B::Source(None)), &mut effects);
        app.update(
            D::Browser(B::Ready(id, Ok((choice("cancelled").source, true)))),
            &mut effects,
        );
        drain_desktop(&mut app, &mut effects);
        assert!(!app.wallpaper.busy());
        assert_eq!(app.wallpaper.source_path(), "/source/local");
    }
    #[test]
    fn desktop_manual_palette_during_download_wins_over_automatic_colors() {
        use {
            crate::{desktop::Message as D, wallpaper_browser::Message as B},
            lucent_api::Component,
        };
        let mut app = desktop();
        let mut effects = Effects::default();
        let id = download(&mut app, &mut effects);
        let explicit = lucent_design::palettes()[2].clone();
        app.update(D::Palette(explicit.clone()), &mut effects);
        app.update(
            D::Browser(B::Ready(id, Ok((choice("network").source, true)))),
            &mut effects,
        );
        drain_desktop(&mut app, &mut effects);
        assert_eq!(app.wallpaper.source_path(), "/source/network");
        assert_eq!(app.settings.palette, Some(explicit));
        assert_eq!(app.browser.status, "Applied");
    }
    #[test]
    fn desktop_extraction_result_cannot_replace_later_palette_or_wallpaper() {
        use {crate::desktop::Message as D, lucent_api::Component};
        let mut app = desktop();
        let mut effects = Effects::default();
        app.update(D::SetWallpaper("/source/first".into()), &mut effects);
        drain_desktop(&mut app, &mut effects);
        app.update(D::ExtractPalette, &mut effects);
        let old_result = effects.tasks.remove(0).run();
        let explicit = lucent_design::palettes()[2].clone();
        app.update(D::Palette(explicit.clone()), &mut effects);
        app.update(old_result, &mut effects);
        drain_desktop(&mut app, &mut effects);
        assert_eq!(app.settings.palette, Some(explicit.clone()));
        app.update(D::ExtractPalette, &mut effects);
        let old_result = effects.tasks.remove(0).run();
        app.update(D::SetWallpaper("/source/second".into()), &mut effects);
        drain_desktop(&mut app, &mut effects);
        app.update(old_result, &mut effects);
        drain_desktop(&mut app, &mut effects);
        assert_eq!(app.settings.palette, Some(explicit));
    }
    #[test]
    fn ambiguous_worker_failure_requests_supervised_reload() {
        use lucent_api::{Application as _, RuntimeFailure};
        let app = desktop();
        assert!(
            app.runtime_failure(RuntimeFailure {
                source: "effect queue appearance".into(),
                reason: "panic".into()
            })
            .is_none()
        );
        assert!(
            app.runtime_failure(RuntimeFailure {
                source: "effect queue controls".into(),
                reason: "panic".into()
            })
            .is_some()
        );
    }
}
