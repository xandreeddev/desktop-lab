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
    ) -> Element<Message> {
        let selected = self
            .target
            .as_ref()
            .and_then(|id| settings.monitors.get(id))
            .or(settings.default.as_ref());
        let label = |text: &str| Element::text(text).font(font::BODY).color(theme.on_surface);
        let button = |text: &str, message: Message| theme.button(text, message);
        let selected_button = |text: &str, message: Message, selected: bool| {
            let control = button(text, message);
            if selected {
                control.background(theme.primary).color(theme.on_primary)
            } else {
                control
            }
        };
        let mut targets = vec![
            selected_button(
                "Default wallpaper",
                Message::Target(None),
                self.target.is_none(),
            )
            .id("live-target-all"),
        ];
        for monitor in monitors {
            targets.push(
                selected_button(
                    monitor.as_str(),
                    Message::Target(Some(monitor.clone())),
                    self.target.as_ref() == Some(monitor),
                )
                .id(format!("live-target-{}", monitor.as_str())),
            );
        }
        let rates = [15, 30, 60]
            .into_iter()
            .map(|fps| {
                selected_button(
                    &format!("{fps} FPS"),
                    Message::Rate(WallpaperFps::new(fps).unwrap()),
                    selected.map_or(30, |setting| setting.fps.value()) == fps,
                )
                .id(format!("live-fps-{fps}"))
            })
            .collect();
        let panel = Element::column(vec![
            Element::row(vec![
                label("Live wallpapers").width(Length::Fill),
                button("Close", Message::Close).id("live-close"),
            ])
            .gap(space::SM),
            label("Local MP4, WebM or animated GIF · muted, looped playback")
                .width(Length::Fill)
                .wrap(component::live_wallpaper::STATUS_LINES as usize),
            Element::text_input(
                self.path.clone(),
                self.field.selection(),
                "Absolute path to a media file",
                Message::EditPath,
            )
            .on_submit(Message::Import)
            .font(font::BODY)
            .color(theme.on_surface)
            .background(theme.surface_container)
            .padding(space::MD)
            .radius(radius::CONTROL)
            .width(Length::Fill)
            .height(Length::Fixed(component::live_wallpaper::INPUT_HEIGHT))
            .id("live-path"),
            Element::column(targets).gap(space::SM),
            Element::column(vec![
                button("Import and play", Message::Import).id("live-import"),
                button(
                    if settings.paused { "Resume" } else { "Pause" },
                    Message::Pause(!settings.paused),
                )
                .id("live-pause"),
                button("Use static wallpaper", Message::Off).id("live-off"),
            ])
            .gap(space::SM),
            Element::row(rates).gap(space::SM),
            label(&format!(
                "Target: {} · {} FPS",
                self.target
                    .as_ref()
                    .map_or("Default wallpaper", MonitorId::as_str),
                selected.map_or(30, |s| s.fps.value())
            ))
            .width(Length::Fill)
            .wrap(component::live_wallpaper::STATUS_LINES as usize),
            label(self.status())
                .width(Length::Fill)
                .wrap(component::live_wallpaper::STATUS_LINES as usize),
        ])
        .gap(space::MD)
        .padding(space::LG)
        .width(Length::Fixed(
            component::live_wallpaper::PANEL_WIDTH.min((cx.width - space::LG * 2.).max(space::LG)),
        ))
        .radius(radius::PANEL)
        .background(theme.surface);
        let height = component::live_wallpaper::PANEL_HEIGHT
            .min((cx.height - space::LG * 2.).max(space::LG));
        let width =
            component::live_wallpaper::PANEL_WIDTH.min((cx.width - space::LG * 2.).max(space::LG));
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
            .background(theme.surface);
        let amount = self.motion.value(cx.now).clamp(0., 1.);
        Element::stack(vec![panel.opacity(amount).at(
            (cx.width - width) / 2.,
            (cx.height - height) / 2. + space::LG * (1. - amount),
        )])
        .fill()
        .background(Color::TRANSPARENT)
    }
}
