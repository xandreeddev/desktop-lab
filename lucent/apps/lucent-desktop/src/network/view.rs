//! Lucent presentation over borrowed domain observations; no service calls.
use super::{
    Control, Message, Network, Observation, Tab,
    form::{Form, Target},
};
use lucent_api::*;
use lucent_design::{
    Theme,
    component::{input, network as token, scroll},
    font, opacity, radius, space,
};
use lucent_domain::*;

pub fn security_name(security: WifiSecurity) -> &'static str {
    match security {
        WifiSecurity::Open => "Open",
        WifiSecurity::WpaPersonal => "WPA personal",
        WifiSecurity::Wpa3Personal => "WPA3 personal",
        WifiSecurity::EnhancedOpen => "Enhanced open",
        WifiSecurity::Enterprise => "Enterprise",
        WifiSecurity::Wep => "WEP",
        WifiSecurity::Unsupported => "Unsupported security",
    }
}
fn text(value: impl Into<String>, theme: Theme) -> Element<Message> {
    Element::text(value)
        .font(font::BODY)
        .color(theme.on_surface)
        .width(Length::Fill)
        .wrap(token::STATUS_LINES as usize)
}
fn card(children: Vec<Element<Message>>, theme: Theme) -> Element<Message> {
    Element::column(children)
        .width(Length::Fill)
        .padding(token::CARD_PADDING)
        .gap(token::CARD_GAP)
        .background(theme.surface_container)
        .radius(radius::PANEL)
        .clip()
}
fn button(
    label: impl Into<String>,
    id: impl Into<String>,
    message: Message,
    enabled: bool,
    theme: Theme,
) -> Element<Message> {
    theme
        .button(label, if enabled { message } else { Message::Ignore })
        .id(id)
        .width(Length::Fill)
        .wrap(token::TITLE_LINES as usize)
        .tab_stop(enabled)
        .opacity(if enabled { 1. } else { opacity::MUTED })
}
fn field_style(field: Element<Message>, theme: Theme) -> Element<Message> {
    field
        .width(Length::Fill)
        .height(Length::Fixed(input::HEIGHT))
        .padding_xy(input::PADDING_INLINE, input::PADDING_BLOCK)
        .background(theme.surface)
        .color(theme.on_surface)
        .font(font::BODY)
        .radius(radius::CONTROL)
        .clip()
}
impl Network {
    fn form_view(&self, form: &Form, snapshot: &NetworkSnapshot, theme: Theme) -> Element<Message> {
        let generation = form.generation;
        let mut rows = vec![
            text(
                if matches!(form.target, Target::Hidden { .. }) {
                    "Hidden network"
                } else {
                    &form.name
                },
                theme,
            )
            .font(font::TITLE),
        ];
        if matches!(form.target, Target::Hidden { .. }) {
            rows.push(
                theme
                    .text_input(
                        &form.name,
                        form.name_field.selection(),
                        "Network name",
                        move |name| Message::Name(generation, name),
                    )
                    .id(format!("network-name-{generation}"))
                    .autofocus()
                    .on_submit(Message::Submit(generation))
                    .background(theme.surface),
            );
            for choices in [
                [WifiSecurity::WpaPersonal, WifiSecurity::Open],
                [WifiSecurity::Wpa3Personal, WifiSecurity::EnhancedOpen],
            ] {
                rows.push(
                    Element::row(
                        choices
                            .into_iter()
                            .map(|security| {
                                button(
                                    security_name(security),
                                    format!("network-security-{security:?}"),
                                    Message::Security(generation, security),
                                    true,
                                    theme,
                                )
                                .background(if form.security == security {
                                    theme.primary
                                } else {
                                    theme.surface
                                })
                                .color(
                                    if form.security == security {
                                        theme.on_primary
                                    } else {
                                        theme.on_surface
                                    },
                                )
                            })
                            .collect(),
                    )
                    .width(Length::Fill)
                    .gap(space::SM),
                );
            }
        } else if let Target::Visible { ssid, .. } = &form.target {
            for profile in snapshot
                .profiles
                .iter()
                .filter(|p| p.ssid.as_ref() == Some(ssid))
            {
                rows.push(button(
                    format!("Use saved: {}", profile.name),
                    format!("network-saved-choice-{}", profile.id.value()),
                    Message::Request(Control::Activate(profile.id, Some(form.device()))),
                    self.active.is_none(),
                    theme,
                ));
            }
        }
        rows.push(text(security_name(form.security), theme).font(font::CAPTION));
        let supported = matches!(
            form.security,
            WifiSecurity::Open
                | WifiSecurity::EnhancedOpen
                | WifiSecurity::WpaPersonal
                | WifiSecurity::Wpa3Personal
        );
        if matches!(
            form.security,
            WifiSecurity::WpaPersonal | WifiSecurity::Wpa3Personal
        ) {
            rows.push(field_style(
                Element::masked_input(form.password_length(), "Password", move |edit| {
                    Message::Password(generation, edit)
                })
                .id(format!("network-password-{generation}"))
                .autofocus()
                .on_submit(Message::Submit(generation)),
                theme,
            ));
            rows.push(
                text(
                    if form.security == WifiSecurity::WpaPersonal {
                        "8–63 printable characters, or a 64-digit hexadecimal key."
                    } else {
                        "Enter this network's WPA3 password."
                    },
                    theme,
                )
                .font(font::CAPTION),
            );
        } else if !supported {
            rows.push(text("Choose an existing configured profile. Creating this security type is not supported yet.",theme));
        }
        rows.push(button(
            if form.remember {
                "Remember network · on"
            } else {
                "Remember network · off"
            },
            "network-remember",
            Message::Remember(generation),
            self.active.is_none(),
            theme,
        ));
        rows.push(
            Element::row(vec![
                button("Back", "network-form-back", Message::Cancel, true, theme),
                button(
                    "Connect",
                    "network-connect",
                    Message::Submit(generation),
                    supported && self.active.is_none(),
                    theme,
                )
                .background(theme.primary)
                .color(theme.on_primary),
            ])
            .width(Length::Fill)
            .gap(space::SM),
        );
        card(rows, theme)
    }
    fn available(&self, snapshot: &NetworkSnapshot, theme: Theme) -> Vec<Element<Message>> {
        let mut rows = vec![];
        let enabled =
            snapshot.wifi_enabled && snapshot.wifi_hardware_enabled && self.active.is_none();
        if !snapshot.wifi_hardware_enabled {
            rows.push(card(
                vec![text("Wi-Fi is disabled by a hardware switch.", theme)],
                theme,
            ));
        }
        let devices: Vec<_> = snapshot
            .devices
            .iter()
            .filter(|d| d.kind == NetworkKind::Wifi && d.managed)
            .collect();
        if devices.is_empty() {
            rows.push(card(
                vec![text("No managed Wi-Fi device is available.", theme)],
                theme,
            ));
            return rows;
        }
        if devices.len() > 1 {
            rows.push(
                Element::row(
                    devices
                        .iter()
                        .map(|d| {
                            button(
                                &d.interface,
                                format!("network-device-{}", d.id.value()),
                                Message::Device(d.id),
                                true,
                                theme,
                            )
                            .background(if self.device == Some(d.id) {
                                theme.primary
                            } else {
                                theme.surface_container
                            })
                            .color(if self.device == Some(d.id) {
                                theme.on_primary
                            } else {
                                theme.on_surface
                            })
                        })
                        .collect(),
                )
                .width(Length::Fill)
                .gap(space::SM),
            );
        }
        if let Some(device) = self.device.and_then(|id| snapshot.device(id)) {
            rows.push(
                text(format!("{} · {:?}", device.interface, device.state), theme)
                    .font(font::CAPTION),
            );
            if device.failure != NetworkFailure::None {
                rows.push(text(format!("Connection issue: {:?}. Select the network again to enter new credentials.",device.failure),theme).color(theme.error));
            }
            rows.push(
                Element::row(vec![
                    button(
                        if self.scan.is_some() {
                            "Scanning…"
                        } else {
                            "Scan"
                        },
                        "network-scan",
                        Message::Request(Control::Scan(device.id)),
                        enabled && self.scan.is_none(),
                        theme,
                    ),
                    button(
                        "Hidden network",
                        "network-hidden",
                        Message::Hidden,
                        enabled,
                        theme,
                    ),
                ])
                .width(Length::Fill)
                .gap(space::SM),
            );
            let mut points: Vec<_> = snapshot
                .access_points
                .iter()
                .filter(|a| Some(a.device) == self.device && a.ssid.is_some())
                .collect();
            points.sort_by_key(|a| (std::cmp::Reverse(a.strength.percent()), a.id));
            for ap in points {
                rows.push(card(
                    vec![
                        text(ap.ssid.as_ref().unwrap().display_name(), theme).font(font::TITLE),
                        text(
                            format!(
                                "{} · {}% signal",
                                security_name(ap.security),
                                ap.strength.percent()
                            ),
                            theme,
                        )
                        .font(font::CAPTION),
                        button(
                            "Choose network",
                            format!("network-ap-{}", ap.id.value()),
                            Message::Join(ap.id),
                            enabled,
                            theme,
                        ),
                    ],
                    theme,
                ));
            }
            if !snapshot
                .access_points
                .iter()
                .any(|a| Some(a.device) == self.device && a.ssid.is_some())
            {
                rows.push(card(
                    vec![text(
                        if snapshot.wifi_enabled {
                            "No networks discovered. Scan or enter a hidden name."
                        } else {
                            "Turn on Wi-Fi to discover networks."
                        },
                        theme,
                    )],
                    theme,
                ));
            }
        }
        rows
    }
    fn saved(&self, snapshot: &NetworkSnapshot, theme: Theme) -> Vec<Element<Message>> {
        let mut rows = vec![];
        for profile in &snapshot.profiles {
            let device = if profile.kind == NetworkKind::Wifi {
                self.device
            } else {
                None
            };
            let activate = Control::Activate(profile.id, device);
            rows.push(card(
                vec![
                    text(&profile.name, theme).font(font::TITLE),
                    text(format!("{:?}", profile.kind), theme).font(font::CAPTION),
                    Element::row(vec![
                        button(
                            "Connect",
                            format!("network-activate-{}", profile.id.value()),
                            Message::Request(activate.clone()),
                            self.active.is_none() && activate.valid(Some(snapshot)),
                            theme,
                        ),
                        button(
                            "Forget",
                            format!("network-forget-{}", profile.id.value()),
                            Message::AskForget(profile.id),
                            self.active.is_none(),
                            theme,
                        ),
                    ])
                    .width(Length::Fill)
                    .gap(space::SM),
                ],
                theme,
            ));
        }
        if rows.is_empty() {
            rows.push(card(vec![text("No saved connections", theme)], theme));
        }
        rows
    }
    fn connections(&self, snapshot: &NetworkSnapshot, theme: Theme) -> Vec<Element<Message>> {
        let mut rows = vec![];
        for connection in &snapshot.connections {
            rows.push(card(
                vec![
                    text(&connection.name, theme).font(font::TITLE),
                    text(
                        format!("{:?} · {:?}", connection.kind, connection.state),
                        theme,
                    )
                    .font(font::CAPTION),
                    button(
                        "Disconnect",
                        format!("network-disconnect-{}", connection.id.value()),
                        Message::Request(Control::Disconnect(connection.id)),
                        self.active.is_none(),
                        theme,
                    ),
                ],
                theme,
            ));
        }
        if rows.is_empty() {
            rows.push(card(vec![text("No active connections", theme)], theme));
        }
        rows
    }
    pub fn view(&self, cx: &ViewContext, theme: Theme) -> Element<Message> {
        let mut content = match &self.observation {
            Observation::Loading => vec![card(
                vec![text("Connecting to the network service…", theme)],
                theme,
            )],
            Observation::Unavailable(reason) => vec![card(
                vec![
                    text("Network is unavailable", theme).font(font::TITLE),
                    text(reason, theme),
                    text("The service will reconnect automatically.", theme),
                ],
                theme,
            )],
            Observation::Ready(s) => {
                if let Some(form) = &self.form {
                    vec![self.form_view(form, s, theme)]
                } else if let Some(profile) = self.forget.and_then(|id| s.profile(id)) {
                    vec![card(
                        vec![
                            text(format!("Forget {}?", profile.name), theme).font(font::TITLE),
                            text(
                                "This disconnects its active connection and removes the saved profile.",
                                theme,
                            ),
                            Element::row(vec![
                                button(
                                    "Cancel",
                                    "network-forget-cancel",
                                    Message::Cancel,
                                    true,
                                    theme,
                                ),
                                button(
                                    "Forget",
                                    "network-forget-confirm",
                                    Message::ConfirmForget,
                                    self.active.is_none(),
                                    theme,
                                )
                                .color(theme.error),
                            ])
                            .width(Length::Fill)
                            .gap(space::SM),
                        ],
                        theme,
                    )]
                } else {
                    match self.tab {
                        Tab::Available => self.available(s, theme),
                        Tab::Saved => self.saved(s, theme),
                        Tab::Connections => self.connections(s, theme),
                    }
                }
            }
        };
        if content.is_empty() {
            content.push(card(vec![text("No network information", theme)], theme));
        }
        let mut children = vec![
            Element::row(vec![
                text("Network", theme).font(font::TITLE),
                button("Close", "network-close", Message::Close, true, theme).width(Length::Shrink),
            ])
            .width(Length::Fill)
            .height(Length::Fixed(token::HEADER_HEIGHT))
            .align(Align::Center)
            .gap(space::SM),
            text(self.status(), theme).font(font::CAPTION),
        ];
        if let Some(s) = self.snapshot() {
            children.push(button(
                if s.wifi_enabled {
                    "Wi-Fi on · turn off"
                } else {
                    "Wi-Fi off · turn on"
                },
                "network-wifi",
                Message::Request(Control::Wifi(!s.wifi_enabled)),
                self.active.is_none()
                    && (!s.wifi_enabled && s.wifi_hardware_enabled || s.wifi_enabled),
                theme,
            ));
        }
        children.push(
            Element::row(
                [Tab::Available, Tab::Saved, Tab::Connections]
                    .into_iter()
                    .map(|tab| {
                        button(
                            tab.name(),
                            format!("network-tab-{}", tab.name()),
                            Message::Tab(tab),
                            true,
                            theme,
                        )
                        .background(if self.tab == tab {
                            theme.primary
                        } else {
                            theme.surface_container
                        })
                        .color(if self.tab == tab {
                            theme.on_primary
                        } else {
                            theme.on_surface
                        })
                    })
                    .collect(),
            )
            .width(Length::Fill)
            .height(Length::Fixed(token::TAB_HEIGHT))
            .gap(space::SM),
        );
        children.push(
            theme
                .scroll(
                    ScrollSpec::new(self.scroll, scroll::LINE_STEP)
                        .expect("controlled network offset"),
                    Element::column(content).width(Length::Fill).gap(space::MD),
                    Message::Scroll,
                )
                .id("network-scroll"),
        );
        if let Some(failure) = &self.failure {
            let retry = failure
                .control
                .as_ref()
                .is_some_and(|control| control.valid(self.snapshot()));
            children.push(
                Element::row(vec![
                    text(&failure.reason, theme)
                        .color(theme.error)
                        .font(font::CAPTION),
                    button(
                        if retry { "Retry" } else { "Dismiss" },
                        "network-error-action",
                        if retry {
                            Message::Retry
                        } else {
                            Message::Dismiss
                        },
                        self.active.is_none(),
                        theme,
                    )
                    .width(Length::Shrink),
                ])
                .width(Length::Fill)
                .gap(space::SM),
            );
        } else if self.active.is_some() {
            children.push(text("Submitting network request…", theme).font(font::CAPTION));
        } else if !self.notice.is_empty() {
            children.push(text(&self.notice, theme).font(font::CAPTION));
        }
        let rect = crate::shell_layout::network_panel((cx.width, cx.height));
        let progress = self.motion.value(cx.now);
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
            .id("network-panel");
        Element::stack(vec![
            Element::empty()
                .fill()
                .on_click(Message::Close)
                .tab_stop(false)
                .id("network-backdrop"),
            panel,
        ])
        .fill()
    }
}
