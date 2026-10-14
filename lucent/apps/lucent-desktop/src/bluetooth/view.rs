//! Token-based presentation over borrowed observations, never native operations.
use super::{Bluetooth, Message, Observation, Tab};
use lucent_api::*;
use lucent_design::{
    Theme,
    component::{bluetooth as token, input, scroll},
    font, opacity, radius, space,
};
use lucent_domain::*;

fn text(value: impl Into<String>, theme: Theme) -> Element<Message> {
    Element::text(value)
        .font(font::BODY)
        .color(theme.on_surface)
        .width(Length::Fill)
        .wrap(token::STATUS_LINES as usize)
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
fn card(children: Vec<Element<Message>>, theme: Theme) -> Element<Message> {
    Element::column(children)
        .width(Length::Fill)
        .padding(token::CARD_PADDING)
        .gap(token::CARD_GAP)
        .background(theme.surface_container)
        .radius(radius::PANEL)
        .clip()
}
fn row(children: Vec<Element<Message>>) -> Element<Message> {
    Element::row(children).width(Length::Fill).gap(space::SM)
}
impl Bluetooth {
    fn pairing_view(
        &self,
        request: &BluetoothPairingRequest,
        snapshot: &BluetoothSnapshot,
        theme: Theme,
    ) -> Element<Message> {
        let id = request.id;
        let mut children = vec![
            text(
                snapshot
                    .devices
                    .iter()
                    .find(|d| d.id == request.device)
                    .map(|d| d.name.as_str())
                    .unwrap_or("Bluetooth device"),
                theme,
            )
            .font(font::TITLE),
        ];
        let description = match &request.prompt {
            BluetoothPairingPrompt::PinCode => "Enter this device's PIN (1–16 letters or digits).",
            BluetoothPairingPrompt::Passkey => {
                "Enter the passkey shown on this device (up to six digits)."
            }
            BluetoothPairingPrompt::ConfirmPasskey(_) => {
                "Confirm that the code below matches the code on the other device."
            }
            BluetoothPairingPrompt::DisplayPinCode(_) => {
                "Type this code on the other device, then press Enter there."
            }
            BluetoothPairingPrompt::DisplayPasskey { .. } => {
                "Type this code on the other device, then press Enter there."
            }
            BluetoothPairingPrompt::AuthorizePairing => {
                "Allow this device to pair? It will remain untrusted until you explicitly change trust."
            }
            BluetoothPairingPrompt::AuthorizeService(_) => {
                "Allow this device to use the Bluetooth service below?"
            }
        };
        children.push(text(description, theme));
        if let BluetoothPairingPrompt::ConfirmPasskey(code)
        | BluetoothPairingPrompt::DisplayPinCode(code)
        | BluetoothPairingPrompt::DisplayPasskey { code, .. } = &request.prompt
        {
            children.push(
                text(code.digits(), theme)
                    .font(token::CODE_FONT)
                    .color(theme.primary),
            );
        }
        if let BluetoothPairingPrompt::DisplayPasskey { entered, .. } = &request.prompt {
            children.push(
                text(
                    format!("{entered} of 6 digits entered on the device"),
                    theme,
                )
                .font(font::CAPTION),
            );
        }
        if let BluetoothPairingPrompt::AuthorizeService(service) = &request.prompt {
            children.push(text(service.as_str(), theme).font(font::CAPTION));
        }
        if matches!(
            request.prompt,
            BluetoothPairingPrompt::PinCode | BluetoothPairingPrompt::Passkey
        ) {
            let current = self.form.as_ref().is_some_and(|f| f.request == id);
            let count = self
                .form
                .as_ref()
                .filter(|f| f.request == id)
                .map(|f| f.length())
                .unwrap_or(0);
            if current {
                children.push(
                    Element::masked_input(count, "PIN / passkey", move |edit| {
                        Message::Edit(id, edit)
                    })
                    .id(format!("bluetooth-input-{}", id.value()))
                    .autofocus()
                    .on_submit(Message::Submit(id))
                    .width(Length::Fill)
                    .height(Length::Fixed(input::HEIGHT))
                    .padding_xy(input::PADDING_INLINE, input::PADDING_BLOCK)
                    .background(theme.surface)
                    .color(theme.on_surface)
                    .font(font::BODY)
                    .radius(radius::CONTROL)
                    .clip(),
                );
            } else {
                children.push(
                    text(
                        "Finish the earlier request before entering this code.",
                        theme,
                    )
                    .font(font::CAPTION),
                );
            }
        }
        let display = matches!(
            request.prompt,
            BluetoothPairingPrompt::DisplayPinCode(_)
                | BluetoothPairingPrompt::DisplayPasskey { .. }
        );
        let mut actions = vec![button(
            if display { "Cancel pairing" } else { "Decline" },
            format!("bluetooth-decline-{}", id.value()),
            Message::Decision(id, false),
            self.answering.is_none(),
            theme,
        )];
        if !display {
            let entry = matches!(
                request.prompt,
                BluetoothPairingPrompt::PinCode | BluetoothPairingPrompt::Passkey
            );
            actions.push(
                button(
                    if entry { "Send" } else { "Accept" },
                    format!("bluetooth-accept-{}", id.value()),
                    if entry {
                        Message::Submit(id)
                    } else {
                        Message::Decision(id, true)
                    },
                    self.answering.is_none()
                        && (!entry || self.form.as_ref().is_some_and(|f| f.request == id)),
                    theme,
                )
                .background(theme.primary)
                .color(theme.on_primary),
            );
        }
        children.push(row(actions));
        card(children, theme)
    }
    fn ready_content(&self, snapshot: &BluetoothSnapshot, theme: Theme) -> Vec<Element<Message>> {
        let mut children = vec![];
        for request in &snapshot.pairing_requests {
            children.push(self.pairing_view(request, snapshot, theme));
        }
        if let Some(admission) = &self.pending_connection {
            children.push(card(
                vec![
                    text(
                        if admission.cancelled {
                            "Cancelling the submitted connection…"
                        } else {
                            "Submitting a connection request…"
                        },
                        theme,
                    ),
                    button(
                        "Cancel connection",
                        "bluetooth-cancel-connection-admission",
                        Message::CancelConnectionAdmission,
                        !admission.cancelled && self.answering.is_none(),
                        theme,
                    ),
                ],
                theme,
            ));
        }
        if let Some(attempt) = &snapshot.connection {
            let mut status = vec![text(
                match &attempt.phase {
                    BluetoothConnectionPhase::Connecting => "Connecting to the device…".into(),
                    BluetoothConnectionPhase::Succeeded => {
                        "Connection method completed. Observe the device and its resolved services."
                            .into()
                    }
                    BluetoothConnectionPhase::Cancelled => "Connection cancelled.".into(),
                    BluetoothConnectionPhase::Failed(error) => {
                        format!("Connection failed: {error}")
                    }
                },
                theme,
            )];
            if attempt.phase == BluetoothConnectionPhase::Connecting {
                status.push(button(
                    "Cancel connection",
                    "bluetooth-cancel-connection",
                    Message::CancelConnection(attempt.id),
                    self.answering.is_none(),
                    theme,
                ));
            }
            children.push(card(status, theme));
        }
        if let Some(attempt) = &snapshot.pairing {
            let mut status = vec![text(
                match &attempt.phase {
                    BluetoothPairingPhase::Pairing => "Pairing with the device…".into(),
                    BluetoothPairingPhase::Succeeded => {
                        "Pairing completed. Connect and trust are separate controls.".into()
                    }
                    BluetoothPairingPhase::Cancelled => "Pairing cancelled.".into(),
                    BluetoothPairingPhase::Failed(error) => format!("Pairing failed: {error}"),
                },
                theme,
            )];
            if attempt.phase == BluetoothPairingPhase::Pairing {
                status.push(button(
                    "Cancel pairing",
                    "bluetooth-cancel-pairing",
                    Message::CancelPairing(attempt.id),
                    self.answering.is_none(),
                    theme,
                ));
            }
            children.push(card(status, theme));
        }
        if let Some(id) = self.forget {
            if let Some(device) = snapshot.devices.iter().find(|d| d.id == id) {
                children.push(card(
                    vec![
                        text(format!("Forget {}?", device.name), theme).font(font::TITLE),
                        text(
                            "This removes the saved pairing information for this device.",
                            theme,
                        ),
                        row(vec![
                            button("Back", "bluetooth-forget-back", Message::Back, true, theme),
                            button(
                                "Forget device",
                                "bluetooth-forget-confirm",
                                Message::ConfirmForget,
                                !self.ordinary_busy(),
                                theme,
                            )
                            .background(theme.surface_container)
                            .color(theme.error),
                        ]),
                    ],
                    theme,
                ));
            }
            return children;
        }
        if snapshot.adapters.is_empty() {
            children.push(card(vec![text("No Bluetooth controller is available. Connect a supported controller to use Bluetooth.",theme)],theme));
            return children;
        }
        for controller in &snapshot.adapters {
            let selected = Some(controller.id) == self.controller;
            children.push(
                button(
                    &controller.name,
                    format!("bluetooth-controller-{}", controller.id.value()),
                    Message::Controller(controller.id),
                    true,
                    theme,
                )
                .background(if selected {
                    theme.primary
                } else {
                    theme.surface_container
                })
                .color(if selected {
                    theme.on_primary
                } else {
                    theme.on_surface
                }),
            );
        }
        if let Some(controller) = snapshot
            .adapters
            .iter()
            .find(|a| Some(a.id) == self.controller)
        {
            let mut controls = vec![
                row(vec![
                    button(
                        if controller.powered {
                            "Power off"
                        } else {
                            "Power on"
                        },
                        "bluetooth-power",
                        Message::Control(BluetoothCommand::SetPowered {
                            adapter: controller.id,
                            powered: !controller.powered,
                        }),
                        !self.ordinary_busy(),
                        theme,
                    ),
                    button(
                        if controller.discovery_owned {
                            "Stop scan"
                        } else {
                            "Scan"
                        },
                        "bluetooth-discovery",
                        Message::Control(BluetoothCommand::SetDiscovery {
                            adapter: controller.id,
                            active: !controller.discovery_owned,
                        }),
                        controller.powered && !self.ordinary_busy(),
                        theme,
                    ),
                ]),
                text(
                    if controller.discovery_owned {
                        "Scanning for nearby devices"
                    } else if controller.discovering {
                        "Another application is scanning"
                    } else {
                        "Discovery idle"
                    },
                    theme,
                )
                .font(font::CAPTION),
            ];
            if self.tab == Tab::Radio {
                controls.insert(
                    0,
                    text(controller.address.to_string(), theme).font(font::CAPTION),
                );
                controls.push(row(vec![
                    button(
                        if controller.discoverable {
                            "Hide"
                        } else {
                            "Show"
                        },
                        "bluetooth-visibility",
                        Message::Control(BluetoothCommand::SetDiscoverable {
                            adapter: controller.id,
                            discoverable: !controller.discoverable,
                        }),
                        controller.powered && !self.ordinary_busy(),
                        theme,
                    ),
                    button(
                        if controller.pairable {
                            "Block pairs"
                        } else {
                            "Allow pairs"
                        },
                        "bluetooth-pairable",
                        Message::Control(BluetoothCommand::SetPairable {
                            adapter: controller.id,
                            pairable: !controller.pairable,
                        }),
                        !self.ordinary_busy(),
                        theme,
                    ),
                ]));
                controls.push(
                    text(
                        if controller.discoverable {
                            "Visible to nearby devices"
                        } else {
                            "Not advertised to nearby devices"
                        },
                        theme,
                    )
                    .font(font::CAPTION),
                );
                controls.push(
                    text(
                        "Only incoming pairs are affected. Visibility may expire automatically.",
                        theme,
                    )
                    .font(font::CAPTION),
                );
            }
            children.push(card(controls, theme));
        }
        let pairing = snapshot
            .pairing
            .as_ref()
            .is_some_and(|a| a.phase == BluetoothPairingPhase::Pairing);
        if self.tab == Tab::Radio {
            children.push(card(vec![
                button(if snapshot.incoming_pairing_enabled { "Handle requests · on" }
                    else { "Handle incoming here" }, "bluetooth-incoming",
                    Message::Incoming(!snapshot.incoming_pairing_enabled),
                    !pairing && !self.ordinary_busy(), theme),
                text("Let Lucent handle incoming requests. Visibility and radio permission are separate. Each request needs your decision.",theme).font(font::CAPTION),
            ], theme));
            return children;
        }
        let devices: Vec<_> = snapshot
            .devices
            .iter()
            .filter(|d| {
                Some(d.adapter) == self.controller && (self.tab == Tab::Available || d.paired)
            })
            .collect();
        if devices.is_empty() {
            children.push(card(
                vec![text(
                    if self.tab == Tab::Paired {
                        "No paired devices"
                    } else {
                        "No devices discovered. Power on the controller and start discovery."
                    },
                    theme,
                )],
                theme,
            ));
        }
        for device in devices {
            let mut details = vec![
                if device.connected {
                    "Connected"
                } else if device.paired {
                    "Paired"
                } else {
                    "Not paired"
                }
                .to_owned(),
            ];
            if device.trusted {
                details.push("Trusted".into());
            }
            if device.blocked {
                details.push("Blocked".into());
            }
            if let Some(battery) = device.battery_percent {
                details.push(format!("Battery {battery}%"));
            }
            if let Some(rssi) = device.rssi {
                details.push(format!("{rssi} dBm"));
            }
            let mut actions = vec![];
            if device.connected || device.paired {
                let action = if device.connected {
                    Message::Control(BluetoothCommand::Disconnect(device.id))
                } else {
                    Message::Connect(device.id)
                };
                let valid = if device.connected {
                    lucent_usecases::bluetooth::validate_bluetooth_command(
                        snapshot,
                        &BluetoothCommand::Disconnect(device.id),
                    )
                } else {
                    lucent_usecases::bluetooth::validate_connection_target(snapshot, device.id)
                }
                .is_ok();
                actions.push(button(
                    if device.connected {
                        "Disconnect"
                    } else {
                        "Connect"
                    },
                    format!("bluetooth-connect-{}", device.id.value()),
                    action,
                    valid && !self.ordinary_busy(),
                    theme,
                ));
            }
            if device.paired {
                actions.push(button(
                    if device.trusted {
                        "Remove trust"
                    } else {
                        "Trust device"
                    },
                    format!("bluetooth-trust-{}", device.id.value()),
                    Message::Control(BluetoothCommand::SetTrusted {
                        device: device.id,
                        trusted: !device.trusted,
                    }),
                    !self.ordinary_busy(),
                    theme,
                ));
            } else {
                actions.push(button(
                    "Pair device",
                    format!("bluetooth-pair-{}", device.id.value()),
                    Message::Pair(device.id),
                    !self.ordinary_busy()
                        && !pairing
                        && snapshot.pairing_requests.is_empty()
                        && lucent_usecases::bluetooth::validate_pairing_target(snapshot, device.id)
                            .is_ok(),
                    theme,
                ));
            }
            let mut content = vec![
                text(&device.name, theme).font(font::TITLE),
                text(details.join(" · "), theme).font(font::CAPTION),
                row(actions),
            ];
            content.push(button(
                "Forget device",
                format!("bluetooth-forget-{}", device.id.value()),
                Message::AskForget(device.id),
                !self.ordinary_busy() && !pairing,
                theme,
            ));
            children.push(card(content, theme));
        }
        children
    }
    pub fn view(&self, cx: &ViewContext, theme: Theme) -> Element<Message> {
        if !self.mounted {
            return Element::empty();
        }
        let content = match &self.observation {
            Observation::Loading => vec![card(
                vec![text("Connecting to the Bluetooth service…", theme)],
                theme,
            )],
            Observation::Unavailable(error) => vec![card(
                vec![
                    text(error, theme).color(theme.error),
                    text(
                        "Waiting for the native service to return. No device change will be replayed automatically.",
                        theme,
                    ),
                ],
                theme,
            )],
            Observation::Ready(snapshot) => self.ready_content(snapshot, theme),
        };
        let mut children = vec![
            row(vec![
                text("Bluetooth", theme).font(font::TITLE),
                button("Close", "bluetooth-close", Message::Close, true, theme)
                    .width(Length::Shrink),
            ])
            .height(Length::Fixed(token::HEADER_HEIGHT))
            .align(Align::Center),
            text(self.status(), theme).font(font::CAPTION),
            row([Tab::Available, Tab::Paired, Tab::Radio]
                .into_iter()
                .map(|tab| {
                    button(
                        match tab {
                            Tab::Available => "Devices",
                            Tab::Paired => "Paired",
                            Tab::Radio => "Radio",
                        },
                        match tab {
                            Tab::Available => "bluetooth-tab-devices",
                            Tab::Paired => "bluetooth-tab-paired",
                            Tab::Radio => "bluetooth-tab-radio",
                        },
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
                .collect())
            .height(Length::Fixed(token::TAB_HEIGHT)),
            theme
                .scroll(
                    ScrollSpec::new(self.scroll, scroll::LINE_STEP)
                        .expect("controlled Bluetooth offset"),
                    Element::column(content).width(Length::Fill).gap(space::MD),
                    Message::Scroll,
                )
                .id("bluetooth-scroll"),
        ];
        if !self.error.is_empty() {
            children.push(row(vec![
                text(&self.error, theme)
                    .font(font::CAPTION)
                    .color(theme.error),
                button(
                    "Dismiss",
                    "bluetooth-error-dismiss",
                    Message::Dismiss,
                    true,
                    theme,
                )
                .width(Length::Shrink),
            ]));
        } else if self.active.is_some() || self.answering.is_some() {
            children.push(text("Submitting request…", theme).font(font::CAPTION));
        } else if !self.notice.is_empty() {
            children.push(text(&self.notice, theme).font(font::CAPTION));
        }
        let rect = crate::shell_layout::bluetooth_panel((cx.width, cx.height));
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
            .id("bluetooth-panel");
        Element::stack(vec![
            Element::empty()
                .fill()
                .on_click(Message::Close)
                .tab_stop(false)
                .id("bluetooth-backdrop"),
            panel,
        ])
        .fill()
    }
}
