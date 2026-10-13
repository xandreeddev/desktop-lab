//! Semantic tokens and SDK controls over borrowed observations.
use super::*;
use lucent_design::{
    Theme,
    component::{power as token, scroll},
    font, radius, space,
};
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
        .tab_stop(enabled)
        .wrap(token::TITLE_LINES as usize)
        .opacity(if enabled {
            1.
        } else {
            lucent_design::opacity::MUTED
        })
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
pub(super) fn action_name(action: PowerAction) -> &'static str {
    match action {
        PowerAction::Suspend => "Suspend",
        PowerAction::Hibernate => "Hibernate",
        PowerAction::HybridSleep => "Hybrid sleep",
        PowerAction::Reboot => "Restart",
        PowerAction::PowerOff => "Power off",
    }
}
impl Power {
    fn content(&self, theme: Theme) -> Vec<Element<Message>> {
        let Some(snapshot) = self.snapshot() else {
            return vec![card(
                vec![text(
                    match &self.observation {
                        Observation::Loading => "Connecting to power services…",
                        Observation::Unavailable(error) => error,
                        Observation::Ready(_) => unreachable!(),
                    },
                    theme,
                )],
                theme,
            )];
        };
        if let Some(action) = self.confirmation {
            return vec![card(
                vec![
                    text(format!("{} this computer?", action_name(action)), theme)
                        .font(font::TITLE),
                    text(
                        "Save your work before continuing. The system may request authorization.",
                        theme,
                    ),
                    button("Back", "power-confirm-back", Message::Back, true, theme),
                    button(
                        action_name(action),
                        "power-confirm",
                        Message::Confirm(action),
                        !self.busy(),
                        theme,
                    ),
                ],
                theme,
            )];
        }
        let mut result = vec![];
        if let Some(operation) = &snapshot.operation {
            result.push(card(
                vec![text(
                    match &operation.phase {
                        PowerOperationPhase::InProgress => {
                            "Waiting for the native power operation or authorization…".into()
                        }
                        PowerOperationPhase::Succeeded => {
                            "Power request completed. Values below show the observed state.".into()
                        }
                        PowerOperationPhase::Failed(error) => {
                            format!("Power request failed: {error}")
                        }
                    },
                    theme,
                )],
                theme,
            ));
        }
        let mut battery = vec![text("Battery", theme).font(font::TITLE)];
        battery.push(text(
            match &snapshot.battery {
                PowerCapability::Ready(None) => "No display battery or UPS is present.".into(),
                PowerCapability::Ready(Some(b)) => format!(
                    "{} · {}",
                    self.label(),
                    match b.state {
                        BatteryState::Unknown => "State unknown",
                        BatteryState::Charging => "Charging",
                        BatteryState::Discharging => "Discharging",
                        BatteryState::Empty => "Empty",
                        BatteryState::Full => "Fully charged",
                        BatteryState::PendingCharge => "Waiting to charge",
                        BatteryState::PendingDischarge => "Waiting to discharge",
                    }
                ),
                PowerCapability::Unavailable(error) => error.clone(),
            },
            theme,
        ));
        if let PowerCapability::Ready(Some(b)) = &snapshot.battery {
            battery.push(
                text(
                    if b.on_battery {
                        "On battery power"
                    } else {
                        "Connected to external power"
                    },
                    theme,
                )
                .font(font::CAPTION),
            );
            if let Some(seconds) = b
                .seconds_to_empty
                .filter(|_| b.state == BatteryState::Discharging)
                .or(b
                    .seconds_to_full
                    .filter(|_| b.state == BatteryState::Charging))
            {
                battery.push(
                    text(
                        format!(
                            "Estimated {:02}h {:02}m remaining",
                            seconds / 3600,
                            (seconds % 3600) / 60
                        ),
                        theme,
                    )
                    .font(font::CAPTION),
                );
            }
        }
        result.push(card(battery, theme));
        let mut brightness = vec![text("Brightness", theme).font(font::TITLE)];
        match &snapshot.brightness {
            PowerCapability::Unavailable(error) => brightness.push(text(error, theme)),
            PowerCapability::Ready(devices) if devices.is_empty() => {
                brightness.push(text("No built-in backlight controls are available.", theme))
            }
            PowerCapability::Ready(devices) => {
                for device in devices {
                    let id = device.id;
                    let shown = self
                        .brightness
                        .filter(|(target, _)| *target == id)
                        .map(|(_, v)| v)
                        .unwrap_or(device.brightness);
                    brightness.push(
                        text(format!("{} · {:.0}%", device.name, shown.value()), theme)
                            .font(font::CAPTION),
                    );
                    brightness.push(
                        theme
                            .slider(
                                SliderSpec::new(
                                    (shown.value() / 100.) as f32,
                                    token::BRIGHTNESS_STEP,
                                )
                                .expect("validated percentage"),
                                move |event| Message::Brightness(id, event),
                            )
                            .id(format!("power-brightness-{}", id.value())),
                    );
                }
            }
        }
        result.push(card(brightness, theme));
        let mut profiles = vec![text("Power profile", theme).font(font::TITLE)];
        match &snapshot.profiles {
            PowerCapability::Unavailable(error) => profiles.push(text(error, theme)),
            PowerCapability::Ready(state) => {
                if state.profiles.is_empty() {
                    profiles.push(text("No power profiles are available.", theme));
                }
                for profile in &state.profiles {
                    let selected = state.active == Some(profile.id);
                    profiles.push(
                        button(
                            match profile.name.as_str() {
                                "power-saver" => "Power saver",
                                "balanced" => "Balanced",
                                "performance" => "Performance",
                                other => other,
                            },
                            format!("power-profile-{}", profile.id.value()),
                            Message::Control(PowerCommand::SetProfile(profile.id)),
                            !self.busy(),
                            theme,
                        )
                        .background(if selected {
                            theme.primary
                        } else {
                            theme.surface
                        })
                        .color(if selected {
                            theme.on_primary
                        } else {
                            theme.on_surface
                        }),
                    );
                }
                if state.performance_degraded.is_some() {
                    profiles.push(
                        text(
                            "Performance is limited by current hardware conditions.",
                            theme,
                        )
                        .font(font::CAPTION),
                    );
                }
            }
        }
        result.push(card(profiles, theme));
        let mut session = vec![text("Session", theme).font(font::TITLE)];
        match &snapshot.session {
            PowerCapability::Unavailable(error) => session.push(text(error, theme)),
            PowerCapability::Ready(actions) => {
                for capability in actions {
                    session.push(button(
                        action_name(capability.action),
                        format!("power-action-{:?}", capability.action),
                        Message::Ask(capability.action),
                        !self.busy()
                            && matches!(
                                capability.permission,
                                PowerPermission::Allowed | PowerPermission::Authentication
                            ),
                        theme,
                    ));
                }
            }
        }
        result.push(card(session, theme));
        result
    }
    pub fn view(&self, cx: &ViewContext, theme: Theme) -> Element<Message> {
        if !self.mounted {
            return Element::empty();
        }
        let mut children = vec![
            Element::row(vec![
                text("Power", theme).font(font::TITLE),
                button("Close", "power-close", Message::Close, true, theme).width(Length::Shrink),
            ])
            .height(Length::Fixed(token::HEADER_HEIGHT))
            .align(Align::Center),
            theme
                .scroll(
                    ScrollSpec::new(self.scroll, scroll::LINE_STEP)
                        .expect("controlled power offset"),
                    Element::column(self.content(theme))
                        .width(Length::Fill)
                        .gap(space::MD),
                    Message::Scroll,
                )
                .id("power-scroll"),
        ];
        if !self.error.is_empty() {
            children.push(
                Element::row(vec![
                    text(&self.error, theme)
                        .font(font::CAPTION)
                        .color(theme.error),
                    button("Dismiss", "power-dismiss", Message::Dismiss, true, theme)
                        .width(Length::Shrink),
                ])
                .gap(space::SM),
            );
        }
        let rect = crate::shell_layout::power_panel((cx.width, cx.height));
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
            .tab_stop(false)
            .id("power-panel");
        Element::stack(vec![
            Element::empty()
                .fill()
                .on_click(Message::Close)
                .tab_stop(false)
                .id("power-backdrop"),
            panel,
        ])
        .fill()
    }
}
