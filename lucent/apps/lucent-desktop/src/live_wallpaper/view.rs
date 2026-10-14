use super::*;
use lucent_api::{Color, Element, Length, ScrollSpec, ViewContext};
use lucent_design::{component, font, radius, space};
impl LiveWallpaper {
    pub fn view(
        &self,
        cx: &ViewContext,
        settings: &LiveWallpapers,
        monitors: &[MonitorId],
        theme: lucent_design::Theme,
        save: SaveState<'_>,
    ) -> Element<Message> {
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
        let mut children = vec![
            Element::row(vec![
                label("Live wallpapers").width(Length::Fill),
                button("Close", Message::Close).id("live-close"),
            ])
            .gap(space::SM),
            label("Local MP4, WebM or animated GIF · muted, looped playback")
                .width(Length::Fill)
                .wrap(component::live_wallpaper::STATUS_LINES as usize),
            theme
                .text_input(
                    self.path.clone(),
                    self.field.selection(),
                    "Absolute path to a media file",
                    Message::EditPath,
                )
                .on_submit(Message::Import(self.target.clone()))
                .autofocus()
                .height(Length::Fixed(component::live_wallpaper::INPUT_HEIGHT))
                .id("live-path"),
            Element::column(targets).gap(space::SM),
        ];
        if save.writable {
            children.push(
                button(
                    if settings.paused {
                        "Import wallpaper"
                    } else {
                        "Import and play"
                    },
                    Message::Import(self.target.clone()),
                )
                .id("live-import"),
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
                let rates = [15, 30, 60]
                    .into_iter()
                    .map(|fps| {
                        theme
                            .choice_button(
                                format!("{fps} FPS"),
                                Message::Rate(self.target.clone(), WallpaperFps::new(fps).unwrap()),
                                selected.is_some_and(|setting| setting.fps.value() == fps),
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
        }
        let target_label = self
            .target
            .monitor()
            .map_or("Default wallpaper", MonitorId::as_str);
        children.push(
            label(&format!(
                "Target: {target_label} · {} FPS",
                selected.map_or(30, |profile| profile.fps.value())
            ))
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
        let width =
            component::live_wallpaper::PANEL_WIDTH.min((cx.width - space::LG * 2.).max(space::LG));
        let height = component::live_wallpaper::PANEL_HEIGHT
            .min((cx.height - space::LG * 2.).max(space::LG));
        let panel = Element::column(children)
            .gap(space::MD)
            .padding(space::LG)
            .width(Length::Fixed(width))
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
            .tab_stop(false);
        let amount = self.motion.value(cx.now).clamp(0., 1.);
        Element::stack(vec![
            Element::empty()
                .fill()
                .on_click(Message::Close)
                .tab_stop(false)
                .id("live-backdrop"),
            panel.opacity(amount).at(
                (cx.width - width) / 2.,
                (cx.height - height) / 2. + space::LG * (1. - amount),
            ),
        ])
        .fill()
        .background(Color::TRANSPARENT)
    }
}
