use crate::{desktop::*, shell_layout};
use lucent_api::*;
use lucent_design::{self as design, *};
use lucent_domain::Application as App;

impl Desktop {
    pub fn theme(&self) -> design::Theme {
        design::Theme::from_palette(&design::selected_palette(&self.settings))
    }
    pub fn surface_color(&self) -> Color {
        self.theme().surface
    }
    pub fn widget_color(&self) -> Color {
        self.theme().surface_container
    }
    pub fn ink(&self) -> Color {
        self.theme().on_surface
    }
    pub fn accent(&self) -> Color {
        self.theme().primary
    }
    pub fn button(&self, label: impl Into<String>, message: Message) -> Element<Message> {
        self.theme().button(label, message)
    }
    pub fn label(&self, text: impl Into<String>, size: f32) -> Element<Message> {
        Element::text(text).font(size).color(self.ink())
    }
    pub fn icon(&self, name: &str, size: f32) -> Element<Message> {
        self.images
            .get(&format!("symbol:{name}"))
            .cloned()
            .map(|data| Element::image(data).contain())
            .unwrap_or_else(Element::empty)
            .size(size, size)
            .tint(self.ink())
    }
    pub fn icon_button(&self, id: &str, name: &str, message: Message) -> Element<Message> {
        self.icon(name, icon::CONTROL)
            .padding(component::icon_button::PADDING)
            .size(
                component::icon_button::EXTENT,
                component::icon_button::EXTENT,
            )
            .radius(radius::CONTROL)
            .background(self.surface_color())
            .on_click(message)
            .id(id)
    }
    pub fn app_icon(&self, app: &App, size: f32) -> Element<Message> {
        self.images
            .get(&format!("app:{}", app.id.0))
            .or_else(|| self.images.get(&app.icon))
            .or_else(|| self.images.get("app:fallback"))
            .cloned()
            .map(|data| {
                let monochrome = data.key.starts_with("lucent-app:");
                let image = Element::image(data).contain();
                if monochrome {
                    image.tint(self.ink())
                } else {
                    image
                }
            })
            .unwrap_or_else(|| {
                self.icon("apps", size)
                    .padding(space::XS)
                    .background(self.widget_color())
                    .radius(radius::CARD)
            })
            .size(size, size)
    }
    fn pill(&self, children: Vec<Element<Message>>) -> Element<Message> {
        Element::row(children)
            .gap(space::SM)
            .padding_xy(
                component::pill::PADDING_INLINE,
                component::pill::PADDING_BLOCK,
            )
            .height(Length::Fixed(component::pill::HEIGHT))
            .align(Align::Center)
            .radius(radius::CONTROL)
            .background(self.surface_color())
    }
    pub fn bar(&self, cx: &ViewContext) -> Element<Message> {
        let geometry = shell_layout::BarLayout::new(cx.width, self.compositor.workspaces.len());
        let active = self
            .compositor
            .workspaces
            .iter()
            .position(|w| w.active)
            .unwrap_or(0);
        let start = (active + 1).saturating_sub(geometry.workspace_count).min(
            self.compositor
                .workspaces
                .len()
                .saturating_sub(geometry.workspace_count),
        );
        let bar_icon = |name: &str| {
            self.icon(name, icon::TINY)
                .size(component::bar::CONTROL_SIZE, component::bar::CONTROL_SIZE)
                .padding(space::XS)
        };
        let workspaces = self
            .compositor
            .workspaces
            .iter()
            .skip(start)
            .take(geometry.workspace_count)
            .map(|w| {
                let ring = if w.active {
                    Element::stack(vec![
                        Element::empty()
                            .size(component::bar::RING_SIZE, component::bar::RING_SIZE)
                            .at(
                                (component::bar::WORKSPACE_SIZE - component::bar::RING_SIZE) / 2.,
                                (component::bar::WORKSPACE_SIZE - component::bar::RING_SIZE) / 2.,
                            )
                            .radius(radius::SMALL)
                            .background(self.surface_color()),
                    ])
                    .size(
                        component::bar::WORKSPACE_SIZE,
                        component::bar::WORKSPACE_SIZE,
                    )
                    .radius(radius::CONTROL)
                    .background(self.accent())
                } else {
                    Element::stack(vec![
                        Element::empty()
                            .size(component::bar::DOT_SIZE, component::bar::DOT_SIZE)
                            .at(
                                (component::bar::WORKSPACE_SIZE - component::bar::DOT_SIZE) / 2.,
                                (component::bar::WORKSPACE_SIZE - component::bar::DOT_SIZE) / 2.,
                            )
                            .radius(radius::INDICATOR)
                            .background(self.ink().alpha(if w.windows > 0 {
                                opacity::STRONG
                            } else {
                                opacity::QUIET
                            })),
                    ])
                    .size(
                        component::bar::WORKSPACE_SIZE,
                        component::bar::WORKSPACE_SIZE,
                    )
                };
                ring.on_click(Message::Workspace(w.id))
                    .id(format!("workspace-{}", w.id))
            })
            .collect();
        let spaces = self
            .pill(workspaces)
            .gap(space::SM)
            .size(geometry.workspaces.w, geometry.workspaces.h)
            .at(geometry.workspaces.x, geometry.workspaces.y);
        let title = if self.media.title.is_empty() {
            "Nothing playing".into()
        } else {
            format!("{} · {}", self.media.artist, self.media.title)
        };
        let media = self.pill(vec![
            bar_icon("music"),
            self.label(title, font::SMALL)
                .width(Length::Fixed(component::bar::MEDIA_TEXT_WIDTH))
                .clip(),
            bar_icon(if self.media.playing { "pause" } else { "play" })
                .on_click(Message::Action(Action::PlayPause))
                .id("bar-play"),
        ]);
        let weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        let date = &self.clock.date;
        let time = self
            .label(
                format!("{:02}:{:02}", self.clock.hour, self.clock.minute),
                font::CONTROL,
            )
            .width(Length::Fixed(component::bar::TIME_WIDTH))
            .height(Length::Fill)
            .align(Align::Center)
            .padding_xy(space::SM, space::XXS)
            .radius(radius::CONTROL)
            .background(self.accent())
            .color(self.theme().on_primary);
        let center = self
            .pill(vec![
                self.label(
                    format!(
                        "{}, {:02}/{:02}",
                        weekdays[date.weekday as usize], date.day, date.month
                    ),
                    font::CAPTION,
                )
                .width(Length::Fixed(component::bar::DATE_WIDTH))
                .align(Align::Center),
                time,
                bar_icon("cloud"),
                self.label(
                    self.weather
                        .as_ref()
                        .map(|w| format!("{:.0}°", w.temperature))
                        .unwrap_or_else(|| "—".into()),
                    font::CAPTION,
                )
                .width(Length::Fixed(component::bar::CONTROL_SIZE))
                .align(Align::Center),
            ])
            .on_click(Message::Mode(Mode::Widgets))
            .id("bar-clock");
        let volume = self
            .system
            .volume
            .map(|v| v.to_string())
            .unwrap_or_else(|| "—".into());
        let right = self
            .pill(vec![
                bar_icon("notifications")
                    .on_click(Message::Notifications(
                        crate::notifications::Message::Toggle,
                    ))
                    .id("bar-notifications"),
                bar_icon("network"),
                bar_icon("volume")
                    .on_click(Message::Action(Action::Mute))
                    .id("bar-mute"),
                self.label(
                    if self.system.muted {
                        "×".into()
                    } else {
                        volume
                    },
                    font::CAPTION,
                )
                .width(Length::Fixed(component::bar::CONTROL_SIZE))
                .align(Align::Center),
                bar_icon("lock")
                    .on_click(Message::Action(Action::Lock))
                    .id("bar-lock"),
            ])
            .size(geometry.system.w, geometry.system.h)
            .at(geometry.system.x, geometry.system.y);
        let mut groups = vec![right];
        if geometry.workspace_count > 0 {
            groups.push(spaces);
        }
        if let Some(rect) = geometry.media {
            groups.push(media.size(rect.w, rect.h).at(rect.x, rect.y));
        }
        if let Some(rect) = geometry.clock {
            groups.push(center.size(rect.w, rect.h).at(rect.x, rect.y));
        }
        Element::stack(groups).fill()
    }
    pub fn dock(&self, cx: &ViewContext) -> Element<Message> {
        let width = self
            .panel_width
            .value(cx.now)
            .max(component::dock::MINIMUM_WIDTH);
        let height = self
            .panel_height
            .value(cx.now)
            .max(component::dock::MINIMUM_HEIGHT);
        let reveal = self.reveal.value(cx.now).clamp(0., 1.);
        let (x, y) = shell_layout::panel_origin((cx.width, cx.height), (width, height));
        let mut children = vec![];
        if reveal < 0.999 {
            children.push(self.dock_row().opacity((1. - reveal * 2.).max(0.)).at(
                (self.dock_width() - shell_layout::dock_content_width(self.dock_apps().len() + 1))
                    / 2.,
                (component::panel::DOCK_HEIGHT - component::dock_row::HEIGHT) / 2.,
            ));
        }
        if reveal > 0.001 {
            let target_width = self.panel_width.target_value();
            let target_height = self.panel_height.target_value();
            children.push(
                self.launcher_face(
                    cx,
                    target_width - (2. * component::dock::CONTENT_INSET),
                    target_height - (2. * component::dock::CONTENT_INSET),
                )
                .at(
                    component::dock::CONTENT_INSET,
                    component::dock::CONTENT_INSET,
                )
                .opacity(((reveal - 0.2) / 0.8).clamp(0., 1.)),
            );
        }
        // One persistent rounded surface morphs between the dock and launcher.
        let panel = Element::stack(children)
            .size(width, height)
            .at(x, y)
            .radius(radius::PANEL)
            .background(self.surface_color())
            .shadow()
            .clip()
            .tab_stop(false)
            .on_click(Message::Select(self.selected))
            .id("dock-panel");
        Element::stack(vec![panel]).fill()
    }
    fn dock_row(&self) -> Element<Message> {
        let mut items = vec![
            self.icon("apps", icon::LAUNCHER)
                .padding(space::SM)
                .size(
                    component::dock_row::ITEM_SIZE,
                    component::dock_row::ITEM_SIZE,
                )
                .radius(radius::CARD)
                .background(self.surface_color())
                .on_click(Message::ToggleLauncher)
                .id("dock-launcher"),
        ];
        for app in self.dock_apps() {
            let running = self.compositor.windows.iter().any(|w| {
                w.app_class.eq_ignore_ascii_case(&app.startup_class)
                    || w.app_class
                        .eq_ignore_ascii_case(app.id.0.trim_end_matches(".desktop"))
            });
            let mut content = vec![self.app_icon(app, icon::APP).at(
                (component::dock_row::ITEM_SIZE - icon::APP) / 2.,
                (component::dock_row::ITEM_SIZE - icon::APP) / 2.,
            )];
            if running {
                content.push(
                    Element::empty()
                        .size(
                            component::dock_row::INDICATOR_WIDTH,
                            component::dock_row::INDICATOR_HEIGHT,
                        )
                        .at(
                            component::dock_row::INDICATOR_LEFT,
                            component::dock_row::INDICATOR_TOP,
                        )
                        .radius(radius::INDICATOR)
                        .background(self.accent()),
                );
            }
            items.push(
                Element::stack(content)
                    .size(
                        component::dock_row::ITEM_SIZE,
                        component::dock_row::ITEM_SIZE,
                    )
                    .radius(radius::CONTROL)
                    .background(self.surface_color())
                    .on_click(Message::Launch(app.id.clone(), true))
                    .id(format!("dock-{}", app.id.0)),
            );
        }
        Element::row(items)
            .gap(space::SM)
            .height(Length::Fixed(component::dock_row::HEIGHT))
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
            let active = mode == self.mode;
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
        let body_h = shell_layout::body_height(self.mode, height);
        let visible_rows = (body_h / component::launcher::ROW_HEIGHT).floor().max(1.) as usize;
        match self.mode {
            Mode::Apps => {
                let rows: Vec<_> = self
                    .results
                    .iter()
                    .enumerate()
                    .skip(self.scroll)
                    .take(visible_rows)
                    .map(|(rank, index)| {
                        let app = &self.apps[*index];
                        Element::row(vec![
                            self.app_icon(app, component::launcher::ICON_WIDTH),
                            self.label(&app.name, font::BODY).width(Length::Fill),
                            self.label(
                                if rank == self.selected { "Open" } else { "" },
                                font::MICRO,
                            )
                            .width(Length::Fixed(component::launcher::TAB_HEIGHT)),
                        ])
                        .align(Align::Center)
                        .gap(space::LG)
                        .padding_xy(
                            component::launcher::ROW_PADDING_INLINE,
                            component::launcher::ROW_PADDING_BLOCK,
                        )
                        .size(width, component::launcher::ROW_HEIGHT)
                        .radius(radius::CARD)
                        .background(if rank == self.selected {
                            self.widget_color()
                        } else {
                            Color::TRANSPARENT
                        })
                        .selected(rank == self.selected)
                        .on_hover(Message::Select(rank))
                        .on_click(Message::Launch(app.id.clone(), false))
                        .id(format!("result-{rank}"))
                    })
                    .collect();
                let list = if rows.is_empty() {
                    self.label(
                        if self.apps.is_empty() {
                            "Loading applications…"
                        } else {
                            "No matching applications"
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
                    Element::input(&self.query, "Search apps and settings", Message::Query)
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
                    self.browser
                        .sources(width, self.theme())
                        .map(Message::Browser)
                        .at(0., body_y),
                );
                let remaining = body_h - source_height - layout::SECTION_GAP;
                let browser = if self.browser.source.is_some() {
                    self.browser
                        .view(width, remaining, self.theme())
                        .map(Message::Browser)
                } else {
                    self.wallpaper_strip(cx, width, remaining)
                };
                elements.push(browser.at(0., body_y + source_height + layout::SECTION_GAP));
            }
            Mode::Widgets => {
                let rows = WIDGETS
                    .iter()
                    .enumerate()
                    .skip(self.scroll)
                    .take(visible_rows)
                    .map(|(index, (id, title, description))| {
                        let on = self.settings.visible_widgets.iter().any(|w| w == id);
                        Element::row(vec![
                            self.icon(
                                if *id == "media" { "music" } else { "widgets" },
                                icon::WIDGET,
                            ),
                            Element::column(vec![
                                self.label(*title, font::BODY),
                                self.label(*description, font::MICRO)
                                    .opacity(opacity::MUTED),
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
                        .selected(index == self.selected)
                        .on_hover(Message::Select(index))
                        .on_click(Message::ToggleWidget((*id).into()))
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
                    self.button("Reset widget positions", Message::ResetLayout)
                        .selected(self.selected == WIDGETS.len())
                        .on_hover(Message::Select(WIDGETS.len()))
                        .id("reset-layout")
                        .size(width, component::launcher::RESET_HEIGHT)
                        .font(font::CONTROL)
                        .background(self.widget_color())
                        .at(0., height - component::launcher::RESET_HEIGHT),
                );
            }
            Mode::Themes => {
                let choices = self.palette_choices();
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
                                if self.settings.palette.as_ref().map_or_else(
                                    || lucent_design::selected_palette(&self.settings).id,
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
                        .selected(self.selected == index)
                        .on_hover(Message::Select(index))
                        .on_click(Message::Palette(palette.clone()))
                        .id(format!("theme-{}", palette.id))
                    })
                    .chain(std::iter::once(
                        self.button("Use current wallpaper colors", Message::ExtractPalette)
                            .size(width, row_height)
                            .selected(self.selected == choices.len())
                            .on_hover(Message::Select(choices.len()))
                            .id("palette-from-wallpaper"),
                    ))
                    .skip(self.scroll)
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
                                self.button(*label, Message::LauncherSize(*size))
                                    .size(button_width, component::launcher::SIZE_BUTTON_HEIGHT)
                                    .padding_xy(space::SM, space::XS)
                                    .font(font::CAPTION)
                                    .background(if self.settings.launcher == *size {
                                        self.accent()
                                    } else {
                                        self.widget_color()
                                    })
                                    .color(if self.settings.launcher == *size {
                                        self.theme().on_primary
                                    } else {
                                        self.ink()
                                    })
                                    .selected(self.selected == index + choices.len() + 1)
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
                            .skip(self.scroll)
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
                                .selected(self.selected == index)
                                .on_hover(Message::Select(index))
                                .on_click(Message::Action(action))
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
        let error = if self.mode == Mode::Themes && !self.theme_error.is_empty() {
            &self.theme_error
        } else {
            &self.error
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
        if self.wallpapers.is_empty() {
            return self
                .label("Add pictures to ~/Pictures/Wallpapers", font::LABEL)
                .size(width, height)
                .align(Align::Center);
        }
        let selected = self.carousel.value(cx.now);
        let mut cards = vec![];
        // Render distant cards first so the central hero remains on top.
        let mut order: Vec<_> = (0..self.wallpapers.len())
            .filter(|i| (*i as f32 - selected).abs() < component::wallpaper::VISIBLE_DISTANCE)
            .collect();
        order.sort_by(|a, b| {
            ((*b as f32 - selected).abs()).total_cmp(&(*a as f32 - selected).abs())
        });
        for i in order {
            let wall = &self.wallpapers[i];
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
                .images
                .get(&wall.path)
                .cloned()
                .map(|data| Element::image(data).contain())
                .unwrap_or_else(|| Element::empty().background(self.widget_color()));
            let active = i == self.wallpaper_index;
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
                .on_click(Message::Wallpaper(i, active))
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
                if self.applied_wallpaper == self.wallpapers[self.wallpaper_index].path {
                    "Applied"
                } else {
                    "Apply wallpaper"
                },
                Message::Wallpaper(self.wallpaper_index, true),
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
