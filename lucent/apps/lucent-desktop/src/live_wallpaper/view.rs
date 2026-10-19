use super::*;
use lucent_api::{Color, Element, Length, ScrollSpec, ViewContext};
use lucent_design::{component, font, radius, space};

fn carousel_width(panel_width: f32) -> f32 {
    // Match the controlled scroll viewport and the content's token padding.
    // A fixed panel-wide child would overflow the scrollbar gutter.
    (panel_width - component::scroll::PADDING * 2. - component::scroll::GUTTER - space::LG * 2.)
        .max(0.)
}
/// Logical image demand shared by the controller and its actual presentation.
/// Density conversion and checked physical preview bounds belong to the caller.
pub(super) fn preview_size(viewport: (f32, f32)) -> (f32, f32) {
    let bounds = crate::shell_layout::live_wallpaper_panel(viewport);
    crate::wallpaper_carousel::hero_size(carousel_width(bounds.w))
}

impl LiveWallpaper {
    pub fn view(
        &self,
        cx: &ViewContext,
        settings: &LiveWallpapers,
        monitors: &[MonitorId],
        artwork: crate::artwork::Artwork<'_>,
        save: SaveState<'_>,
    ) -> Element<Message> {
        use crate::wallpaper_carousel::{Action, Card, Carousel, Controls, visible_wallpapers};
        let theme = artwork.theme;
        let bounds = crate::shell_layout::live_wallpaper_panel((cx.width, cx.height));
        let (width, height) = (bounds.w, bounds.h);
        let carousel_width = carousel_width(width);
        let selected = settings.selected(&self.target);
        let label = |text: &str| Element::text(text).font(font::BODY).color(theme.on_surface);
        let button = |text: &str, message: Message| theme.button(text, message);
        let mut targets = vec![
            theme
                .choice_button(
                    "Default wallpaper",
                    Message::Target(LiveWallpaperTarget::Default),
                    self.target == LiveWallpaperTarget::Default,
                )
                .id("live-target-all"),
        ];
        for monitor in monitors {
            let target = LiveWallpaperTarget::Monitor(monitor.clone());
            targets.push(
                theme
                    .choice_button(
                        monitor.as_str(),
                        Message::Target(target.clone()),
                        self.target == target,
                    )
                    .id(format!("live-target-{}", monitor.as_str())),
            );
        }
        let center = self.carousel.value(cx.now);
        let cards = visible_wallpapers(self.library.len(), center)
            .map(|index| {
                let source = &self.library[index];
                Card {
                    index,
                    caption: &source.name,
                    image: self.preview.frame_for(&source.path).cloned(),
                    unavailable: index == self.selection && !self.preview.error().is_empty(),
                    select: Message::SelectSource(index),
                    id: format!("live-library-{index}"),
                }
            })
            .collect();
        let applied = self.library.get(self.selection).is_some_and(|source| {
            selected
                .is_some_and(|profile| profile.enabled && profile.asset.source.path == source.path)
        });
        let carousel = Carousel {
            cards,
            center,
            artwork,
            controls: Controls {
                previous: ("live-previous", Message::Browse(-1)),
                next: ("live-next", Message::Browse(1)),
                apply: Action {
                    label: if applied {
                        "Applied"
                    } else {
                        "Apply wallpaper"
                    },
                    message: Message::ApplySelection,
                    enabled: save.writable && self.importing.is_none(),
                    id: "live-apply",
                },
            },
            empty: "No local live wallpapers",
        }
        .render(
            carousel_width,
            crate::wallpaper_carousel::preferred_height(),
        );
        let mut children = vec![
            Element::row(vec![
                label("Live wallpapers").width(Length::Fill),
                button("Close", Message::Close).id("live-close"),
            ])
            .gap(space::SM),
            Element::row(vec![
                label("Local library").width(Length::Fill),
                button("Refresh", Message::RefreshLibrary)
                    .autofocus()
                    .id("live-library-refresh"),
            ])
            .gap(space::SM),
            carousel,
            label("Local MP4, WebM or animated GIF · muted, looped playback")
                .width(Length::Fill)
                .wrap(component::live_wallpaper::STATUS_LINES as usize),
        ];
        match self.preview.status() {
            super::preview::Status::Idle => {}
            super::preview::Status::Loading => {
                children.push(theme.status_text("Preparing preview…", false));
            }
            super::preview::Status::Ready => {
                children.push(theme.status_text("Previewing selection", false));
            }
            super::preview::Status::Failed => {
                children.push(
                    theme
                        .status_text(self.preview.error(), true)
                        .wrap(component::live_wallpaper::STATUS_LINES as usize),
                );
                children
                    .push(button("Retry preview", Message::RetryPreview).id("live-preview-retry"));
            }
        }
        if self.importing.is_some() {
            children.push(theme.status_text("Importing and decoding…", false));
        } else if !self.error.is_empty() {
            children.push(
                theme
                    .status_text(&self.error, true)
                    .wrap(component::live_wallpaper::STATUS_LINES as usize),
            );
        }
        if self.library_loading {
            children.push(theme.status_text("Looking for local loops…", false));
        } else if !self.library_error.is_empty() {
            children.push(
                theme
                    .status_text(&self.library_error, true)
                    .wrap(component::live_wallpaper::STATUS_LINES as usize),
            );
        } else if self.library.is_empty() {
            children.push(
                theme
                    .status_text("Add clips to ~/Videos/Wallpapers, then Refresh", false)
                    .wrap(component::live_wallpaper::STATUS_LINES as usize),
            );
        }
        children.extend([
            label("Or import another local file"),
            theme
                .text_input(
                    self.path.clone(),
                    self.field.selection(),
                    "Absolute path to a media file",
                    Message::EditPath,
                )
                .on_submit(Message::Import(self.target.clone()))
                .id("live-path"),
            Element::column(targets).gap(space::SM),
        ]);
        if save.writable {
            children.push(
                button("Import wallpaper", Message::Import(self.target.clone())).id("live-import"),
            );
            if let Some(profile) = selected.filter(|profile| profile.enabled) {
                children.push(
                    button(
                        if profile.paused {
                            "Resume this profile"
                        } else {
                            "Pause this profile"
                        },
                        Message::Pause(self.target.clone(), !profile.paused),
                    )
                    .id("live-pause"),
                );
            }
            children.push(
                button("Use static wallpaper", Message::Off(self.target.clone())).id("live-off"),
            );
            if let LiveWallpaperTarget::Monitor(id) = &self.target
                && settings.monitors.contains_key(id)
            {
                children.push(
                    button("Use default wallpaper", Message::Inherit(id.clone()))
                        .id("live-inherit"),
                );
            }
            if selected.is_some() {
                children.push(label("Frame rate"));
                children.push(
                    theme
                        .choice_button(
                            "Source frame rate",
                            Message::Rate(self.target.clone(), WallpaperFrameRate::Source),
                            selected
                                .is_some_and(|setting| setting.fps == WallpaperFrameRate::Source),
                        )
                        .width(Length::Fill)
                        .id("live-fps-source"),
                );
                let rates = [15, 30, 60]
                    .into_iter()
                    .map(|fps| {
                        theme
                            .choice_button(
                                format!("{fps} FPS"),
                                Message::Rate(
                                    self.target.clone(),
                                    WallpaperFps::new(fps).unwrap().into(),
                                ),
                                selected.is_some_and(|setting| {
                                    setting.fps.cap().is_some_and(|cap| cap.value() == fps)
                                }),
                            )
                            .id(format!("live-fps-{fps}"))
                    })
                    .collect();
                children.push(Element::row(rates).gap(space::SM));
            }
            if self.retry_available(settings, &self.target) {
                children.push(
                    button("Retry playback", Message::Retry(self.target.clone())).id("live-retry"),
                );
            }
            children.push(
                theme
                    .toggle_row(
                        "Pause all live wallpapers",
                        Message::PauseAll(!settings.paused),
                        settings.paused,
                    )
                    .id("live-pause-all"),
            );
            for (label, condition, enabled, id) in [
                (
                    "Pause on battery",
                    WallpaperPauseCondition::Battery,
                    settings.policy.pause_on_battery,
                    "live-policy-battery",
                ),
                (
                    "Pause in fullscreen",
                    WallpaperPauseCondition::Fullscreen,
                    settings.policy.pause_when_fullscreen,
                    "live-policy-fullscreen",
                ),
            ] {
                children.push(
                    theme
                        .toggle_row(label, Message::Policy(condition, !enabled), enabled)
                        .id(id),
                );
            }
        }
        let target_label = self
            .target
            .monitor()
            .map_or("Default wallpaper", MonitorId::as_str);
        let frame_rate = selected.map_or(WallpaperFrameRate::Source, |profile| profile.fps);
        let rate_label = frame_rate.cap().map_or_else(
            || "Source frame rate".to_string(),
            |cap| format!("{} FPS limit", cap.value()),
        );
        children.push(
            label(&format!("Target: {target_label} · {rate_label}",))
                .width(Length::Fill)
                .wrap(component::live_wallpaper::STATUS_LINES as usize),
        );
        if self
            .target
            .monitor()
            .is_some_and(|id| !settings.monitors.contains_key(id))
        {
            children.push(
                theme
                    .status_text("Following the default wallpaper", false)
                    .wrap(component::live_wallpaper::STATUS_LINES as usize),
            );
        }
        children.push(
            theme
                .status_text(
                    self.selected_status(settings, &self.target),
                    !self.error.is_empty() || self.retry_available(settings, &self.target),
                )
                .wrap(component::live_wallpaper::STATUS_LINES as usize),
        );
        use crate::persistence::Status;
        let save_label = match save.status {
            Status::Unavailable => "Saved preferences are unavailable",
            Status::Saving => "Saving preferences…",
            Status::Retrying => "Retrying preference save…",
            Status::Saved => "Preferences saved",
            Status::Failed => "Preference save failed",
        };
        children.push(
            theme
                .status_text(
                    save_label,
                    matches!(save.status, Status::Failed | Status::Unavailable),
                )
                .wrap(component::live_wallpaper::STATUS_LINES as usize),
        );
        if !save.error.is_empty() {
            children.push(
                theme
                    .status_text(save.error, true)
                    .wrap(component::live_wallpaper::STATUS_LINES as usize),
            );
        }
        if save.writable && save.status == Status::Failed {
            children
                .push(button("Retry saving preferences", Message::RetrySave).id("live-save-retry"));
        }
        let panel = Element::column(children)
            .gap(space::MD)
            .padding(space::LG)
            .width(Length::Fill)
            .radius(radius::PANEL)
            .background(theme.surface);
        let panel = theme
            .scroll(
                ScrollSpec::new(self.scroll, component::scroll::LINE_STEP)
                    .expect("validated controlled scroll"),
                panel,
                Message::Scroll,
            )
            .id("live-scroll")
            .size(width, height)
            .radius(radius::PANEL)
            .background(theme.surface)
            .on_click(Message::Ignore)
            .hover_feedback(false)
            .tab_stop(false);
        let amount = self.motion.value(cx.now).clamp(0., 1.);
        Element::stack(vec![
            Element::empty()
                .fill()
                .on_click(Message::Close)
                .tab_stop(false)
                .id("live-backdrop"),
            panel.diagonal_reveal(amount).at(bounds.x, bounds.y),
        ])
        .fill()
        .background(Color::TRANSPARENT)
    }
}
