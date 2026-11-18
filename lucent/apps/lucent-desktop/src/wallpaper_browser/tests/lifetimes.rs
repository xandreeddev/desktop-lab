use super::*;
use lucent_ui::{Interaction, Layout, Scene};

fn item(id: &str) -> RemoteWallpaper {
    RemoteWallpaper {
        provider: WallpaperProvider::Wallhaven,
        id: id.into(),
        title: id.into(),
        page_url: format!("https://fixture.invalid/{id}"),
        thumbnail_url: format!("https://fixture.invalid/{id}/small"),
        image_url: format!("https://fixture.invalid/{id}/original"),
        resolution: "1920x1080".into(),
    }
}
fn reply(browser: &mut Browser, page: u32, has_more: bool) {
    let id = browser.generation;
    browser.update(
        Message::Results(
            id,
            Ok(WallpaperPage {
                items: vec![item("A"), item("B")],
                page,
                has_more,
            }),
        ),
        &mut Effects::default(),
    );
    browser.update(Message::Finished(id), &mut Effects::default());
}
fn browser() -> Browser {
    let mut browser = Browser::default();
    for message in [
        Message::Source(Some(WallpaperProvider::Wallhaven)),
        Message::Query("mountains".into()),
        Message::Search,
    ] {
        browser.update(message, &mut Effects::default());
    }
    reply(&mut browser, 1, true);
    browser
}
fn scene(browser: &Browser, input: &Interaction<Message>) -> Scene<Message> {
    let theme = Theme::new(false);
    let tree = theme.apply(
        Element::column(vec![
            Element::row(browser.source_choices(theme)).size(800., 40.),
            browser.view(800., 580., theme),
        ])
        .gap(8.)
        .size(800., 640.),
    );
    let font = lucent_ui::text::Font::from_bytes(
        include_bytes!("../../../assets/LucentSans.ttf") as &[u8],
        Default::default(),
    )
    .unwrap();
    Layout::new(Arc::new(vec![font])).build(&tree, 800., 640., input, 1.)
}
fn control(browser: &mut Browser, id: &str, keyboard: bool) -> Message {
    let mut input = Interaction::default();
    let mut tree = scene(browser, &input);
    if keyboard {
        for _ in 0..=tree.hits.len() {
            for message in input.key(&tree, &Key::Tab) {
                browser.update(message, &mut Effects::default());
            }
            tree = scene(browser, &input);
            if input.focus.as_deref() == Some(id) {
                break;
            }
        }
        assert_eq!(input.focus.as_deref(), Some(id));
        input.key(&tree, &Key::Enter).pop().unwrap()
    } else {
        let hit = tree.hits.iter().find(|hit| hit.id == id).unwrap();
        let (x, y) = (hit.rect.x + hit.rect.w / 2., hit.rect.y + hit.rect.h / 2.);
        assert!(hit.contains(x, y), "{id} must be reachable");
        assert!(input.press(&tree, x, y).is_none());
        input.release(x, y).unwrap()
    }
}
fn requested(browser: &Browser, query: &str, page: u32) {
    assert!(
        matches!(&browser.job, Some(Job::Search(WallpaperProvider::Wallhaven, actual, actual_page))
        if actual == query && *actual_page == page)
    );
}
fn assert_disabled(browser: &Browser, ids: &[&str]) {
    let mut input = Interaction::default();
    let scene = scene(browser, &input);
    for id in ids {
        assert!(
            scene.hits.iter().all(|hit| hit.id != *id),
            "{id} must have no pointer hit"
        );
    }
    for _ in 0..=scene.hits.len() {
        let _ = input.key(&scene, &Key::Tab);
        assert!(
            !ids.iter().any(|id| input.focus.as_deref() == Some(*id)),
            "disabled action received focus"
        );
    }
}

#[test]
fn unavailable_and_busy_footer_actions_have_no_pointer_or_keyboard_authority() {
    let mut browser = Browser::default();
    let all = [
        "wallpaper-search-submit",
        "catalog-previous",
        "catalog-next",
        "download-wallpaper",
        "download-theme",
    ];
    assert_disabled(&browser, &all);
    browser.update(
        Message::Source(Some(WallpaperProvider::Wallhaven)),
        &mut Effects::default(),
    );
    assert_disabled(&browser, &all[1..]);
    let search = control(&mut browser, "wallpaper-search-submit", true);
    browser.update(search, &mut Effects::default());
    assert_disabled(&browser, &all);
    reply(&mut browser, 1, true);
    assert_disabled(&browser, &["catalog-previous"]);
    let download = control(&mut browser, "download-theme", true);
    browser.update(download, &mut Effects::default());
    assert_disabled(&browser, &all);
    browser.update(
        Message::Ready(
            browser.generation,
            Err(DomainError::Unavailable("offline".into())),
        ),
        &mut Effects::default(),
    );
    let tree = scene(&browser, &Interaction::default());
    for id in [
        "wallpaper-search-submit",
        "catalog-next",
        "download-wallpaper",
        "download-theme",
    ] {
        assert!(
            tree.hits.iter().any(|hit| hit.id == id && hit.tab_stop),
            "{id} must recover"
        );
    }
}

#[test]
fn metadata_and_status_have_bounded_lines_and_readable_narrow_footer_geometry() {
    use lucent_ui::Paint;
    let fonts = Arc::new(vec![
        lucent_ui::text::Font::from_bytes(
            include_bytes!("../../../assets/LucentSans.ttf") as &[u8],
            Default::default(),
        )
        .unwrap(),
    ]);
    let layout = Layout::new(fonts.clone());
    for light in [false, true] {
        for (width, height) in [(320., 240.), (320., 580.), (640., 580.)] {
            for long in [false, true] {
                let mut browser = browser();
                browser.items.truncate(1);
                browser.items[0].id = format!("rid-{}", "long-provider-identifier".repeat(20));
                browser.items[0].resolution = "unbounded-resolution".repeat(20);
                let readable = "Wallpaper provider unavailable. Please try again later.";
                browser.status = if long {
                    format!("{readable} {}", "Extra provider explanation. ".repeat(20))
                } else {
                    readable.into()
                };
                let theme = Theme::new(light);
                let tree = theme.apply(browser.view(width, height, theme));
                let scene = layout.build(&tree, width, height, &Interaction::default(), 1.);
                let next = scene
                    .hits
                    .iter()
                    .find(|hit| hit.id == "catalog-next")
                    .unwrap();
                let mut status = Vec::new();
                let mut metadata = 0;
                for paint in &scene.paint {
                    if let Paint::Text {
                        text,
                        rect,
                        clip,
                        size,
                        face,
                        layout,
                        ..
                    } = paint
                    {
                        let is_metadata = text.starts_with("rid-");
                        let is_status = rect.y >= next.rect.y - 0.1
                            && rect.y < next.rect.y + next.rect.h
                            && rect.x > next.rect.w
                            && rect.x < next.rect.x;
                        if is_metadata || is_status {
                            assert!(
                                lucent_ui::text::run_metrics_with_layout(
                                    &fonts[*face],
                                    text,
                                    *size,
                                    layout.clone()
                                )
                                .height
                                    * text.lines().count() as f32
                                    <= rect.h + 0.1,
                                "text exceeded its allocated height: {text}"
                            );
                            assert!(
                                rect.x >= clip.bounds.x - 0.1
                                    && rect.y >= clip.bounds.y - 0.1
                                    && rect.x + rect.w <= clip.bounds.x + clip.bounds.w + 0.1
                                    && rect.y + rect.h <= clip.bounds.y + clip.bounds.h + 0.1,
                                "clipped {text}"
                            );
                            assert!(
                                lucent_ui::text::width_with_layout(
                                    &fonts[*face],
                                    text,
                                    *size,
                                    layout.clone()
                                ) <= rect.w + 0.1,
                                "text exceeded its measured width: {text}"
                            );
                        }
                        if is_metadata {
                            metadata += 1;
                        }
                        if is_status {
                            assert!(rect.x + rect.w <= next.rect.x + 0.1, "status overlaps Next");
                            assert!(
                                rect.y + rect.h <= next.rect.y + next.rect.h + 0.1,
                                "status overlaps actions below"
                            );
                            status.extend(text.lines().map(|line| line.trim().to_owned()));
                        }
                    }
                }
                if height > 240. {
                    assert_eq!(metadata, 1);
                }
                assert!(!status.is_empty());
                assert!(status.len() <= t::STATUS_LINES as usize);
                if !long {
                    assert_eq!(
                        status.join(" "),
                        readable,
                        "ordinary error text must remain complete at {width}x{height}"
                    );
                }
            }
        }
    }
}

#[test]
fn injected_catalog_receives_submitted_pages_and_the_exact_accepted_download() {
    use std::sync::Mutex;
    #[derive(Default)]
    struct Catalog {
        searches: Mutex<Vec<(WallpaperProvider, String, u32)>>,
        downloads: Mutex<Vec<RemoteWallpaper>>,
    }
    impl WallpaperCatalogPort for Catalog {
        fn search(
            &self,
            provider: WallpaperProvider,
            query: &str,
            page: u32,
        ) -> domain::Result<WallpaperPage> {
            self.searches
                .lock()
                .unwrap()
                .push((provider, query.into(), page));
            Ok(WallpaperPage {
                items: vec![item(&format!("{query}-{page}"))],
                page,
                has_more: page == 1,
            })
        }
        fn preview(&self, _: &RemoteWallpaper) -> domain::Result<String> {
            Err(DomainError::Unavailable(
                "owned fixture has no thumbnail".into(),
            ))
        }
        fn download(&self, source: &RemoteWallpaper) -> domain::Result<Wallpaper> {
            self.downloads.lock().unwrap().push(source.clone());
            Ok(Wallpaper {
                path: "/fixture/download.png".into(),
                name: source.title.clone(),
            })
        }
    }
    fn complete(
        browser: &mut Browser,
        catalog: &Arc<dyn WallpaperCatalogPort>,
        assets: &Arc<dyn AssetPort>,
    ) {
        let subscriptions = browser.subscriptions(catalog, assets);
        assert_eq!(subscriptions.len(), 1);
        for subscription in subscriptions {
            let (sender, receiver) = std::sync::mpsc::channel();
            (subscription.run)(
                Emitter::new(move |message| {
                    sender.send(message).unwrap();
                }),
                Cancellation::default(),
            );
            for message in receiver.try_iter() {
                browser.update(message, &mut Effects::default());
            }
        }
    }
    let recorder = Arc::new(Catalog::default());
    let catalog: Arc<dyn WallpaperCatalogPort> = recorder.clone();
    let assets = crate::test_adapters::adapters().assets;
    let mut browser = Browser::default();
    for message in [
        Message::Source(Some(WallpaperProvider::Wallhaven)),
        Message::Query("A".into()),
        Message::Search,
    ] {
        browser.update(message, &mut Effects::default());
    }
    complete(&mut browser, &catalog, &assets);
    browser.update(Message::Query("B".into()), &mut Effects::default());
    assert_eq!(
        recorder.searches.lock().unwrap().len(),
        1,
        "draft editing performs no provider IO"
    );
    let next = control(&mut browser, "catalog-next", true);
    browser.update(next, &mut Effects::default());
    complete(&mut browser, &catalog, &assets);
    assert_eq!(
        *recorder.searches.lock().unwrap(),
        [
            (WallpaperProvider::Wallhaven, "A".into(), 1),
            (WallpaperProvider::Wallhaven, "A".into(), 2),
        ]
    );
    let source = browser.items[0].clone();
    let download = control(&mut browser, "download-wallpaper", false);
    browser.update(download, &mut Effects::default());
    browser.retire_controls();
    complete(&mut browser, &catalog, &assets);
    assert_eq!(*recorder.downloads.lock().unwrap(), [source]);
    assert!(!browser.busy());
    assert_eq!(browser.status, "Downloaded · applying…");
}

#[test]
fn duplicate_root_completion_cannot_erase_failure_or_start_selection_work() {
    use crate::desktop::{Desktop, Message as Root};
    let mut app = Desktop::new(crate::test_adapters::adapters());
    app.browser = browser();
    let mut effects = Effects::default();
    app.update(
        Root::Browser(app.browser.download_message(false)),
        &mut effects,
    );
    let generation = app.browser.generation;
    app.update(
        Root::Browser(Message::Ready(
            generation,
            Err(DomainError::Unavailable("original failure".into())),
        )),
        &mut effects,
    );
    assert!(!app.browser.busy());
    effects.tasks.clear();
    app.update(
        Root::Browser(Message::Ready(
            generation,
            Ok((
                Wallpaper {
                    path: "/fixture/late.png".into(),
                    name: "Late success".into(),
                },
                false,
            )),
        )),
        &mut effects,
    );
    assert!(app.browser.status.contains("original failure"));
    assert!(effects.tasks.is_empty());
}

#[test]
fn rendered_pagination_uses_submitted_query_and_explicit_search_uses_the_draft() {
    for keyboard in [false, true] {
        let mut browser = browser();
        browser.update(Message::Query("ocean".into()), &mut Effects::default());
        let next = control(&mut browser, "catalog-next", keyboard);
        browser.update(next, &mut Effects::default());
        requested(&browser, "mountains", 2);
        assert_eq!(browser.query, "ocean");
        assert_eq!(browser.submitted, "mountains");
        assert_eq!((browser.page, browser.has_more), (0, false));
        reply(&mut browser, 2, true);
        let previous = control(&mut browser, "catalog-previous", keyboard);
        browser.update(previous, &mut Effects::default());
        requested(&browser, "mountains", 1);
        reply(&mut browser, 1, true);
        let search = control(&mut browser, "wallpaper-search-submit", keyboard);
        browser.update(search, &mut Effects::default());
        requested(&browser, "ocean", 1);
        assert_eq!(browser.submitted, "ocean");
    }
}

#[test]
fn retired_pages_and_failed_new_queries_cannot_reuse_previous_page_metadata() {
    for source_change in [false, true] {
        let mut browser = browser();
        let old = control(&mut browser, "catalog-next", false);
        if source_change {
            browser.update(
                Message::Source(Some(WallpaperProvider::AlphaCoders)),
                &mut Effects::default(),
            );
            assert!(browser.submitted.is_empty());
        } else {
            browser.update(Message::Query("new".into()), &mut Effects::default());
            browser.update(Message::Search, &mut Effects::default());
            browser.update(
                Message::Results(
                    browser.generation,
                    Err(DomainError::Unavailable("offline".into())),
                ),
                &mut Effects::default(),
            );
            browser.update(
                Message::Finished(browser.generation),
                &mut Effects::default(),
            );
        }
        let generation = browser.generation;
        browser.update(old, &mut Effects::default());
        assert_disabled(&browser, &["catalog-next", "catalog-previous"]);
        browser.update(browser.page_message(1), &mut Effects::default());
        assert!(!browser.busy());
        assert_eq!(browser.generation, generation);
        assert_eq!((browser.page, browser.has_more), (0, false));
    }
}

#[test]
fn page_boundaries_and_duplicate_actions_admit_only_one_adjacent_request() {
    let mut browser = browser();
    assert_disabled(&browser, &["catalog-previous"]);
    browser.update(browser.page_message(0), &mut Effects::default());
    assert!(!browser.busy());
    let next = control(&mut browser, "catalog-next", false);
    browser.update(next.clone(), &mut Effects::default());
    let generation = browser.generation;
    browser.update(next, &mut Effects::default());
    assert_eq!(browser.generation, generation);
    reply(&mut browser, 2, false);
    assert_disabled(&browser, &["catalog-next"]);
    browser.update(browser.page_message(3), &mut Effects::default());
    assert!(!browser.busy());
    browser.page = u32::MAX;
    browser.has_more = true;
    assert_disabled(&browser, &["catalog-next"]);
    browser.update(browser.page_message(0), &mut Effects::default());
    assert!(!browser.busy());
}

#[test]
fn rendered_results_and_downloads_cannot_rebind_to_reused_slots_or_urls() {
    for keyboard in [false, true] {
        for change in 0..3 {
            let mut browser = browser();
            let select = control(&mut browser, "online-wallpaper-0", keyboard);
            let download = control(&mut browser, "download-wallpaper", keyboard);
            match change {
                0 => {
                    browser.update(Message::Query("different".into()), &mut Effects::default());
                    browser.update(Message::Search, &mut Effects::default());
                    reply(&mut browser, 1, true);
                }
                1 => browser.items[0].image_url = "https://fixture.invalid/replacement".into(),
                _ => browser.retire_controls(),
            }
            browser.update(browser.select_message(1), &mut Effects::default());
            let generation = browser.generation;
            browser.update(select, &mut Effects::default());
            browser.update(download, &mut Effects::default());
            assert_eq!(browser.selected, 1);
            assert_eq!(browser.generation, generation);
            assert!(!browser.busy());
        }
    }
}

#[test]
fn selection_round_trip_retires_download_but_preview_delivery_preserves_it() {
    let mut browser = browser();
    let old = control(&mut browser, "download-theme", true);
    for index in [1, 0] {
        browser.update(browser.select_message(index), &mut Effects::default());
    }
    browser.update(old, &mut Effects::default());
    assert!(!browser.busy());
    browser.update(Message::Search, &mut Effects::default());
    browser.update(
        Message::Results(
            browser.generation,
            Ok(WallpaperPage {
                items: vec![item("A"), item("B")],
                page: 1,
                has_more: true,
            }),
        ),
        &mut Effects::default(),
    );
    let current = control(&mut browser, "download-theme", true);
    browser.update(
        Message::Preview(
            browser.generation,
            "A".into(),
            Arc::new(ImageData::from_rgba("fixture", 1, 1, vec![255; 4]).unwrap()),
        ),
        &mut Effects::default(),
    );
    browser.update(current.clone(), &mut Effects::default());
    let generation = browser.generation;
    assert!(matches!(&browser.job, Some(Job::Download(source, true)) if source == &item("A")));
    browser.update(
        Message::Ready(generation, Err(DomainError::Unavailable("offline".into()))),
        &mut Effects::default(),
    );
    browser.update(current, &mut Effects::default());
    assert!(
        !browser.busy(),
        "a completed attempt cannot replay the old Apply callback"
    );
}

#[test]
fn inconsistent_provider_pages_are_not_published_and_can_be_retried() {
    for wrong_provider in [false, true] {
        let mut browser = browser();
        browser.update(Message::Search, &mut Effects::default());
        let mut result = item("bad");
        if wrong_provider {
            result.provider = WallpaperProvider::AlphaCoders;
        }
        browser.update(
            Message::Results(
                browser.generation,
                Ok(WallpaperPage {
                    items: vec![result],
                    page: if wrong_provider { 1 } else { 2 },
                    has_more: true,
                }),
            ),
            &mut Effects::default(),
        );
        browser.update(
            Message::Finished(browser.generation),
            &mut Effects::default(),
        );
        assert!(browser.items.is_empty());
        assert!(!browser.has_more);
        assert!(browser.status.contains("inconsistent"));
        browser.update(Message::Search, &mut Effects::default());
        reply(&mut browser, 1, false);
        assert_eq!(browser.items.len(), 2);
    }
}

#[test]
fn launcher_departure_retires_rendered_search_source_and_download_without_cancelling_accepted_io() {
    use crate::{
        desktop::{Desktop, Message as Root},
        launcher::{self, Mode},
    };
    for mode_change in [false, true] {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.browser = browser();
        let mut effects = Effects::default();
        app.update(
            Root::Launcher(launcher::Message::Mode(Mode::Wallpapers)),
            &mut effects,
        );
        let callbacks = [
            control(&mut app.browser, "wallpaper-search-submit", false),
            control(&mut app.browser, "source-alpha-coders", false),
            control(&mut app.browser, "download-theme", true),
        ];
        app.update(
            Root::Browser(app.browser.download_message(false)),
            &mut effects,
        );
        let generation = app.browser.generation;
        app.update(
            Root::Launcher(if mode_change {
                launcher::Message::Mode(Mode::Apps)
            } else {
                launcher::Message::Close
            }),
            &mut effects,
        );
        app.update(
            Root::Launcher(launcher::Message::Mode(Mode::Wallpapers)),
            &mut effects,
        );
        for message in callbacks {
            app.update(Root::Browser(message), &mut effects);
        }
        assert_eq!(app.browser.generation, generation);
        assert_eq!(app.browser.source, Some(WallpaperProvider::Wallhaven));
        assert!(app.browser.accepts_download(generation));
        app.update(
            Root::Browser(Message::Ready(
                generation,
                Err(DomainError::Unavailable(
                    "accepted download finished".into(),
                )),
            )),
            &mut effects,
        );
        assert!(!app.browser.busy());
        assert!(app.browser.status.contains("accepted download finished"));
    }
}

#[test]
fn draft_edits_survive_result_delivery_but_not_the_retired_control_presentation() {
    let mut browser = browser();
    let mut input = Interaction::default();
    let tree = scene(&browser, &input);
    for _ in 0..=tree.hits.len() {
        for message in input.key(&tree, &Key::Tab) {
            browser.update(message, &mut Effects::default());
        }
        if input.focus.as_deref() == Some("wallpaper-search") {
            break;
        }
    }
    assert_eq!(input.focus.as_deref(), Some("wallpaper-search"));
    let edit = input.key(&tree, &Key::Text("x".into())).pop().unwrap();
    browser.update(Message::Search, &mut Effects::default());
    reply(&mut browser, 1, true);
    browser.update(edit.clone(), &mut Effects::default());
    assert!(browser.query.contains('x'));
    browser.retire_controls();
    let query = browser.query.clone();
    browser.update(edit, &mut Effects::default());
    assert_eq!(browser.query, query);
}
