//! Selector rendering consumes borrowed domain snapshots and decoded artwork.
use super::*;
use crate::{artwork::Artwork, shell_layout};
use lucent_design::{self as design, *};
pub struct View<'a> {
    pub launcher: &'a Launcher,
    pub input: Input<'a>,
    pub artwork: Artwork<'a>,
    pub previews: crate::previews::View<'a>,
    pub carousel: Motion,
    pub current_wallpaper: &'a str,
    pub error: &'a str,
    pub theme_error: &'a str,
}
impl View<'_> {
    pub fn render(&self, cx: &ViewContext, width: f32, height: f32) -> Element<Message> {
        self.launcher_face(cx, width, height)
    }

    fn theme(&self) -> design::Theme {
        self.artwork.theme
    }
    fn surface_color(&self) -> Color {
        self.theme().surface
    }
    fn widget_color(&self) -> Color {
        self.theme().surface_container
    }
    fn ink(&self) -> Color {
        self.theme().on_surface
    }
    fn accent(&self) -> Color {
        self.theme().primary
    }
    fn button(&self, label: impl Into<String>, message: Message) -> Element<Message> {
        self.theme().button(label, message)
    }
    fn label(&self, text: impl Into<String>, size: f32) -> Element<Message> {
        Element::text(text).font(size).color(self.ink())
    }
    fn icon(&self, name: &str, size: f32) -> Element<Message> {
        self.artwork.icon(name, size)
    }
    fn icon_button(&self, id: &str, name: &str, message: Message) -> Element<Message> {
        self.artwork.icon_button(id, name, message)
    }
    fn app_icon(&self, app: &lucent_domain::Application, size: f32) -> Element<Message> {
        self.artwork.app_icon(app, size)
    }
    fn launcher_face(&self, cx: &ViewContext, width: f32, height: f32) -> Element<Message> {
        let mut modes = Vec::new();
        for (mode, icon) in [
            (Mode::Apps, "apps"),
            (Mode::Commands, "command"),
            (Mode::Wallpapers, "wallpaper"),
            (Mode::Themes, "palette"),
            (Mode::Widgets, "widgets"),
        ] {
            let active = mode == self.launcher.mode;
            let mut contents = vec![self.icon(icon, icon::TINY).tint(if active {
                self.theme().on_primary
            } else {
                self.ink()
            })];
            if active && width >= component::launcher::TAB_LABEL_MIN_WIDTH {
                contents.push(
                    self.label(mode.name(), font::CAPTION)
                        .color(self.theme().on_primary),
                );
            }
            modes.push(
                Element::row(contents)
                    .align(Align::Center)
                    .gap(space::SM)
                    .padding(space::SM)
                    .size(
                        if active && width >= component::launcher::TAB_LABEL_MIN_WIDTH {
                            component::launcher::ACTIVE_TAB_WIDTH
                        } else {
                            component::launcher::TAB_WIDTH
                        },
                        component::launcher::TAB_HEIGHT,
                    )
                    .radius(radius::CONTROL)
                    .background(if active {
                        self.accent()
                    } else {
                        self.surface_color()
                    })
                    .on_click(Message::Mode(mode))
                    .id(format!("mode-{}", mode.name().to_lowercase())),
            );
        }
        modes.push(Element::empty().width(Length::Fill));
        modes.push(
            self.icon("power", icon::SMALL)
                .padding(space::XS)
                .size(
                    component::launcher::ICON_WIDTH,
                    component::launcher::TAB_HEIGHT,
                )
                .on_click(Message::Mode(Mode::Commands))
                .id("mode-power"),
        );
        let header = Element::row(modes)
            .gap(space::SM)
            .size(width, component::launcher::HEADER_HEIGHT);
        let mut elements = vec![header];
        let body_y = shell_layout::body_top();
        let body_h = shell_layout::body_height(self.launcher.mode, height);
        let visible_rows = (body_h / component::launcher::ROW_HEIGHT).floor().max(1.) as usize;
        match self.launcher.mode {
            Mode::Apps => {
                let pinned = crate::dock::Dock::pins(self.input.settings, self.input.apps);
                let rows: Vec<_> = self
                    .launcher
                    .results
                    .iter()
                    .enumerate()
                    .skip(self.launcher.scroll)
                    .take(visible_rows)
                    .map(|(rank, index)| {
                        let app = &self.input.apps[*index];
                        Element::row(vec![
                            self.app_icon(app, component::launcher::ICON_WIDTH),
                            self.label(&app.name, font::BODY).width(Length::Fill),
                            self.icon("pin", icon::SMALL)
                                .padding(space::XS)
                                .size(
                                    component::launcher::TAB_HEIGHT,
                                    component::launcher::TAB_HEIGHT,
                                )
                                .tint(if pinned.contains(&app.id) {
                                    self.accent()
                                } else {
                                    self.ink()
                                })
                                .opacity(if pinned.contains(&app.id) {
                                    1.
                                } else {
                                    component::launcher::PIN_UNSELECTED_OPACITY
                                })
                                .radius(radius::CONTROL)
                                .on_click(Message::Request(Intent::Pin(app.id.clone())))
                                .id(format!("pin-{}", app.id.as_str())),
                        ])
                        .align(Align::Center)
                        .gap(space::LG)
                        .padding_xy(
                            component::launcher::ROW_PADDING_INLINE,
                            component::launcher::ROW_PADDING_BLOCK,
                        )
                        .size(width, component::launcher::ROW_HEIGHT)
                        .radius(radius::CARD)
                        .background(if rank == self.launcher.selected {
                            self.widget_color()
                        } else {
                            Color::TRANSPARENT
                        })
                        .selected(rank == self.launcher.selected)
                        .on_hover(Message::Select(rank))
                        .on_click(Message::Request(Intent::Launch(app.id.clone(), false)))
                        .id(format!("result-{rank}"))
                    })
                    .collect();
                let list = if rows.is_empty() {
                    self.label(
                        match self.input.application_status {
                            crate::catalog::Status::Loading if self.input.apps.is_empty() => {
                                "Loading applications…"
                            }
                            crate::catalog::Status::Unavailable => "Unable to load applications",
                            _ if self.input.apps.is_empty() => "No installed applications",
                            _ => "No matching applications",
                        },
                        font::LABEL,
                    )
                    .size(width, component::launcher::EMPTY_HEIGHT)
                    .align(Align::Center)
                } else {
                    Element::column(rows).size(width, body_h).clip()
                };
                elements.push(list.at(0., body_y));
                let search = Element::row(vec![
                    self.icon("search", icon::COMPACT),
                    Element::input(
                        &self.launcher.query,
                        "Search apps and settings",
                        Message::Query,
                    )
                    .id("launcher-search")
                    .autofocus()
                    .focus_outline(false)
                    .font(font::BODY)
                    .color(self.ink())
                    .width(Length::Fill)
                    .height(Length::Fill),
                    self.icon("close", icon::SMALL)
                        .on_click(Message::Query(String::new()))
                        .id("clear-search"),
                ])
                .align(Align::Center)
                .gap(space::SM)
                .padding_xy(
                    component::input::PADDING_INLINE,
                    component::input::PADDING_BLOCK,
                )
                .size(width, component::input::HEIGHT)
                .id("launcher-search-field")
                .focus_within()
                .radius(radius::SEARCH)
                .background(self.widget_color());
                elements.push(search.at(0., height - component::input::HEIGHT));
            }
            Mode::Wallpapers => {
                let source_height = component::wallpaper_browser::SOURCE_HEIGHT;
                elements.push(
                    self.input
                        .browser
                        .sources(width, self.theme())
                        .map(|message| Message::Request(Intent::Browser(message)))
                        .at(0., body_y),
                );
                let remaining = body_h - source_height - layout::SECTION_GAP;
                let browser = if self.input.browser.source.is_some() {
                    self.input
                        .browser
                        .view(width, remaining, self.theme())
                        .map(|message| Message::Request(Intent::Browser(message)))
                } else {
                    self.wallpaper_strip(cx, width, remaining)
                };
                elements.push(browser.at(0., body_y + source_height + layout::SECTION_GAP));
            }
            Mode::Widgets => {
                let rows = WIDGETS
                    .iter()
                    .enumerate()
                    .skip(self.launcher.scroll)
                    .take(visible_rows)
                    .map(|(index, entry)| {
                        let (id, title, description) = (entry.id, entry.title, entry.description);
                        let on = self.input.settings.visible_widgets.iter().any(|w| w == id);
                        Element::row(vec![
                            self.icon(
                                if id == "media" { "music" } else { "widgets" },
                                icon::WIDGET,
                            ),
                            Element::column(vec![
                                self.label(title, font::BODY),
                                self.label(description, font::MICRO).opacity(opacity::MUTED),
                            ])
                            .width(Length::Fill),
                            self.theme().switch_indicator(on),
                        ])
                        .align(Align::Center)
                        .gap(space::LG)
                        .padding_xy(
                            component::launcher::ROW_PADDING_INLINE,
                            component::launcher::ROW_PADDING_BLOCK,
                        )
                        .size(width, component::launcher::ROW_HEIGHT)
                        .radius(radius::CONTROL)
                        .background(self.surface_color())
                        .selected(index == self.launcher.selected)
                        .on_hover(Message::Select(index))
                        .on_click(Message::Request(Intent::ToggleWidget(id.into())))
                        .id(format!("toggle-{id}"))
                    })
                    .collect();
                elements.push(
                    Element::column(rows)
                        .size(width, body_h)
                        .clip()
                        .at(0., body_y),
                );
                elements.push(
                    self.button(
                        "Reset widget positions",
                        Message::Request(Intent::ResetLayout),
                    )
                    .selected(self.launcher.selected == WIDGETS.len())
                    .on_hover(Message::Select(WIDGETS.len()))
                    .id("reset-layout")
                    .size(width, component::launcher::RESET_HEIGHT)
                    .font(font::CONTROL)
                    .background(self.widget_color())
                    .at(0., height - component::launcher::RESET_HEIGHT),
                );
            }
            Mode::Themes => {
                let choices = self.input.palettes;
                let rows = shell_layout::palette_rows(height);
                let list_height = (body_h
                    - component::launcher::THEME_CAPTION_HEIGHT
                    - component::launcher::SIZE_BUTTON_HEIGHT
                    - layout::SECTION_GAP * 2.)
                    .max(0.);
                let row_height = component::wallpaper_browser::PALETTE_ROW.min(list_height);
                let entries = choices
                    .iter()
                    .enumerate()
                    .map(|(index, palette)| {
                        let preview = design::Theme::from_palette(palette);
                        let swatches = Element::row(
                            [
                                preview.primary,
                                preview.surface_container,
                                preview.on_surface,
                                preview.error,
                            ]
                            .into_iter()
                            .map(|color| {
                                Element::empty()
                                    .size(
                                        component::wallpaper_browser::SWATCH,
                                        component::wallpaper_browser::SWATCH,
                                    )
                                    .radius(radius::INDICATOR)
                                    .background(color)
                            })
                            .collect(),
                        )
                        .gap(space::XS);
                        Element::row(vec![
                            self.label(
                                if self.input.settings.palette.as_ref().map_or_else(
                                    || lucent_design::selected_palette(self.input.settings).id,
                                    |p| p.id.clone(),
                                ) == palette.id
                                {
                                    format!("{} · Applied", palette.name)
                                } else {
                                    palette.name.clone()
                                },
                                font::BODY,
                            )
                            .color(preview.on_surface)
                            .width(Length::Fill),
                            swatches,
                        ])
                        .gap(space::SM)
                        .align(Align::Center)
                        .padding_xy(space::LG, space::SM)
                        .size(width, row_height)
                        .radius(radius::CONTROL)
                        .background(preview.surface)
                        .selected(self.launcher.selected == index)
                        .on_hover(Message::Select(index))
                        .on_click(Message::Request(Intent::Palette(palette.clone())))
                        .id(format!("theme-{}", palette.id))
                    })
                    .chain(std::iter::once(
                        self.button(
                            "Use current wallpaper colors",
                            Message::Request(Intent::ExtractPalette),
                        )
                        .size(width, row_height)
                        .selected(self.launcher.selected == choices.len())
                        .on_hover(Message::Select(choices.len()))
                        .id("palette-from-wallpaper"),
                    ))
                    .skip(self.launcher.scroll)
                    .take(rows)
                    .collect();
                elements.push(
                    Element::column(entries)
                        .size(width, list_height)
                        .clip()
                        .at(0., body_y),
                );
                elements.push(
                    self.label("Launcher size", font::SMALL)
                        .size(width, component::launcher::THEME_CAPTION_HEIGHT)
                        .align(Align::Center)
                        .at(
                            0.,
                            height
                                - component::launcher::SIZE_BUTTON_HEIGHT
                                - layout::SECTION_GAP
                                - component::launcher::THEME_CAPTION_HEIGHT,
                        ),
                );
                let button_width = shell_layout::floor(
                    (width - space::SM * 2.) / shell_layout::LAUNCHER_PRESETS.len() as f32,
                    layout::CONTROL_STEP,
                );
                elements.push(
                    Element::row(
                        shell_layout::LAUNCHER_PRESETS
                            .iter()
                            .enumerate()
                            .map(|(index, (label, size))| {
                                self.button(*label, Message::Request(Intent::LauncherSize(*size)))
                                    .size(button_width, component::launcher::SIZE_BUTTON_HEIGHT)
                                    .padding_xy(space::SM, space::XS)
                                    .font(font::CAPTION)
                                    .background(if self.input.settings.launcher == *size {
                                        self.accent()
                                    } else {
                                        self.widget_color()
                                    })
                                    .color(if self.input.settings.launcher == *size {
                                        self.theme().on_primary
                                    } else {
                                        self.ink()
                                    })
                                    .selected(self.launcher.selected == index + choices.len() + 1)
                                    .on_hover(Message::Select(index + choices.len() + 1))
                                    .id(format!("launcher-size-{}", label.to_lowercase()))
                            })
                            .collect(),
                    )
                    .gap(space::SM)
                    .at(0., height - component::launcher::SIZE_BUTTON_HEIGHT),
                );
            }
            Mode::Commands => {
                elements.push(
                    Element::column(
                        COMMANDS
                            .into_iter()
                            .enumerate()
                            .skip(self.launcher.scroll)
                            .take(visible_rows)
                            .map(|(index, (label, action, icon))| {
                                Element::row(vec![
                                    self.icon(icon, icon::ACTION),
                                    self.label(label, font::BODY),
                                ])
                                .align(Align::Center)
                                .gap(space::LG)
                                .padding_xy(
                                    component::launcher::ROW_PADDING_INLINE,
                                    component::launcher::ROW_PADDING_BLOCK,
                                )
                                .size(width, component::launcher::ROW_HEIGHT)
                                .radius(radius::CARD)
                                .background(self.surface_color())
                                .selected(self.launcher.selected == index)
                                .on_hover(Message::Select(index))
                                .on_click(Message::Request(Intent::Action(action)))
                                .id(format!("command-{icon}-{label}"))
                            })
                            .collect(),
                    )
                    .size(width, body_h)
                    .clip()
                    .at(0., body_y),
                );
            }
        }
        let error = if self.launcher.mode == Mode::Themes && !self.theme_error.is_empty() {
            self.theme_error
        } else {
            self.error
        };
        if !error.is_empty() {
            elements.push(
                self.label(error, font::CAPTION)
                    .size(width, component::launcher::ERROR_HEIGHT)
                    .at(0., height - component::launcher::ERROR_BOTTOM)
                    .color(self.theme().error),
            );
        }
        // This launcher owns Tab to cycle modes. Keep typing focused in its
        // search input while pointer clicks select/activate other controls.
        fn retain_search_focus(e: &mut Element<Message>) {
            if e.input.is_none() {
                e.style.tab_stop = false;
            }
            for child in &mut e.children {
                retain_search_focus(child);
            }
        }
        let mut face = Element::stack(elements).size(width, height);
        retain_search_focus(&mut face);
        face
    }
    fn wallpaper_strip(&self, cx: &ViewContext, width: f32, height: f32) -> Element<Message> {
        if self.input.wallpapers.is_empty() {
            return self
                .label("Add pictures to ~/Pictures/Wallpapers", font::LABEL)
                .size(width, height)
                .align(Align::Center);
        }
        let selected = self.carousel.value(cx.now);
        let mut cards = vec![];
        // Render distant cards first so the central hero remains on top.
        let mut order: Vec<_> = visible_wallpapers(self.input.wallpapers.len(), selected).collect();
        order.sort_by(|a, b| {
            ((*b as f32 - selected).abs()).total_cmp(&(*a as f32 - selected).abs())
        });
        for i in order {
            let wall = &self.input.wallpapers[i];
            let distance = i as f32 - selected;
            let abs = distance.abs();
            let size = component::wallpaper::HERO_WIDTH
                - (abs.min(1.) * component::wallpaper::NEAR_SHRINK)
                - ((abs - 1.).clamp(0., 1.) * component::wallpaper::FAR_SHRINK);
            let h = size * component::wallpaper::ASPECT_RATIO;
            let center = width / 2.
                + distance.signum()
                    * (abs.min(1.) * component::wallpaper::NEAR_STEP
                        + (abs - 1.).max(0.) * component::wallpaper::FAR_STEP);
            let image = self
                .previews
                .get(&wall.path)
                .cloned()
                .map(|data| Element::image(data).contain())
                .unwrap_or_else(|| {
                    if self.previews.error(&wall.path).is_some() {
                        self.label("Preview unavailable", font::SMALL)
                            .align(Align::Center)
                            .background(self.widget_color())
                    } else {
                        Element::empty().background(self.widget_color())
                    }
                });
            let active = i == self.input.wallpaper_index;
            cards.push(
                Element::stack(vec![
                    image.size(size, h).radius(radius::CARD),
                    self.label(&wall.name, font::SMALL)
                        .size(
                            size - component::wallpaper::CAPTION_INSETS,
                            component::wallpaper::CAPTION_HEIGHT,
                        )
                        .at(
                            component::wallpaper::CAPTION_INSET,
                            h + component::wallpaper::CAPTION_GAP,
                        )
                        .align(Align::Center),
                ])
                .size(size, h + component::wallpaper::FOOTER_HEIGHT)
                .at(
                    center - size / 2.,
                    (component::wallpaper::STAGE_HEIGHT - h) / 2.,
                )
                .on_click(Message::Request(Intent::Wallpaper(i, active)))
                .id(format!("wallpaper-{i}")),
            );
        }
        cards.push(
            self.icon_button("wallpaper-previous", "left", Message::Navigate(-1))
                .size(
                    component::wallpaper::CONTROL_SIZE,
                    component::wallpaper::CONTROL_SIZE,
                )
                .padding(space::MD)
                .at(0., height - component::wallpaper::FOOTER_HEIGHT),
        );
        let apply_width = component::wallpaper::BUTTON_WIDTH.min(
            (width - component::wallpaper::CONTROL_SIZE * 2. - layout::SECTION_GAP * 2.).max(0.),
        );
        cards.push(
            self.button(
                if self.current_wallpaper == self.input.wallpapers[self.input.wallpaper_index].path
                {
                    "Applied"
                } else {
                    "Apply wallpaper"
                },
                Message::Request(Intent::Wallpaper(self.input.wallpaper_index, true)),
            )
            .size(apply_width, component::wallpaper::BUTTON_HEIGHT)
            .background(self.accent())
            .color(self.theme().on_primary)
            .id("apply-wallpaper")
            .at(
                (width - apply_width) / 2.,
                height - component::wallpaper::FOOTER_HEIGHT,
            ),
        );
        cards.push(
            self.icon_button("wallpaper-next", "right", Message::Navigate(1))
                .size(
                    component::wallpaper::CONTROL_SIZE,
                    component::wallpaper::CONTROL_SIZE,
                )
                .padding(space::MD)
                .at(
                    width - component::wallpaper::CONTROL_SIZE,
                    height - component::wallpaper::FOOTER_HEIGHT,
                ),
        );
        Element::stack(cards).size(width, height).clip()
    }
}
