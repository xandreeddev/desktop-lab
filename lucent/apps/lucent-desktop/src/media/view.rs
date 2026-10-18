//! Views borrow the selected observation and prepared pixels; no native I/O.
use super::*;
use lucent_api::*;
use lucent_design::{Theme, component::media_panel as token, font, radius, space};

fn time(value: MediaTime) -> String {
    let seconds = value.micros() / 1_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
impl Media {
    fn control(
        &self,
        label: &str,
        id: &str,
        command: MediaCommand,
        theme: Theme,
    ) -> Element<Message> {
        if self.admits(&command) && self.pending.is_none() {
            theme.button(label, Message::Request(command)).id(id)
        } else {
            Element::text(label)
                .font(font::CONTROL)
                .color(theme.on_surface)
                .padding(space::MD)
                .opacity(lucent_design::opacity::DISABLED)
        }
    }
    pub fn view(&self, cx: &ViewContext, theme: Theme) -> Element<Message> {
        let mut content = vec![];
        let mut controls = vec![];
        if !self.observation_error.is_empty() {
            content.push(
                theme
                    .status_text(
                        "Media service unavailable · showing the last observation",
                        true,
                    )
                    .wrap(3),
            );
            content.push(theme.status_text(&self.observation_error, true).wrap(3));
        }
        if !self.observed || self.snapshot.players.is_empty() {
            content.push(
                Element::text(self.label())
                    .font(font::BODY)
                    .color(theme.on_surface)
                    .wrap(3),
            );
        }
        for player in &self.snapshot.players {
            content.push(
                theme
                    .choice_button(
                        &player.name,
                        Message::Select(player.id),
                        self.selected == Some(player.id),
                    )
                    .id(format!("media-player-{}", player.id.value()))
                    .width(Length::Fill),
            );
        }
        if let Some(player) = self.selected() {
            if let MediaPlayerState::Unavailable(error) = &player.state {
                content.push(theme.status_text(error, true).wrap(3));
            }
            if let Some(playback) = self.playback() {
                if let Some(image) = self.artwork() {
                    content.push(
                        Element::image(image.clone())
                            .size(token::ARTWORK, token::ARTWORK)
                            .cover()
                            .radius(radius::CARD),
                    );
                }
                content.push(
                    Element::text(playback.track.as_ref().map_or(self.label(), |track| {
                        if track.title.is_empty() {
                            "Untitled track"
                        } else {
                            track.title.as_str()
                        }
                    }))
                    .font(font::TITLE)
                    .color(theme.on_surface)
                    .wrap(3),
                );
                content.push(
                    Element::text(self.artist())
                        .font(font::BODY)
                        .color(theme.on_surface)
                        .wrap(2),
                );
                if let Some(track) = &playback.track
                    && !track.album.is_empty()
                {
                    content.push(theme.status_text(&track.album, false).wrap(2));
                }
                if let Some((fraction, position, length)) = self.progress() {
                    content.push(
                        theme.status_text(format!("{} / {}", time(position), time(length)), false),
                    );
                    if let Some(target) = self.seek_target() {
                        content.push(
                            theme
                                .slider(
                                    SliderSpec::new(fraction, 0.01).expect("normalized progress"),
                                    move |event| Message::Seek(target, event),
                                )
                                .id(format!(
                                    "media-seek-{}-{}-{}",
                                    target.player.value(),
                                    target.track.value(),
                                    target.epoch
                                )),
                        );
                    }
                }
                let toggle = if playback.status == PlaybackStatus::Playing {
                    self.control(
                        "Pause",
                        "media-pause",
                        MediaCommand::Pause(player.id),
                        theme,
                    )
                } else {
                    self.control("Play", "media-play", MediaCommand::Play(player.id), theme)
                };
                controls.push(
                    Element::row(vec![
                        self.control(
                            "Previous",
                            "media-previous",
                            MediaCommand::Previous(player.id),
                            theme,
                        ),
                        toggle,
                        self.control("Next", "media-next", MediaCommand::Next(player.id), theme),
                    ])
                    .width(Length::Fill)
                    .gap(space::SM),
                );
                controls.push(
                    Element::row(vec![
                        self.control("Stop", "media-stop", MediaCommand::Stop(player.id), theme),
                        self.control(
                            "Open player",
                            "media-raise",
                            MediaCommand::Raise(player.id),
                            theme,
                        ),
                    ])
                    .width(Length::Fill)
                    .gap(space::SM),
                );
            }
        }
        if !self.artwork_error.is_empty() {
            content.push(theme.status_text("Artwork could not be loaded", true));
            content.push(
                theme
                    .button("Retry artwork", Message::RetryArtwork)
                    .id("media-artwork-retry"),
            );
        }
        if !self.error.is_empty() {
            content.push(theme.status_text(&self.error, true).wrap(3));
        }
        if self.pending.is_some() {
            content.push(theme.status_text("Sending media command…", false));
        }
        let rect = crate::shell_layout::media_panel((cx.width, cx.height));
        let progress = self.motion.value(cx.now);
        let body = theme
            .scroll(
                ScrollSpec::new(self.scroll, lucent_design::component::scroll::LINE_STEP).unwrap(),
                Element::column(content).width(Length::Fill).gap(space::MD),
                Message::Scroll,
            )
            .id("media-scroll");
        let mut children = vec![
            Element::row(vec![
                Element::text("Media")
                    .font(font::TITLE)
                    .color(theme.on_surface)
                    .width(Length::Fill),
                theme.button("Close", Message::Close).id("media-close"),
            ])
            .width(Length::Fill)
            .align(Align::Center)
            .gap(space::SM),
            body,
        ];
        if !controls.is_empty() {
            children.push(
                Element::column(controls)
                    .width(Length::Fill)
                    .height(Length::Shrink)
                    .gap(space::MD),
            );
        }
        let panel = Element::column(children)
            .size(rect.w, rect.h)
            .padding(space::XL)
            .gap(space::MD)
            .background(theme.surface)
            .radius(radius::PANEL)
            .shadow()
            .clip()
            .at(rect.x, rect.y - (1. - progress) * token::SLIDE_DISTANCE)
            .opacity(progress)
            .on_click(Message::Ignore)
            .hover_feedback(false)
            .tab_stop(false)
            .id("media-panel");
        Element::stack(vec![
            Element::empty()
                .fill()
                .on_click(Message::Close)
                .tab_stop(false)
                .id("media-backdrop"),
            panel,
        ])
        .fill()
    }
}
