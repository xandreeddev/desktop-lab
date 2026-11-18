//! Responsive widget references share the owned media/artwork adapters used by
//! the panel fixtures, while exercising the actual widget host composition.
use super::*;
use lucent_domain::{MediaArtworkSource, MediaPlayerState, Placement, WidgetSize};

pub(super) fn prepare(
    app: &mut Desktop,
    name: &str,
    layout: &Layout,
    width: f32,
    height: f32,
    now: f64,
) -> Scene<Message> {
    use crate::{media::Message as Media, widgets::Message as Widget};
    open_media_fixture(
        app,
        match name {
            "media-widget-fallback" => "media-empty",
            "media-widget-pending" => "media-pending",
            "media-widget-tall" => "media-paused",
            _ => "media-playing",
        },
    );
    app.update(Message::Media(Media::Close), &mut Effects::default());
    if name == "media-widget-wide-long" {
        let mut snapshot = crate::media::fixture();
        let MediaPlayerState::Ready(playback) = &mut snapshot.players[0].state else {
            unreachable!("owned fixture has ready playback");
        };
        let track = playback.track.as_mut().unwrap();
        track.title =
            "A long track title that should use the wider card before it ends with an ellipsis"
                .into();
        track.artists = vec![
            "A long artist name with collaborators and additional metadata that stays inside its row"
                .into(),
        ];
        track.artwork = Some(MediaArtworkSource::new(MEDIA_ARTWORK_SOURCE).unwrap());
        let mut effects = Effects::default();
        app.update(Message::Media(Media::Observed(Ok(snapshot))), &mut effects);
        assert!(effects.tasks.is_empty(), "unchanged artwork stays prepared");
    }
    app.settings.layout.visible_widgets = vec!["media".into()];
    app.settings
        .layout
        .positions
        .insert("media".into(), Placement::new(64., 64.).unwrap());
    let size = match name {
        "media-widget-wide-long" | "media-widget-fallback" | "media-widget-pending" => (420., 392.),
        "media-widget-tall" => (260., 560.),
        _ => (260., 392.),
    };
    if size != (260., 392.) {
        app.settings
            .layout
            .widget_sizes
            .insert("media".into(), WidgetSize::new(size.0, size.1).unwrap());
    }
    if name == "media-widget-resize-preview" {
        let mut effects = Effects::default();
        app.update(Message::Widgets(Widget::Edit(true)), &mut effects);
        app.update(Message::Widgets(Widget::Press), &mut effects);
        app.update(
            Message::Widgets(Widget::Resize(
                "media".into(),
                DragEvent {
                    dx: 160.,
                    dy: 168.,
                    finished: false,
                },
            )),
            &mut effects,
        );
        assert!(app.widgets.grid_visible());
        assert!(app.settings.layout.widget_sizes.is_empty());
        assert_eq!(
            app.widgets
                .size("media", &app.settings.layout, app.viewport, true)
                .tuple(),
            (260., 392.)
        );
        assert_eq!(
            app.widgets
                .size("media", &app.settings.layout, app.viewport, false)
                .tuple(),
            (420., 560.)
        );
    }
    let cx = ViewContext {
        surface: "widgets",
        instance: None,
        width,
        height,
        now,
        density: Density::ONE,
    };
    let scene = layout.build(&app.view(&cx), width, height, &Interaction::default(), now);
    for id in ["media-open", "media-previous", "media-play", "media-next"] {
        let hit = scene.hits.iter().find(|hit| hit.id == id);
        if id != "media-open" && matches!(name, "media-widget-fallback" | "media-widget-pending") {
            assert!(hit.is_none(), "unavailable transport has no input target");
            continue;
        }
        let hit = hit.expect("available widget control");
        assert!(hit.rect.x >= 0. && hit.rect.y >= 0.);
        assert!(hit.rect.x + hit.rect.w <= width && hit.rect.y + hit.rect.h <= height);
        assert!(hit.contains(hit.rect.x + hit.rect.w / 2., hit.rect.y + hit.rect.h / 2.));
    }
    scene
}
