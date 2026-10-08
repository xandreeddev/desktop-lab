use crate::desktop::*;
use lucent_api::*;
use lucent_domain::Application as App;

impl Desktop {
    pub fn surface_color(&self) -> Color {
        if self.settings.light {
            Color::hex(0xbadbea)
        } else {
            Color::hex(0x2c5184)
        }
    }
    pub fn widget_color(&self) -> Color {
        if self.settings.light {
            Color::hex(0xb1e9eb)
        } else {
            Color::hex(0x1b6a7d)
        }
    }
    pub fn ink(&self) -> Color {
        if self.settings.light {
            Color::hex(0x17334b)
        } else {
            Color::hex(0xbce8f4)
        }
    }
    pub fn accent(&self) -> Color {
        Color::hex(0x73dcee)
    }
    pub fn label(&self, text: impl Into<String>, size: f32) -> Element<Message> {
        Element::text(text).font(size).color(self.ink())
    }
    pub fn icon(&self, name: &str, size: f32) -> Element<Message> {
        self.images
            .get(&format!("symbol:{name}"))
            .cloned()
            .map(Element::image)
            .unwrap_or_else(Element::empty)
            .size(size, size)
    }
    pub fn icon_button(&self, id: &str, name: &str, message: Message) -> Element<Message> {
        self.icon(name, 19.)
            .padding(7.)
            .size(33., 33.)
            .radius(18.)
            .background(self.surface_color())
            .on_click(message)
            .id(id)
    }
    pub fn app_icon(&self, app: &App, size: f32) -> Element<Message> {
        self.images
            .get(&app.icon)
            .cloned()
            .map(Element::image)
            .unwrap_or_else(|| {
                self.label(
                    app.name.chars().next().unwrap_or('A').to_string(),
                    size * 0.5,
                )
                .align(Align::Center)
                .background(self.widget_color())
                .radius(size / 2.)
            })
            .size(size, size)
    }
    fn pill(&self, children: Vec<Element<Message>>) -> Element<Message> {
        Element::row(children)
            .gap(6.)
            .padding(5.)
            .height(Length::Fixed(32.))
            .align(Align::Center)
            .radius(18.)
            .background(self.surface_color())
    }
    pub fn bar(&self, cx: &ViewContext) -> Element<Message> {
        let workspaces = self
            .compositor
            .workspaces
            .iter()
            .take(8)
            .map(|w| {
                let ring = if w.active {
                    Element::stack(vec![
                        Element::empty()
                            .size(17., 17.)
                            .at(3.5, 3.5)
                            .radius(9.)
                            .background(self.surface_color()),
                    ])
                    .size(24., 24.)
                    .radius(13.)
                    .background(self.accent())
                } else {
                    Element::stack(vec![
                        Element::empty()
                            .size(5., 5.)
                            .at(9.5, 9.5)
                            .radius(3.)
                            .background(self.ink().alpha(if w.windows > 0 { 0.9 } else { 0.4 })),
                    ])
                    .size(24., 24.)
                };
                ring.on_click(Message::Workspace(w.id))
                    .id(format!("workspace-{}", w.id))
            })
            .collect();
        let spaces = self.pill(workspaces).gap(2.).at(16., 12.);
        let title = if self.media.title.is_empty() {
            "Nothing playing".into()
        } else {
            format!("{} · {}", self.media.artist, self.media.title)
        };
        let media = self
            .pill(vec![
                self.icon("music", 19.),
                self.label(title, 12.).width(Length::Fixed(135.)),
                self.icon(if self.media.playing { "pause" } else { "play" }, 19.)
                    .on_click(Message::Action(Action::PlayPause))
                    .id("bar-play"),
            ])
            .at(184., 12.);
        let weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        let date = &self.clock.date;
        let time = self
            .label(
                format!("{:02}:{:02}", self.clock.hour, self.clock.minute),
                13.,
            )
            .width(Length::Fixed(50.))
            .align(Align::Center)
            .padding(3.)
            .radius(12.)
            .background(self.accent())
            .color(Color::hex(0x153f5b));
        let center = self
            .pill(vec![
                self.label(
                    format!(
                        "{}, {:02}/{:02}",
                        weekdays[date.weekday as usize], date.day, date.month
                    ),
                    11.,
                )
                .width(Length::Fixed(78.))
                .align(Align::Center),
                time,
                self.icon("cloud", 18.),
                self.label(
                    self.weather
                        .as_ref()
                        .map(|w| format!("{:.0}°", w.temperature))
                        .unwrap_or_else(|| "—".into()),
                    11.,
                )
                .width(Length::Fixed(24.))
                .align(Align::Center),
            ])
            .on_click(Message::Mode(Mode::Widgets))
            .id("bar-clock")
            .at((cx.width - 198.) / 2., 12.);
        let volume = self
            .system
            .volume
            .map(|v| v.to_string())
            .unwrap_or_else(|| "—".into());
        let right = self
            .pill(vec![
                self.label("US", 11.),
                self.icon("network", 17.),
                self.icon("volume", 17.)
                    .on_click(Message::Action(Action::Mute))
                    .id("bar-mute"),
                self.label(
                    if self.system.muted {
                        "×".into()
                    } else {
                        volume
                    },
                    11.,
                ),
                self.icon("lock", 17.)
                    .on_click(Message::Action(Action::Lock))
                    .id("bar-lock"),
            ])
            .at(cx.width - 132., 12.);
        Element::stack(vec![spaces, media, center, right]).fill()
    }
    pub fn dock(&self, cx: &ViewContext) -> Element<Message> {
        let width = self.panel_width.value(cx.now).max(60.);
        let height = self.panel_height.value(cx.now).max(40.);
        let reveal = self.reveal.value(cx.now).clamp(0., 1.);
        let x = (cx.width - width) / 2.;
        let y = cx.height - height - 12.;
        let mut children = vec![];
        if reveal < 0.999 {
            children.push(
                self.dock_row()
                    .opacity((1. - reveal * 2.).max(0.))
                    .at(10., 6.),
            );
        }
        if reveal > 0.001 {
            let target_width = self.panel_width.target_value();
            let target_height = self.panel_height.target_value();
            children.push(
                self.launcher_face(cx, target_width - 36., target_height - 36.)
                    .at(18., 18.)
                    .opacity(((reveal - 0.2) / 0.8).clamp(0., 1.)),
            );
        }
        // One persistent rounded surface morphs between the dock and launcher.
        let panel = Element::stack(children)
            .size(width, height)
            .at(x, y)
            .radius(28.)
            .background(self.surface_color())
            .shadow()
            .clip()
            .on_click(Message::Select(self.selected))
            .id("dock-panel");
        Element::stack(vec![panel]).fill()
    }
    fn dock_row(&self) -> Element<Message> {
        let mut items = vec![
            self.icon("apps", 23.)
                .padding(9.)
                .size(41., 41.)
                .radius(22.)
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
            let mut content = vec![self.app_icon(app, 28.).at(6.5, 3.)];
            if running {
                content.push(
                    Element::empty()
                        .size(11., 2.)
                        .at(15., 36.)
                        .radius(1.)
                        .background(self.accent()),
                );
            }
            items.push(
                Element::stack(content)
                    .size(41., 41.)
                    .radius(18.)
                    .background(self.surface_color())
                    .on_click(Message::Launch(app.id.clone(), true))
                    .id(format!("dock-{}", app.id.0)),
            );
        }
        Element::row(items).gap(8.).height(Length::Fixed(42.))
    }
    fn launcher_face(&self, cx: &ViewContext, width: f32, height: f32) -> Element<Message> {
        let mut modes = vec![
            self.icon("apps", 20.)
                .size(28., 30.)
                .on_click(Message::Mode(Mode::Apps)),
        ];
        for (mode, icon) in [
            (Mode::Apps, "apps"),
            (Mode::Commands, "command"),
            (Mode::Wallpapers, "wallpaper"),
            (Mode::Themes, "palette"),
            (Mode::Widgets, "widgets"),
        ] {
            let active = mode == self.mode;
            let mut contents = vec![self.icon(icon, 16.)];
            if active {
                contents.push(self.label(mode.name(), 11.).color(Color::hex(0x173e59)));
            }
            modes.push(
                Element::row(contents)
                    .align(Align::Center)
                    .gap(5.)
                    .padding(6.)
                    .height(Length::Fixed(30.))
                    .radius(18.)
                    .background(if active {
                        self.accent()
                    } else {
                        self.surface_color()
                    })
                    .on_click(Message::Mode(mode))
                    .id(format!("mode-{}", mode.name().to_lowercase())),
            );
        }
        modes.push(
            self.icon("power", 17.)
                .size(28., 30.)
                .on_click(Message::Mode(Mode::Commands))
                .id("mode-power"),
        );
        let header = Element::row(modes).gap(4.).height(Length::Fixed(31.));
        let mut elements = vec![header];
        let body_y = 43.;
        let body_h = (height - 99.).max(50.);
        match self.mode {
            Mode::Apps => {
                let rows: Vec<_> = self
                    .results
                    .iter()
                    .enumerate()
                    .skip(self.scroll)
                    .take(7)
                    .map(|(rank, index)| {
                        let app = &self.apps[*index];
                        Element::row(vec![
                            self.app_icon(app, 28.),
                            self.label(&app.name, 14.).width(Length::Fill),
                            self.label(if rank == self.selected { "Open" } else { "" }, 10.)
                                .width(Length::Fixed(30.)),
                        ])
                        .align(Align::Center)
                        .gap(12.)
                        .padding(7.)
                        .size(width, 45.)
                        .on_hover(Message::Select(rank))
                        .on_click(Message::Launch(app.id.clone(), false))
                        .id(format!("result-{rank}"))
                    })
                    .collect();
                let selection = Element::empty()
                    .size(width, 42.)
                    .at(0., self.selection.value(cx.now))
                    .radius(23.)
                    .background(self.widget_color());
                let list = if rows.is_empty() {
                    self.label(
                        if self.apps.is_empty() {
                            "Loading applications…"
                        } else {
                            "No matching applications"
                        },
                        15.,
                    )
                    .size(width, 70.)
                    .align(Align::Center)
                } else {
                    Element::stack(vec![selection, Element::column(rows)])
                        .size(width, body_h)
                        .clip()
                };
                elements.push(list.at(0., body_y));
                let search = Element::row(vec![
                    self.icon("search", 18.),
                    Element::input(&self.query, "Search apps and settings", Message::Query)
                        .id("launcher-search")
                        .autofocus()
                        .font(14.)
                        .color(self.ink())
                        .width(Length::Fill)
                        .height(Length::Fixed(28.)),
                    self.icon("close", 17.)
                        .on_click(Message::Query(String::new()))
                        .id("clear-search"),
                ])
                .gap(10.)
                .padding(10.)
                .size(width, 45.)
                .radius(25.)
                .background(self.widget_color());
                elements.push(search.at(0., height - 45.));
            }
            Mode::Wallpapers => {
                elements.push(self.wallpaper_strip(cx, width, body_h + 45.).at(0., body_y))
            }
            Mode::Widgets => {
                let rows = WIDGETS
                    .iter()
                    .map(|(id, title, description)| {
                        let on = self.settings.visible_widgets.iter().any(|w| w == id);
                        Element::row(vec![
                            self.icon(if *id == "media" { "music" } else { "widgets" }, 24.),
                            Element::column(vec![
                                self.label(*title, 14.),
                                self.label(*description, 10.).opacity(0.75),
                            ])
                            .width(Length::Fill),
                            self.label(if on { "●" } else { "○" }, 23.)
                                .color(self.accent()),
                        ])
                        .align(Align::Center)
                        .gap(10.)
                        .padding(6.)
                        .size(width, 49.)
                        .radius(18.)
                        .background(self.surface_color())
                        .on_click(Message::ToggleWidget((*id).into()))
                        .id(format!("toggle-{id}"))
                    })
                    .collect();
                elements.push(Element::column(rows).gap(2.).at(0., body_y));
                elements.push(
                    Element::button("Reset widget positions", Message::ResetLayout)
                        .id("reset-layout")
                        .size(width, 36.)
                        .font(13.)
                        .background(self.widget_color())
                        .at(0., height - 38.),
                );
            }
            Mode::Themes => {
                elements.push(
                    Element::row(vec![
                        Element::button("Dark", Message::Theme(false))
                            .size(width / 2. - 5., 84.)
                            .background(Color::hex(0x2c5184))
                            .color(Color::hex(0xbce8f4))
                            .id("theme-dark"),
                        Element::button("Light", Message::Theme(true))
                            .size(width / 2. - 5., 84.)
                            .background(Color::hex(0xbadbea))
                            .color(Color::hex(0x17334b))
                            .id("theme-light"),
                    ])
                    .gap(10.)
                    .at(0., body_y),
                );
                elements.push(
                    self.label("Colors shared across every component", 12.)
                        .size(width, 30.)
                        .align(Align::Center)
                        .at(0., body_y + 100.),
                );
            }
            Mode::Commands => {
                let actions = [
                    ("New terminal", Action::Terminal, "command"),
                    ("Lock screen", Action::Lock, "lock"),
                    ("Volume up", Action::VolumeUp, "volume"),
                    ("Volume down", Action::VolumeDown, "volume"),
                    ("Mute / unmute", Action::Mute, "volume"),
                    ("Close Lucent", Action::Quit, "power"),
                ];
                elements.push(
                    Element::column(
                        actions
                            .into_iter()
                            .map(|(label, action, icon)| {
                                Element::row(vec![self.icon(icon, 22.), self.label(label, 14.)])
                                    .gap(12.)
                                    .padding(9.)
                                    .size(width, 46.)
                                    .radius(22.)
                                    .background(self.surface_color())
                                    .on_click(Message::Action(action))
                                    .id(format!("command-{icon}-{label}"))
                            })
                            .collect(),
                    )
                    .gap(2.)
                    .at(0., body_y),
                );
            }
        }
        if !self.error.is_empty() {
            elements.push(
                self.label(&self.error, 11.)
                    .size(width, 24.)
                    .at(0., height - 71.)
                    .color(Color::hex(0xffcfbc)),
            );
        }
        Element::stack(elements).size(width, height)
    }
    fn wallpaper_strip(&self, cx: &ViewContext, width: f32, height: f32) -> Element<Message> {
        if self.wallpapers.is_empty() {
            return self
                .label("Add pictures to ~/Pictures/Wallpapers", 15.)
                .size(width, height)
                .align(Align::Center);
        }
        let selected = self.carousel.value(cx.now);
        let mut cards = vec![];
        // Render distant cards first so the central hero remains on top.
        let mut order: Vec<_> = (0..self.wallpapers.len())
            .filter(|i| (*i as f32 - selected).abs() < 3.2)
            .collect();
        order.sort_by(|a, b| {
            ((*b as f32 - selected).abs()).total_cmp(&(*a as f32 - selected).abs())
        });
        for i in order {
            let wall = &self.wallpapers[i];
            let distance = i as f32 - selected;
            let abs = distance.abs();
            let size = 340. - (abs.min(1.) * 102.) - ((abs - 1.).clamp(0., 1.) * 88.);
            let h = size * 0.62;
            let center =
                width / 2. + distance.signum() * (abs.min(1.) * 300. + (abs - 1.).max(0.) * 205.);
            let image = self
                .images
                .get(&wall.path)
                .cloned()
                .map(Element::image)
                .unwrap_or_else(|| Element::empty().background(self.widget_color()));
            let active = i == self.wallpaper_index;
            cards.push(
                Element::stack(vec![
                    image.size(size, h).radius(22.),
                    self.label(&wall.name, 12.)
                        .size(size - 20., 30.)
                        .at(10., h + 6.)
                        .align(Align::Center),
                ])
                .size(size, h + 40.)
                .at(center - size / 2., (230. - h) / 2.)
                .on_click(Message::Wallpaper(i, active))
                .id(format!("wallpaper-{i}")),
            );
        }
        cards.push(
            self.icon_button("wallpaper-previous", "left", Message::Navigate(-1))
                .at(5., height - 40.),
        );
        cards.push(
            Element::button(
                if self.applied_wallpaper == self.wallpapers[self.wallpaper_index].path {
                    "Applied"
                } else {
                    "Apply wallpaper"
                },
                Message::Wallpaper(self.wallpaper_index, true),
            )
            .size(180., 36.)
            .background(self.accent())
            .color(Color::hex(0x153f5b))
            .id("apply-wallpaper")
            .at((width - 180.) / 2., height - 40.),
        );
        cards.push(
            self.icon_button("wallpaper-next", "right", Message::Navigate(1))
                .at(width - 38., height - 40.),
        );
        Element::stack(cards).size(width, height).clip()
    }
}
