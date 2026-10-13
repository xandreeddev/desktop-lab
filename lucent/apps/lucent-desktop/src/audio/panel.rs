//! Native audio presentation. Domain snapshots are borrowed; views perform no I/O.
use super::{Audio, Message, Observation, commands};
use lucent_api::*;
use lucent_design::{
    Theme,
    component::{audio as token, scroll},
    font, motion, radius, space,
};
use lucent_domain::*;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Outputs,
    Inputs,
    Applications,
}
impl Tab {
    pub fn name(self) -> &'static str {
        match self {
            Self::Outputs => "Outputs",
            Self::Inputs => "Inputs",
            Self::Applications => "Apps",
        }
    }
}
pub struct Panel {
    pub open: bool,
    pub mounted: bool,
    pub motion: Motion,
    pub close_generation: u64,
    pub tab: Tab,
    pub scroll: f32,
    pub route: Option<AudioObjectId>,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            open: false,
            mounted: false,
            motion: Motion::fixed(0.),
            close_generation: 0,
            tab: Tab::Outputs,
            scroll: 0.,
            route: None,
        }
    }
}
impl Panel {
    pub fn set_open(&mut self, open: bool, now: f64) {
        if self.open == open {
            return;
        }
        self.open = open;
        self.mounted = true;
        if !open {
            self.close_generation = self.close_generation.wrapping_add(1);
        }
        self.motion.target(
            if open { 1. } else { 0. },
            now,
            if open { motion::ENTER } else { motion::EXIT },
            motion::DECELERATE,
        );
    }
}

impl Audio {
    fn text(&self, text: impl Into<String>, theme: Theme) -> Element<Message> {
        Element::text(text)
            .color(theme.on_surface)
            .font(font::BODY)
            .width(Length::Fill)
            .wrap(token::STATUS_LINES as usize)
    }
    fn card(&self, children: Vec<Element<Message>>, theme: Theme) -> Element<Message> {
        Element::column(children)
            .width(Length::Fill)
            .padding(token::CARD_PADDING)
            .gap(token::CARD_GAP)
            .background(theme.surface)
            .radius(radius::PANEL)
            .clip()
    }
    fn level(
        &self,
        id: AudioObjectId,
        volume: AudioVolume,
        muted: bool,
        writable: bool,
        theme: Theme,
    ) -> Vec<Element<Message>> {
        let preview = self.commands.pending_volume(id);
        let shown = preview.unwrap_or(volume);
        let muted = self.commands.pending_mute(id).unwrap_or(muted);
        let mut rows = vec![
            Element::row(vec![
                self.text(
                    format!(
                        "{:.0}%{}",
                        shown.ratio() * 100.,
                        if preview.is_some() {
                            " · applying"
                        } else {
                            ""
                        }
                    ),
                    theme,
                ),
                theme
                    .button(
                        if muted { "Unmute" } else { "Mute" },
                        Message::Request(AudioCommand::SetMute(id, !muted)),
                    )
                    .id(format!("audio-mute-{}", id.value())),
            ])
            .width(Length::Fill)
            .align(Align::Center)
            .gap(space::SM),
        ];
        if writable {
            rows.push(
                theme
                    .slider(
                        SliderSpec::new(shown.ratio().min(1.) as f32, token::VOLUME_STEP)
                            .expect("validated audio volume"),
                        move |event| Message::Volume(id, event),
                    )
                    .id(format!("audio-volume-{}", id.value())),
            );
        }
        rows
    }
    fn device_card(
        &self,
        device: &AudioDevice,
        snapshot: &AudioSnapshot,
        theme: Theme,
    ) -> Element<Message> {
        let default = snapshot
            .default_device(device.direction)
            .is_some_and(|d| d.id == device.id);
        let mut rows = vec![
            self.text(&device.name, theme)
                .font(font::TITLE)
                .wrap(token::TITLE_LINES as usize),
        ];
        if device.monitor {
            rows.push(
                self.text("Records an output; not a microphone", theme)
                    .font(font::CAPTION),
            );
        }
        rows.extend(self.level(device.id, device.volume, device.muted, true, theme));
        rows.push(
            theme
                .button(
                    if default {
                        "Default device"
                    } else {
                        "Use as default"
                    },
                    Message::Request(AudioCommand::SetDefault(device.id)),
                )
                .width(Length::Fill)
                .color(if default {
                    theme.primary
                } else {
                    theme.on_surface
                })
                .id(format!("audio-default-{}", device.id.value())),
        );
        self.card(rows, theme)
    }
    fn stream_card(
        &self,
        stream: &AudioStream,
        snapshot: &AudioSnapshot,
        theme: Theme,
    ) -> Element<Message> {
        let mut rows = vec![
            self.text(
                if stream.application.is_empty() {
                    &stream.name
                } else {
                    &stream.application
                },
                theme,
            )
            .font(font::TITLE)
            .wrap(token::TITLE_LINES as usize),
            self.text(&stream.name, theme).font(font::CAPTION),
        ];
        if let Some(volume) = stream.volume {
            rows.extend(self.level(
                stream.id,
                volume,
                stream.muted,
                stream.volume_writable,
                theme,
            ));
        } else {
            rows.push(self.text("Volume control unavailable for this stream", theme));
        }
        let route = stream
            .device
            .and_then(|id| snapshot.device(id))
            .map_or("Device unavailable", |d| d.name.as_str());
        rows.push(
            theme
                .button(
                    format!(
                        "{}: {route}",
                        if stream.direction == AudioDirection::Playback {
                            "Output"
                        } else {
                            "Input"
                        }
                    ),
                    Message::ChooseRoute(stream.id),
                )
                .width(Length::Fill)
                .wrap(token::TITLE_LINES as usize)
                .id(format!("audio-route-{}", stream.id.value())),
        );
        self.card(rows, theme)
    }
    pub fn view(&self, cx: &ViewContext, theme: Theme) -> Element<Message> {
        let mut content = vec![];
        match &self.observation {
            Observation::Loading => content.push(self.card(
                vec![self.text("Connecting to the audio service…", theme)],
                theme,
            )),
            Observation::Unavailable(reason) => content.push(self.card(
                vec![
                    self.text("Audio is unavailable", theme).font(font::TITLE),
                    self.text(reason, theme),
                    self.text("The service will reconnect automatically.", theme),
                ],
                theme,
            )),
            Observation::Ready(snapshot) => {
                if let Some(stream) = self
                    .panel
                    .route
                    .and_then(|id| snapshot.streams.iter().find(|s| s.id == id))
                {
                    content.push(self.text(format!("Choose a device for {}", stream.name), theme));
                    for device in snapshot
                        .devices
                        .iter()
                        .filter(|d| d.direction == stream.direction)
                    {
                        content.push(
                            theme
                                .button(
                                    format!(
                                        "{}{}",
                                        if stream.device == Some(device.id) {
                                            "Current · "
                                        } else {
                                            ""
                                        },
                                        device.name
                                    ),
                                    Message::Request(AudioCommand::Route {
                                        stream: stream.id,
                                        device: device.id,
                                    }),
                                )
                                .background(if stream.device == Some(device.id) {
                                    theme.primary
                                } else {
                                    theme.surface
                                })
                                .color(if stream.device == Some(device.id) {
                                    theme.on_primary
                                } else {
                                    theme.on_surface
                                })
                                .width(Length::Fill)
                                .wrap(token::TITLE_LINES as usize)
                                .id(format!("audio-route-device-{}", device.id.value())),
                        );
                    }
                } else {
                    match self.panel.tab {
                        Tab::Outputs | Tab::Inputs => {
                            let direction = if self.panel.tab == Tab::Outputs {
                                AudioDirection::Playback
                            } else {
                                AudioDirection::Capture
                            };
                            content.extend(
                                snapshot
                                    .devices
                                    .iter()
                                    .filter(|d| d.direction == direction)
                                    .map(|d| self.device_card(d, snapshot, theme)),
                            );
                        }
                        Tab::Applications => content.extend(
                            snapshot
                                .streams
                                .iter()
                                .map(|s| self.stream_card(s, snapshot, theme)),
                        ),
                    }
                }
                if content.is_empty() {
                    content.push(self.card(
                        vec![self.text(
                            if self.panel.tab == Tab::Applications {
                                "No active audio applications"
                            } else {
                                "No audio devices available"
                            },
                            theme,
                        )],
                        theme,
                    ));
                }
            }
        }
        let header = Element::row(vec![
            Element::text("Audio")
                .font(font::TITLE)
                .color(theme.on_surface)
                .width(Length::Fill),
            if self.panel.route.is_some() {
                theme.button("Back", Message::Back).id("audio-back")
            } else {
                Element::empty()
            },
            theme.button("Close", Message::Close).id("audio-close"),
        ])
        .width(Length::Fill)
        .height(Length::Fixed(token::HEADER_HEIGHT))
        .align(Align::Center)
        .gap(space::SM);
        let tabs = Element::row(
            [Tab::Outputs, Tab::Inputs, Tab::Applications]
                .into_iter()
                .map(|tab| {
                    theme
                        .button(tab.name(), Message::Tab(tab))
                        .width(Length::Fill)
                        .background(if self.panel.tab == tab {
                            theme.primary
                        } else {
                            theme.surface_container
                        })
                        .color(if self.panel.tab == tab {
                            theme.on_primary
                        } else {
                            theme.on_surface
                        })
                        .id(format!("audio-tab-{}", tab.name()))
                })
                .collect(),
        )
        .width(Length::Fill)
        .height(Length::Fixed(token::TAB_HEIGHT))
        .gap(space::SM);
        let viewport = theme
            .scroll(
                ScrollSpec::new(self.panel.scroll, scroll::LINE_STEP)
                    .expect("controlled scroll offset"),
                Element::column(content).width(Length::Fill).gap(space::MD),
                Message::Scroll,
            )
            .id("audio-scroll");
        let mut children = vec![header, tabs, viewport];
        if let Some(error) = self.commands.error() {
            let can_retry = self.commands.can_retry(self.snapshot());
            let mut status = vec![
                self.text(error, theme)
                    .color(theme.error)
                    .font(font::CAPTION),
            ];
            if !can_retry {
                status.push(
                    self.text("The control changed. Select it again.", theme)
                        .font(font::CAPTION),
                );
            }
            children.push(
                Element::row(vec![
                    Element::column(status).width(Length::Fill).gap(space::XS),
                    if can_retry {
                        theme
                            .button("Retry", Message::Control(commands::Message::Retry))
                            .id("audio-retry")
                    } else {
                        theme
                            .button(
                                "Dismiss",
                                Message::Control(commands::Message::DismissFailure),
                            )
                            .id("audio-dismiss-error")
                    },
                ])
                .width(Length::Fill)
                .gap(space::SM),
            );
        } else if self.commands.busy() {
            children.push(
                self.text("Applying audio changes…", theme)
                    .font(font::CAPTION),
            );
        }
        let rect = crate::shell_layout::audio_panel((cx.width, cx.height));
        let progress = self.panel.motion.value(cx.now);
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
            .tab_stop(false)
            .id("audio-panel");
        Element::stack(vec![
            Element::empty()
                .fill()
                .on_click(Message::Close)
                .tab_stop(false)
                .id("audio-backdrop"),
            panel,
        ])
        .fill()
    }
}
