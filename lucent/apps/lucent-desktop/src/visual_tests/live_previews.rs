//! Owned finite media fixtures drive the normal preview port/subscription path.
//! No source files, imports, background decoders or runtime-only hooks are used.
use super::*;
use crate::live_wallpaper::Message as Live;
use lucent_domain::*;
use std::sync::Mutex;

fn sources() -> Vec<Wallpaper> {
    [
        ("amber.mp4", "Amber Lake"),
        ("moonlit.webm", "Moonlit Lake"),
        ("canopy.gif", "Rain in the Canopy"),
    ]
    .into_iter()
    .map(|(path, name)| Wallpaper {
        path: format!("/fixture/{path}"),
        name: name.into(),
    })
    .collect()
}

pub(super) fn preview(
    source: &Wallpaper,
    options: WallpaperPreviewOptions,
    emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
    stop: Arc<dyn StopSignal>,
    alpha: bool,
    failed: bool,
) {
    if stop.cancelled() {
        return;
    }
    if failed {
        emit(Err(DomainError::Unavailable(
            "Preview fixture unavailable".into(),
        )));
        return;
    }
    let (width, height) = options.fit_size(160, 100).unwrap();
    for frame in 0..2 {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                let mut color = if y < height / 2 {
                    if source.name.contains("Amber") {
                        [224, 139, 83, 255]
                    } else {
                        [51, 66, 111, 255]
                    }
                } else if (x / 8 + y / 4 + frame) % 3 == 0 {
                    [131, 169, 164, 255]
                } else {
                    [41, 79, 86, 255]
                };
                if source.name.contains("Canopy") {
                    color = [35, 106, 81, 255];
                }
                if alpha && x < width / 3 {
                    // Contrasting invisible RGB exercises prepared alpha,
                    // against both semantic palette surfaces in the card.
                    color = [255, 0, 170, if y < height / 2 { 0 } else { 96 }];
                }
                pixels.extend(color);
            }
        }
        if stop.cancelled()
            || !emit(MediaFrame::rgba_with_aspect(
                width,
                height,
                pixels,
                (160, 100),
                std::time::Duration::from_millis(u64::from(frame) * 100),
            ))
        {
            break;
        }
    }
}

struct Playback {
    alpha: bool,
    failed: bool,
}
impl WallpaperPlaybackPort for Playback {
    fn library(&self) -> Result<Vec<Wallpaper>> {
        Ok(sources())
    }
    fn prepare(&self, _: &Wallpaper) -> Result<LiveWallpaperAsset> {
        panic!("visual preview selection must not import")
    }
    fn play(
        &self,
        _: &LiveWallpaperAsset,
        _: PlaybackOptions,
        _: &mut dyn FnMut(Result<MediaFrame>) -> bool,
        _: Arc<dyn StopSignal>,
    ) {
        panic!("visual preview must not run a background decoder")
    }
    fn preview(
        &self,
        source: &Wallpaper,
        options: WallpaperPreviewOptions,
        emit: &mut dyn FnMut(Result<MediaFrame>) -> bool,
        stop: Arc<dyn StopSignal>,
    ) {
        preview(source, options, emit, stop, self.alpha, self.failed);
    }
}

pub(super) fn install(app: &mut Desktop, name: &str) {
    app.live_wallpaper = crate::live_wallpaper::LiveWallpaper::new(
        Arc::new(Playback {
            alpha: name.ends_with("alpha"),
            failed: name.ends_with("error"),
        }),
        app.adapters.assets.clone(),
    );
    app.settings_writable = true;
    app.settings.live_wallpapers.default = Some(LiveWallpaperSettings {
        asset: LiveWallpaperAsset {
            source: sources().remove(usize::from(name.ends_with("ready"))),
            path: "/owned/fixture-current.mp4".into(),
            poster: Wallpaper {
                path: "/owned/fixture-poster.png".into(),
                name: "Fixture poster".into(),
            },
        },
        fps: WallpaperFrameRate::Source,
        paused: false,
        enabled: true,
    });
}

/// Synchronous finite mocks, including the normal retirement ACK. A production
/// animated adapter remains running; this fixture chooses a deterministic frame.
pub(super) fn complete(app: &mut Desktop) {
    for _ in 0..4 {
        let Some(subscription) = app.live_wallpaper.preview.subscriptions().pop() else {
            return;
        };
        let messages = Arc::new(Mutex::new(Vec::new()));
        let output = messages.clone();
        (subscription.run)(
            Emitter::new(move |message| output.lock().unwrap().push(message)),
            Cancellation::default(),
        );
        let messages = std::mem::take(&mut *messages.lock().unwrap());
        assert!(
            !messages.is_empty(),
            "finite producer must acknowledge return"
        );
        for message in messages {
            app.update(
                Message::LiveWallpaper(Live::Preview(message)),
                &mut Effects::default(),
            );
        }
    }
    assert!(app.live_wallpaper.preview.subscriptions().is_empty());
}

pub(super) fn prepare(app: &mut Desktop, name: &str, width: f32, height: f32) {
    let saved = app.settings.clone();
    app.update(
        Message::LiveWallpaper(Live::PreviewViewport(width, height, Density::ONE)),
        &mut Effects::default(),
    );
    app.update(
        Message::LiveWallpaper(Live::SelectSource(1)),
        &mut Effects::default(),
    );
    if !name.ends_with("loading") {
        complete(app);
    }
    if name.ends_with("error") {
        app.live_wallpaper.scroll = lucent_design::space::PAGE;
    }
    assert_eq!(app.settings, saved, "browsing must not change preferences");
    assert_eq!(app.live_wallpaper.selection, 1);
    let state = app.live_wallpaper.preview.inspect();
    if name.ends_with("loading") {
        assert_eq!(state["loading"], true);
    } else if name.ends_with("error") {
        assert_eq!(state["failed"], true);
    } else {
        assert!(
            app.live_wallpaper
                .preview
                .frame_for(&sources()[1].path)
                .is_some()
        );
        // The latest hero frame can coalesce its initial still before delivery.
        // The two adjacent sources remain frozen, distinct cache entries.
        assert!(state["cached"].as_u64().unwrap() >= 2);
        for index in [0, 2] {
            assert!(
                app.live_wallpaper
                    .preview
                    .frame_for(&sources()[index].path)
                    .is_some()
            );
        }
    }
}

pub(super) fn assert_scene(scene: &Scene<Message>, name: &str) {
    let selected = scene
        .hits
        .iter()
        .find(|hit| hit.id == "live-library-1")
        .unwrap();
    assert!(matches!(
        selected.click,
        Some(Message::LiveWallpaper(Live::SelectSource(1)))
    ));
    let apply = scene
        .hits
        .iter()
        .find(|hit| hit.id == "live-apply")
        .unwrap();
    assert!(matches!(
        apply.click,
        Some(Message::LiveWallpaper(Live::ApplySelection))
    ));
    for hit in [selected, apply] {
        assert!(
            fully_visible(hit.rect, hit.clip.bounds),
            "{name}: {} clipped",
            hit.id
        );
    }
    if name.ends_with("error") {
        assert!(scene.hits.iter().any(|hit| hit.id == "live-preview-retry"));
    }
}

#[test]
fn actual_carousel_card_hit_selects_without_applying_or_importing() {
    let mut app = fixture(0, false);
    install(&mut app, "live-wallpaper-preview-unapplied");
    open_live_fixture(&mut app);
    prepare(&mut app, "live-wallpaper-preview-unapplied", 390., 580.);
    let layout = Layout::new(fonts(&app));
    let cx = ViewContext {
        surface: "live-wallpaper",
        instance: None,
        density: Density::ONE,
        width: 390.,
        height: 580.,
        now: 1.,
    };
    let scene = layout.build(
        &app.view(&cx),
        cx.width,
        cx.height,
        &Interaction::default(),
        cx.now,
    );
    assert_scene(&scene, "unapplied native component");
    let saved = app.settings.clone();
    let mut effects = Effects::default();
    let action = scene
        .hits
        .iter()
        .find(|hit| hit.id == "live-library-1")
        .unwrap()
        .click
        .clone()
        .unwrap();
    app.update(action, &mut effects);
    assert_eq!(app.settings, saved);
    assert!(
        effects.tasks.is_empty(),
        "active card cannot start an import"
    );
    assert!(app.live_wallpaper.importing_target().is_none());
}
