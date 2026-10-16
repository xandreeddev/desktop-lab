//! Desktop foreground policy. Child components own their state/cancellation;
//! the composition root coordinates which ordinary panel has the foreground.
use super::{Desktop, Message};
use lucent_api::{Component, Effects, PointerInput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Panel {
    Launcher,
    Audio,
    Network,
    Bluetooth,
    Power,
    Tray,
    LiveWallpaper,
    Notifications,
}
const PANELS: [Panel; 8] = [
    Panel::Launcher,
    Panel::Audio,
    Panel::Network,
    Panel::Bluetooth,
    Panel::Power,
    Panel::Tray,
    Panel::LiveWallpaper,
    Panel::Notifications,
];

impl Desktop {
    pub(super) fn panel_open(&self, panel: Panel) -> bool {
        match panel {
            Panel::Launcher => self.launcher.open(),
            Panel::Audio => self.audio.open(),
            Panel::Network => self.network.open(),
            Panel::Bluetooth => self.bluetooth.open(),
            Panel::Power => self.power.open(),
            Panel::Tray => self.tray.open(),
            Panel::LiveWallpaper => self.live_wallpaper.open,
            Panel::Notifications => self.notifications.history_open(),
        }
    }

    /// React to observed child state, including asynchronous prompts/errors.
    /// Repeated updates inside the same open panel do not cancel other work.
    pub(super) fn panel_opened(
        &mut self,
        panel: Panel,
        was_open: bool,
        effects: &mut Effects<Message>,
    ) {
        if was_open || !self.panel_open(panel) {
            return;
        }
        for other in PANELS.into_iter().filter(|other| *other != panel) {
            match other {
                Panel::Launcher => self.update_launcher(crate::launcher::Message::Close, effects),
                Panel::LiveWallpaper => {
                    self.update_live_wallpaper(crate::live_wallpaper::Message::Close, effects)
                }
                Panel::Audio => effects.delegate(
                    |e| self.audio.update(crate::audio::Message::Close, e),
                    Message::Audio,
                ),
                Panel::Network => effects.delegate(
                    |e| self.network.update(crate::network::Message::Close, e),
                    Message::Network,
                ),
                Panel::Bluetooth => effects.delegate(
                    |e| self.bluetooth.update(crate::bluetooth::Message::Close, e),
                    Message::Bluetooth,
                ),
                Panel::Power => effects.delegate(
                    |e| self.power.update(crate::power::Message::Close, e),
                    Message::Power,
                ),
                // Close also retires a direct bar action whose panel is closed.
                Panel::Tray => effects.delegate(
                    |e| self.tray.update(crate::tray::Message::Close, e),
                    Message::Tray,
                ),
                Panel::Notifications if self.notifications.history_open() => effects.delegate(
                    |e| {
                        self.notifications
                            .update(crate::notifications::Message::Hide, e)
                    },
                    Message::Notifications,
                ),
                Panel::Notifications => (),
            }
        }
    }

    pub(super) fn panel_pointer_input(&self) -> PointerInput {
        PointerInput::Inset {
            top: if self.bar_visible {
                lucent_design::component::panel::BAR_HEIGHT as u32
            } else {
                0
            },
            right: 0,
            bottom: 0,
            left: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_api::{Application, Component, Event, Keyboard};
    fn open(panel: Panel) -> Message {
        match panel {
            Panel::Launcher => {
                Message::Launcher(crate::launcher::Message::Mode(crate::launcher::Mode::Apps))
            }
            Panel::Audio => Message::Audio(crate::audio::Message::Open),
            Panel::Network => Message::Network(crate::network::Message::Open),
            Panel::Bluetooth => Message::Bluetooth(crate::bluetooth::Message::Open),
            Panel::Power => Message::Power(crate::power::Message::Open),
            Panel::Tray => Message::Tray(crate::tray::Message::Open),
            Panel::LiveWallpaper => Message::LiveWallpaper(crate::live_wallpaper::Message::Open),
            Panel::Notifications => Message::Notifications(crate::notifications::Message::Show),
        }
    }
    #[test]
    fn every_ordinary_panel_can_replace_every_other_panel_without_two_keyboard_owners() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        for previous in PANELS {
            for next in PANELS {
                app.update(open(previous), &mut Effects::default());
                assert!(app.panel_open(previous));
                app.update(open(next), &mut Effects::default());
                for panel in PANELS {
                    assert_eq!(
                        app.panel_open(panel),
                        panel == next,
                        "{previous:?} to {next:?}: {panel:?}"
                    );
                }
                let exclusive: Vec<_> = app
                    .surfaces()
                    .into_iter()
                    .filter(|s| s.visible && s.keyboard == Keyboard::Exclusive)
                    .collect();
                assert_eq!(exclusive.len(), 1, "{previous:?} to {next:?}");
                assert_eq!(exclusive[0].pointer_input, app.panel_pointer_input());
            }
        }
    }
    #[test]
    fn repeated_open_and_background_observation_do_not_retire_unrelated_tray_action() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.update(
            Message::Tray(crate::tray::Message::Observed(Ok(
                crate::tray::tests::fixture(),
            ))),
            &mut Effects::default(),
        );
        app.update(open(Panel::Audio), &mut Effects::default());
        app.update(
            Message::Tray(crate::tray::Message::Bar(
                lucent_domain::TrayItemId::new(1).unwrap(),
                crate::tray::BarAction::Activate,
            )),
            &mut Effects::default(),
        );
        assert_eq!(app.tray.inspect()["busy"], true);
        app.update(open(Panel::Audio), &mut Effects::default());
        app.update(
            Message::Notifications(crate::notifications::Message::Snapshot(Ok(
                Default::default(),
            ))),
            &mut Effects::default(),
        );
        assert_eq!(app.tray.inspect()["busy"], true);
        assert!(app.audio.open());
        app.update(open(Panel::Network), &mut Effects::default());
        assert_eq!(app.tray.inspect()["busy"], false);
        assert!(!app.audio.open());
    }
    #[test]
    fn explicit_close_commands_dismiss_ui_without_replacing_preferences() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        let preferences = serde_json::to_value(&app.settings).unwrap();
        for command in ["notifications hide", "notifications close"] {
            app.update(open(Panel::Notifications), &mut Effects::default());
            let message = app.command(command).unwrap().unwrap();
            assert!(matches!(
                message,
                Message::Notifications(crate::notifications::Message::Hide)
            ));
            app.update(message, &mut Effects::default());
            assert!(!app.notifications.history_open());
        }
        app.update(open(Panel::LiveWallpaper), &mut Effects::default());
        let message = app.command("wallpaper live close").unwrap().unwrap();
        assert!(matches!(
            message,
            Message::LiveWallpaper(crate::live_wallpaper::Message::Close)
        ));
        app.update(message, &mut Effects::default());
        assert!(!app.live_wallpaper.open);
        assert_eq!(serde_json::to_value(&app.settings).unwrap(), preferences);
    }

    #[test]
    fn notification_history_takes_keyboard_focus_and_outside_click_only_hides_history() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.update(open(Panel::Notifications), &mut Effects::default());
        let message = app
            .event(Event::Outside {
                surface: "notifications",
            })
            .unwrap();
        app.update(message, &mut Effects::default());
        assert!(!app.notifications.history_open());
        assert!(
            app.event(Event::Outside {
                surface: "notifications"
            })
            .is_none()
        );
        app.bar_visible = false;
        assert_eq!(
            app.panel_pointer_input(),
            PointerInput::Inset {
                top: 0,
                right: 0,
                bottom: 0,
                left: 0
            }
        );
    }
    #[test]
    fn fading_history_releases_input_before_its_final_frame() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.update(open(Panel::Notifications), &mut Effects::default());
        let mut effects = Effects {
            now: 1.,
            ..Effects::default()
        };
        app.update(open(Panel::Audio), &mut effects);
        assert!(app.notifications.history_closing());
        let history = app
            .surfaces()
            .into_iter()
            .find(|s| s.id == "notifications")
            .unwrap();
        assert!(history.visible);
        assert_eq!(history.keyboard, Keyboard::None);
        assert_eq!(history.pointer_input, PointerInput::None);
        assert!(app.audio.open());
    }

    #[test]
    fn live_wallpaper_panel_leaves_the_bar_clear_and_uses_the_shell_grid() {
        use lucent_design::{component::panel, layout};
        for (width, height) in [(390., 580.), (960., 580.), (1920., 1080.), (640., 320.)] {
            let bounds = crate::shell_layout::live_wallpaper_panel((width, height));
            assert!(bounds.y >= panel::BAR_HEIGHT + layout::SHELL_INSET);
            assert!(bounds.x >= layout::SHELL_INSET);
            assert!(bounds.x + bounds.w <= width - layout::SHELL_INSET);
            assert!(bounds.y + bounds.h <= height - layout::SHELL_INSET);
            for value in [bounds.x, bounds.y, bounds.w, bounds.h] {
                assert_eq!(value % layout::SHELL_STEP, 0.);
            }
        }
    }
}
