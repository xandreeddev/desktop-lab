//! Real widget layout and input with owned adapters; no compositor or decoder.
use super::*;
use crate::{artwork::Artwork, media, persistence::Status, ports::AssetPort};
use lucent_api::*;
use lucent_design::{Theme, component::media as token, font, icon, space};
use lucent_domain::*;
use lucent_ui::{Hit, Interaction, Layout, Paint, Scene};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Adapter {
    commands: Mutex<Vec<MediaCommand>>,
}
impl MediaPort for Adapter {
    fn watch(&self, _: &mut dyn FnMut(Result<MediaSnapshot>), _: &dyn StopSignal) {}
    fn start(&self, command: MediaCommand) -> Result<MediaOperationId> {
        let mut commands = self.commands.lock().unwrap();
        commands.push(command);
        MediaOperationId::new(commands.len() as u64)
    }
}
impl AssetPort for Adapter {
    fn initial(&self) -> BTreeMap<String, Arc<ImageData>> {
        BTreeMap::new()
    }
    fn application_icons(&self, _: &[lucent_domain::Application]) -> crate::ports::Images {
        vec![]
    }
    fn thumbnail(&self, _: &str) -> Result<Arc<ImageData>> {
        Err(DomainError::Unavailable("owned fixture".into()))
    }
    fn preview(&self, image: &ImageData) -> Arc<ImageData> {
        Arc::new(image.downsample(1))
    }
    fn background(
        &self,
        _: &str,
        _: Option<crate::ports::BackgroundTarget>,
    ) -> Result<Arc<ImageData>> {
        Err(DomainError::Unavailable("owned fixture".into()))
    }
    fn media_artwork(&self, _: &MediaArtworkSource) -> Result<Arc<ImageData>> {
        Ok(Arc::new(
            ImageData::from_rgba("owned-media-cover", 8, 4, vec![160; 128]).unwrap(),
        ))
    }
}

struct Fixture {
    host: Host,
    settings: DesktopSettings,
    media: media::Media,
    adapter: Arc<Adapter>,
    weather: crate::weather::Weather,
    system: crate::system::System,
    clock: ClockSnapshot,
    images: BTreeMap<String, Arc<ImageData>>,
    layout: Layout,
    viewport: (f32, f32),
}
impl Fixture {
    fn new(width: f32, height: f32) -> Self {
        let adapter = Arc::new(Adapter::default());
        let mut settings = DesktopSettings::default();
        settings.layout.visible_widgets = vec!["media".into()];
        settings
            .layout
            .positions
            .insert("media".into(), Placement::new(40., 40.).unwrap());
        settings
            .layout
            .widget_sizes
            .insert("media".into(), WidgetSize::new(width, height).unwrap());
        let fake = Arc::new(crate::test_adapters::Fake::default());
        let mut fixture = Self {
            host: Host::default(),
            settings,
            media: media::Media::new(adapter.clone(), adapter.clone()),
            adapter,
            weather: crate::weather::Weather::new(fake.clone()),
            system: crate::system::System::default(),
            clock: fake.now(),
            images: BTreeMap::from([(
                "symbol:music".into(),
                Arc::new(ImageData::from_rgba("owned-music-symbol", 1, 1, vec![255; 4]).unwrap()),
            )]),
            layout: Layout::new(Arc::new(vec![
                lucent_ui::text::Font::from_bytes(
                    include_bytes!("../../assets/LucentSans.ttf").as_slice(),
                    Default::default(),
                )
                .unwrap(),
            ])),
            viewport: (900., 800.),
        };
        fixture.observe(snapshot());
        fixture
    }
    fn observe(&mut self, snapshot: MediaSnapshot) {
        self.media.update(
            media::Message::Observed(Ok(snapshot)),
            &mut Effects::default(),
        );
    }
    fn view(&self, light: bool) -> View<'_> {
        View {
            host: &self.host,
            input: Input {
                layout: &self.settings.layout,
                notes: &self.settings.notes,
                save_status: Status::Saved,
                clock: &self.clock,
                system: &self.system,
                network_status: "Offline fixture",
                media: &self.media,
                weather: &self.weather,
            },
            artwork: Artwork {
                images: &self.images,
                theme: Theme::new(light),
            },
        }
    }
    fn context(&self) -> ViewContext {
        ViewContext {
            surface: "widgets",
            instance: None,
            width: self.viewport.0,
            height: self.viewport.1,
            now: 1.,
            density: Density::ONE,
        }
    }
    fn scene(&self, input: &Interaction<Message>, light: bool) -> Scene<Message> {
        let cx = self.context();
        self.layout.build(
            &self.view(light).render(&cx),
            cx.width,
            cx.height,
            input,
            cx.now,
        )
    }
    fn native_scene(&self, cx: &ViewContext, input: &Interaction<Message>) -> Scene<Message> {
        self.layout.build(
            &self.view(false).render_native(cx, self.viewport),
            cx.width,
            cx.height,
            input,
            cx.now,
        )
    }
    fn send(&mut self, message: Message) -> Option<Intent> {
        self.host.update(
            message,
            Update {
                layout: &mut self.settings.layout,
                notes: &mut self.settings.notes,
                instance: None,
                viewport: self.viewport,
                unix_seconds: 0,
            },
            &mut Effects::default(),
        )
    }
    fn deliver(&mut self, message: Message) -> Effects<media::Message> {
        let Some(Intent::Media(message)) = self.send(message) else {
            panic!("widget controls must return the selected Media intent");
        };
        let mut effects = Effects::default();
        self.media.update(message, &mut effects);
        effects
    }
    fn artwork(&mut self) {
        let mut snapshot = snapshot();
        let MediaPlayerState::Ready(playback) = &mut snapshot.players[0].state else {
            unreachable!()
        };
        playback.track.as_mut().unwrap().artwork =
            Some(MediaArtworkSource::new("file:///owned/fixture.png").unwrap());
        let mut effects = Effects::default();
        self.media
            .update(media::Message::Observed(Ok(snapshot)), &mut effects);
        assert_eq!(effects.tasks.len(), 1);
        self.media
            .update(effects.tasks.pop().unwrap().run(), &mut effects);
        assert!(self.media.artwork().is_some());
    }
}

fn snapshot() -> MediaSnapshot {
    let mut snapshot = media::fixture();
    snapshot.players[0].id = MediaPlayerId::new(7).unwrap();
    snapshot
}
fn hit<'a>(scene: &'a Scene<Message>, id: &str) -> &'a Hit<Message> {
    scene
        .hits
        .iter()
        .find(|hit| hit.id == id)
        .unwrap_or_else(|| panic!("missing {id}"))
}
fn inside(inner: Rect, outer: Rect) {
    let epsilon = 0.02;
    assert!(inner.w > 0. && inner.h > 0.);
    assert!(
        inner.x + epsilon >= outer.x
            && inner.y + epsilon >= outer.y
            && inner.x + inner.w <= outer.x + outer.w + epsilon
            && inner.y + inner.h <= outer.y + outer.h + epsilon,
        "{inner:?} outside {outer:?}"
    );
}
fn visible_controls(scene: &Scene<Message>) {
    let card = hit(scene, "widget-media").rect;
    for id in ["media-open", "media-previous", "media-play", "media-next"] {
        let target = hit(scene, id);
        inside(target.rect, card);
        inside(target.rect, target.clip.bounds);
        assert!(target.contains(
            target.rect.x + target.rect.w / 2.,
            target.rect.y + target.rect.h / 2.
        ));
    }
}
fn pointer(scene: &Scene<Message>, input: &mut Interaction<Message>, id: &str) -> Message {
    let target = hit(scene, id);
    let x = target.rect.x + target.rect.w / 2.;
    let y = target.rect.y + target.rect.h / 2.;
    assert!(target.contains(x, y));
    assert!(input.press(scene, x, y).is_none());
    input
        .release(x, y)
        .expect("actual pointer release must activate this control")
}
fn keyboard(scene: &Scene<Message>, input: &mut Interaction<Message>, id: &str) -> Message {
    input.synchronize(scene);
    for _ in 0..8 {
        if input.focus.as_deref() == Some(id) {
            break;
        }
        assert!(input.key(scene, &Key::Tab).is_empty());
    }
    assert_eq!(input.focus.as_deref(), Some(id));
    let mut messages = input.key(scene, &Key::Enter);
    assert_eq!(messages.len(), 1);
    messages.pop().unwrap()
}

#[test]
fn resized_cards_use_their_width_and_height_without_clipping_transport() {
    for (width, height) in [(260., 392.), (420., 392.), (260., 560.), (420., 560.)] {
        let fixture = Fixture::new(width, height);
        for light in [false, true] {
            let scene = fixture.scene(&Interaction::default(), light);
            visible_controls(&scene);
            let card = hit(&scene, "widget-media").rect;
            let art = hit(&scene, "media-open").rect;
            assert_eq!((card.w, card.h), (width, height));
            assert_eq!(art.w, width - token::INSET * 2.);
            assert_eq!(
                art.h,
                token::ARTWORK_HEIGHT + height - widget_size("media").1
            );
            assert_eq!(art.x, card.x + token::INSET);
            assert_eq!(art.y, card.y + token::INSET);
            let previous = hit(&scene, "media-previous").rect;
            let play = hit(&scene, "media-play").rect;
            let next = hit(&scene, "media-next").rect;
            assert!((previous.x + previous.w + space::SM - play.x).abs() < 0.02);
            assert!((play.x + play.w + space::SM - next.x).abs() < 0.02);
            assert!(((previous.x + next.x + next.w) / 2. - (card.x + card.w / 2.)).abs() < 0.02);
            assert!((previous.y + previous.h / 2. - play.y - play.h / 2.).abs() < 0.02);
            assert!((next.y + next.h / 2. - play.y - play.h / 2.).abs() < 0.02);
            assert!((card.y + card.h - play.y - play.h - token::INSET).abs() < 0.02);
        }
    }
}

#[test]
fn long_metadata_uses_bounded_rows_and_keeps_the_progress_and_controls_clear() {
    for (width, height) in [(260., 392.), (420., 392.), (260., 560.)] {
        let mut fixture = Fixture::new(width, height);
        let mut observed = snapshot();
        let MediaPlayerState::Ready(playback) = &mut observed.players[0].state else {
            unreachable!()
        };
        let track = playback.track.as_mut().unwrap();
        track.title = "An exceptionally long owned track title ".repeat(15);
        track.artists = vec!["An exceptionally long owned performer name ".repeat(15)];
        fixture.observe(observed);
        let scene = fixture.scene(&Interaction::default(), false);
        visible_controls(&scene);
        let card = hit(&scene, "widget-media").rect;
        let mut rows = 0;
        for paint in &scene.paint {
            if let Paint::Text {
                text,
                rect,
                clip,
                size,
                ..
            } = paint
            {
                assert!(!text.contains('\n'));
                assert!(
                    text.ends_with('…'),
                    "long metadata must show its honest truncation"
                );
                inside(*rect, card);
                inside(*rect, clip.bounds);
                assert!(rect.x >= card.x + token::INSET);
                assert!(rect.x + rect.w <= card.x + card.w - token::INSET + 0.02);
                assert!(
                    rect.y + rect.h
                        <= card.y + token::PROGRESS_TOP + height - widget_size("media").1
                );
                assert!(*size == font::HEADING || *size == font::SMALL);
                rows += 1;
            }
        }
        assert_eq!(rows, 2);
    }
}

#[test]
fn cover_preserves_aspect_and_fallback_stays_centered_in_resized_artwork() {
    for (width, height) in [(260., 392.), (420., 392.), (260., 560.)] {
        let mut fixture = Fixture::new(width, height);
        let fallback = fixture.scene(&Interaction::default(), false);
        let bounds = hit(&fallback, "media-open").rect;
        let rect = fallback
            .paint
            .iter()
            .find_map(|paint| match paint {
                Paint::Image { data, rect, .. } if data.key == "owned-music-symbol" => Some(*rect),
                _ => None,
            })
            .unwrap();
        assert_eq!((rect.w, rect.h), (icon::ARTWORK, icon::ARTWORK));
        assert!((rect.x + rect.w / 2. - bounds.x - bounds.w / 2.).abs() < 0.02);
        assert!((rect.y + rect.h / 2. - bounds.y - bounds.h / 2.).abs() < 0.02);
        fixture.artwork();
        let scene = fixture.scene(&Interaction::default(), false);
        let bounds = hit(&scene, "media-open").rect;
        let (rect, uv, clip, radius) = scene
            .paint
            .iter()
            .find_map(|paint| match paint {
                Paint::Image {
                    data,
                    rect,
                    uv,
                    clip,
                    radius,
                    ..
                } if data.key == "owned-media-cover" => Some((*rect, *uv, clip, *radius)),
                _ => None,
            })
            .unwrap();
        inside(bounds, rect);
        assert!(((rect.w / uv.w) / (rect.h / uv.h) - 2.).abs() < 0.001);
        assert_eq!(clip.bounds, bounds);
        assert_eq!(radius, lucent_design::radius::CARD);
        assert!(!hit(&scene, "media-open").contains(bounds.x, bounds.y));
    }
}

#[test]
fn resized_pointer_and_keyboard_controls_deliver_only_the_selected_player_command() {
    let id = MediaPlayerId::new(7).unwrap();
    for keyboard_input in [false, true] {
        for (name, status, expected) in [
            (
                "media-previous",
                PlaybackStatus::Playing,
                MediaCommand::Previous(id),
            ),
            (
                "media-play",
                PlaybackStatus::Playing,
                MediaCommand::Pause(id),
            ),
            (
                "media-next",
                PlaybackStatus::Playing,
                MediaCommand::Next(id),
            ),
            ("media-play", PlaybackStatus::Paused, MediaCommand::Play(id)),
        ] {
            let mut fixture = Fixture::new(420., 560.);
            let mut observed = snapshot();
            let MediaPlayerState::Ready(playback) = &mut observed.players[0].state else {
                unreachable!()
            };
            playback.status = status;
            fixture.observe(observed);
            let mut input = Interaction::default();
            let scene = fixture.scene(&input, false);
            let message = if keyboard_input {
                keyboard(&scene, &mut input, name)
            } else {
                pointer(&scene, &mut input, name)
            };
            assert!(
                matches!(&message, Message::Media(media::Message::Request(command)) if *command == expected)
            );
            assert!(fixture.adapter.commands.lock().unwrap().is_empty());
            let mut effects = fixture.deliver(message);
            assert_eq!(effects.tasks.len(), 1);
            fixture
                .media
                .update(effects.tasks.pop().unwrap().run(), &mut effects);
            assert_eq!(*fixture.adapter.commands.lock().unwrap(), vec![expected]);
        }
        let mut fixture = Fixture::new(420., 560.);
        let mut input = Interaction::default();
        let scene = fixture.scene(&input, false);
        let message = if keyboard_input {
            keyboard(&scene, &mut input, "media-open")
        } else {
            pointer(&scene, &mut input, "media-open")
        };
        assert!(fixture.deliver(message).tasks.is_empty());
        assert!(fixture.media.open());
        assert!(fixture.adapter.commands.lock().unwrap().is_empty());
    }
}

#[test]
fn disabled_pending_and_removed_players_cannot_issue_transport_commands() {
    for state in 0..3 {
        let mut fixture = Fixture::new(420., 560.);
        let scene = fixture.scene(&Interaction::default(), false);
        let stale = hit(&scene, "media-play").click.clone().unwrap();
        if state == 0 {
            let mut observed = snapshot();
            let MediaPlayerState::Ready(playback) = &mut observed.players[0].state else {
                unreachable!()
            };
            playback.capabilities.can_control = false;
            fixture.observe(observed);
        } else if state == 1 {
            let _pending = fixture.deliver(stale.clone());
        } else {
            fixture.observe(MediaSnapshot::default());
        }
        let disabled = fixture.scene(&Interaction::default(), false);
        assert!(disabled.hits.iter().all(|hit| !matches!(
            hit.id.as_str(),
            "media-play" | "media-previous" | "media-next"
        )));
        inside(
            hit(&disabled, "media-open").rect,
            hit(&disabled, "widget-media").rect,
        );
        assert!(fixture.deliver(stale).tasks.is_empty());
        assert!(fixture.adapter.commands.lock().unwrap().is_empty());
    }
}

#[test]
fn loading_empty_and_unavailable_labels_remain_visible_without_transport_hits() {
    for state in 0..3 {
        let mut fixture = Fixture::new(420., 560.);
        let expected = if state == 0 {
            fixture.media = media::Media::new(fixture.adapter.clone(), fixture.adapter.clone());
            "Connecting to players…"
        } else if state == 1 {
            fixture.observe(MediaSnapshot::default());
            "No media players"
        } else {
            fixture.media.update(
                media::Message::Observed(Err(DomainError::Unavailable("owned outage".into()))),
                &mut Effects::default(),
            );
            "Media unavailable"
        };
        let scene = fixture.scene(&Interaction::default(), false);
        let (rect, clip) = scene
            .paint
            .iter()
            .find_map(|paint| match paint {
                Paint::Text {
                    text, rect, clip, ..
                } if text == expected => Some((*rect, clip)),
                _ => None,
            })
            .expect("the actual availability label is visible");
        inside(rect, clip.bounds);
        assert!(scene.hits.iter().all(|hit| !matches!(
            hit.id.as_str(),
            "media-play" | "media-previous" | "media-next"
        )));
    }
}

#[test]
fn resizing_grows_the_preview_while_native_source_input_stays_frozen_until_release() {
    let mut fixture = Fixture::new(260., 392.);
    fixture.send(Message::Edit(true));
    let sources = fixture
        .host
        .surfaces(&fixture.settings.layout, fixture.viewport);
    let source = &sources[0];
    let cx = ViewContext {
        surface: source.id,
        width: source.rect.w,
        height: source.rect.h,
        ..fixture.context()
    };
    let mut input = Interaction::default();
    let before = fixture.native_scene(&cx, &input);
    let grip = hit(&before, "widget-resize-media");
    let (x, y) = (
        grip.rect.x + grip.rect.w / 2.,
        grip.rect.y + grip.rect.h / 2.,
    );
    assert!(grip.contains(x, y));
    fixture.send(Message::Press); // The root's existing prelude precedes SDK capture.
    assert!(input.press(&before, x, y).is_none());
    let saved = fixture.settings.layout.widget_sizes.clone();
    let messages = input.motion(&before, x + 156., y + 168., 1.);
    assert_eq!(messages.len(), 1);
    for message in messages {
        assert!(fixture.send(message).is_none());
    }
    assert!(fixture.host.grid_visible());
    assert_eq!(
        fixture
            .host
            .surfaces(&fixture.settings.layout, fixture.viewport),
        sources
    );
    assert_eq!(fixture.settings.layout.widget_sizes, saved);
    let during = fixture.native_scene(&cx, &input);
    assert!(during.paint.is_empty());
    assert_eq!(before.regions, during.regions);
    for id in [
        "media-open",
        "media-previous",
        "media-play",
        "media-next",
        "widget-resize-media",
    ] {
        assert_eq!(hit(&before, id).rect, hit(&during, id).rect);
    }
    let preview = fixture.scene(&Interaction::default(), false);
    visible_controls(&preview);
    assert_eq!(hit(&preview, "widget-media").rect.w, 416.);
    assert_eq!(
        hit(&preview, "media-open").rect.h,
        token::ARTWORK_HEIGHT + 168.
    );
    assert_eq!(hit(&preview, "media-open").rect.w, 416. - token::INSET * 2.);
    input.synchronize(&during);
    assert!(matches!(
        fixture.send(input.release(x + 156., y + 168.).unwrap()),
        Some(Intent::Save)
    ));
    assert_eq!(
        fixture.settings.layout.widget_sizes["media"].tuple(),
        (416., 560.)
    );
    assert!(!fixture.host.grid_visible());
    let restored: DesktopSettings =
        serde_json::from_value(serde_json::to_value(&fixture.settings).unwrap()).unwrap();
    assert_eq!(
        restored.layout.widget_sizes,
        fixture.settings.layout.widget_sizes
    );
    let settled_sources = fixture
        .host
        .surfaces(&fixture.settings.layout, fixture.viewport);
    assert_ne!(settled_sources[0].rect, sources[0].rect);
    let settled_cx = ViewContext {
        surface: settled_sources[0].id,
        width: settled_sources[0].rect.w,
        height: settled_sources[0].rect.h,
        ..fixture.context()
    };
    let settled = fixture.native_scene(&settled_cx, &input);
    assert!(!settled.paint.is_empty());
    visible_controls(&settled);
    let card = hit(&settled, "widget-media").rect;
    let art = hit(&settled, "media-open").rect;
    let play = hit(&settled, "media-play").rect;
    assert_eq!(card.w, 416.);
    assert_eq!(art.w, 416. - token::INSET * 2.);
    assert_eq!(art.h, token::ARTWORK_HEIGHT + 168.);
    assert!((play.x + play.w / 2. - card.x - card.w / 2.).abs() < 0.02);
}

#[test]
fn cancelled_resize_keeps_saved_media_size_and_retires_the_old_release() {
    let mut fixture = Fixture::new(260., 392.);
    fixture.send(Message::Edit(true));
    fixture.send(Message::Press);
    let before = fixture.settings.layout.widget_sizes.clone();
    assert!(
        fixture
            .send(Message::Resize(
                "media".into(),
                DragEvent {
                    dx: 156.,
                    dy: 168.,
                    finished: false
                }
            ))
            .is_none()
    );
    fixture.send(Message::CancelDrags);
    assert!(
        fixture
            .send(Message::Resize(
                "media".into(),
                DragEvent {
                    dx: 156.,
                    dy: 168.,
                    finished: true
                }
            ))
            .is_none()
    );
    assert_eq!(fixture.settings.layout.widget_sizes, before);
    let scene = fixture.scene(&Interaction::default(), false);
    visible_controls(&scene);
    assert_eq!(hit(&scene, "media-open").rect.w, 260. - token::INSET * 2.);
}

#[test]
fn output_constraints_reflow_media_without_rewriting_the_saved_preference() {
    let mut fixture = Fixture::new(420., 560.);
    let saved = fixture.settings.layout.widget_sizes.clone();
    fixture.viewport = (300., 420.);
    let small = fixture.scene(&Interaction::default(), false);
    visible_controls(&small);
    let card = hit(&small, "widget-media").rect;
    assert_eq!((card.w, card.h), fixture.viewport);
    inside(
        card,
        Rect::new(0., 0., fixture.viewport.0, fixture.viewport.1),
    );
    assert_eq!(fixture.settings.layout.widget_sizes, saved);
    fixture.viewport = (900., 800.);
    let restored = fixture.scene(&Interaction::default(), false);
    assert_eq!(
        (
            hit(&restored, "widget-media").rect.w,
            hit(&restored, "widget-media").rect.h
        ),
        (420., 560.)
    );
    visible_controls(&restored);
}
