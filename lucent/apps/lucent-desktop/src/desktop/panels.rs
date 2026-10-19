//! Desktop foreground policy. Child components own their state/cancellation;
//! the composition root coordinates which ordinary panel has the foreground.
use super::{Desktop, Message};
use lucent_api::{Component, Effects, Element, ViewContext};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Panel {
    Launcher,
    Audio,
    Media,
    Weather,
    Network,
    Bluetooth,
    Power,
    Tray,
    LiveWallpaper,
    Notifications,
}
const PANELS: [Panel; 10] = [
    Panel::Launcher,
    Panel::Audio,
    Panel::Media,
    Panel::Weather,
    Panel::Network,
    Panel::Bluetooth,
    Panel::Power,
    Panel::Tray,
    Panel::LiveWallpaper,
    Panel::Notifications,
];

impl Panel {
    fn surface(self) -> &'static str {
        match self {
            Self::Launcher => "dock",
            Self::Audio => "audio",
            Self::Media => "media-controls",
            Self::Weather => "weather-controls",
            Self::Network => "network",
            Self::Bluetooth => "bluetooth",
            Self::Power => "power",
            Self::Tray => "tray",
            Self::LiveWallpaper => "live-wallpaper",
            Self::Notifications => "notifications",
        }
    }
}

impl Desktop {
    pub(super) fn panel_open(&self, panel: Panel) -> bool {
        match panel {
            Panel::Launcher => self.launcher.open(),
            Panel::Audio => self.audio.open(),
            Panel::Media => self.media.open(),
            Panel::Weather => self.weather.open(),
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
                Panel::Weather => effects.delegate(
                    |child| {
                        self.weather.update(crate::weather::Message::Close, child);
                    },
                    Message::Weather,
                ),
                Panel::Media => effects.delegate(
                    |e| self.media.update(crate::media::Message::Close, e),
                    Message::Media,
                ),
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

    pub(super) fn panel_surface(&self) -> Option<&'static str> {
        PANELS
            .into_iter()
            .find(|panel| self.panel_open(*panel))
            .map(Panel::surface)
    }

    pub(super) fn standalone_bar(&self, cx: &ViewContext) -> Element<Message> {
        let active = self
            .monitors
            .current()
            .map(|(_, monitor)| &monitor.instance);
        if self.bar_visible && self.panel_surface().is_some() && cx.instance.as_ref() == active {
            // Retain the native reservation, while its presentation belongs to
            // the active panel. Other monitors keep their normal bar contents.
            Element::empty()
        } else {
            self.bar(cx)
        }
    }

    pub(super) fn compose_panel_bar(
        &self,
        cx: &ViewContext,
        content: Element<Message>,
    ) -> Element<Message> {
        if !self.bar_visible || self.panel_surface() != Some(cx.surface) {
            return content;
        }
        // An exclusive layer can own pointer input outside its input region on
        // Hyprland. Compose related controls inside that native focus scope,
        // using the same bar component and SDK layout/hit testing everywhere.
        let bar_context = ViewContext {
            surface: "bar",
            instance: self
                .monitors
                .current()
                .map(|(_, monitor)| monitor.instance.clone()),
            width: cx.width,
            height: lucent_design::component::panel::BAR_HEIGHT,
            now: cx.now,
            density: cx.density,
        };
        Element::stack(vec![
            content,
            self.bar(&bar_context)
                .size(cx.width, bar_context.height)
                .at(0., 0.),
        ])
        .fill()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_api::{Application, Component, Event, Keyboard, PointerInput};
    fn open(panel: Panel) -> Message {
        match panel {
            Panel::Launcher => {
                Message::Launcher(crate::launcher::Message::Mode(crate::launcher::Mode::Apps))
            }
            Panel::Audio => Message::Audio(crate::audio::Message::Open),
            Panel::Media => Message::Media(crate::media::Message::Open),
            Panel::Weather => Message::Weather(crate::weather::Message::Open),
            Panel::Network => Message::Network(crate::network::Message::Open),
            Panel::Bluetooth => Message::Bluetooth(crate::bluetooth::Message::Open),
            Panel::Power => Message::Power(crate::power::Message::Open),
            Panel::Tray => Message::Tray(crate::tray::Message::Open),
            Panel::LiveWallpaper => Message::LiveWallpaper(crate::live_wallpaper::Message::Open),
            Panel::Notifications => Message::Notifications(crate::notifications::Message::Show),
        }
    }
    fn contains(tree: &Element<Message>, id: &str) -> bool {
        tree.id == id || tree.children.iter().any(|child| contains(child, id))
    }
    fn context(surface: &'static str) -> ViewContext {
        ViewContext {
            surface,
            instance: None,
            width: 1920.,
            height: if surface == "bar" {
                lucent_design::component::panel::BAR_HEIGHT
            } else {
                1080.
            },
            now: 1.,
            density: lucent_api::Density::ONE,
        }
    }
    #[test]
    fn weather_controls_and_widget_have_distinct_routes_with_or_without_named_outputs() {
        use lucent_api::{DisplayOutput, OutputName};
        use std::collections::BTreeSet;
        for named in [false, true] {
            let mut app = Desktop::new(crate::test_adapters::adapters());
            app.settings.layout.visible_widgets = vec!["weather".into()];
            if named {
                app.update(
                    Message::Outputs(
                        ["LEFT", "RIGHT"]
                            .into_iter()
                            .enumerate()
                            .map(|(index, name)| DisplayOutput {
                                global: index as u32 + 1,
                                name: Some(OutputName::new(name).unwrap()),
                                logical_size: Some((1920, 1080)),
                                integer_scale: 1,
                            })
                            .collect(),
                    ),
                    &mut Effects::default(),
                );
            }
            app.update(open(Panel::Weather), &mut Effects::default());
            let specs = app.surfaces();
            let addresses: BTreeSet<_> = specs.iter().map(|spec| spec.address()).collect();
            assert_eq!(
                addresses.len(),
                specs.len(),
                "duplicate native role, named={named}"
            );
            let controls = specs
                .iter()
                .find(|spec| spec.id == "weather-controls")
                .unwrap();
            assert!(controls.visible);
            assert_eq!(controls.keyboard, Keyboard::Exclusive);
            assert!(controls.instance.is_none());
            if named {
                assert_eq!(controls.output.as_ref().unwrap().as_str(), "LEFT");
            }
            let control_tree = app.view(&context("weather-controls"));
            assert!(contains(&control_tree, "weather-scroll"));
            assert!(!contains(&control_tree, "weather-details"));
            let widgets: Vec<_> = specs.iter().filter(|spec| spec.id == "weather").collect();
            assert_eq!(widgets.len(), if named { 2 } else { 1 });
            for widget in widgets {
                let mut cx = context("weather");
                cx.instance = widget.instance.clone();
                cx.width = widget.width as f32;
                cx.height = widget.height as f32;
                let tree = app.view(&cx);
                assert!(
                    contains(&tree, "weather-details"),
                    "widget routed to panel: {:?}",
                    widget.address()
                );
                assert!(!contains(&tree, "weather-scroll"));
                // Escape from a widget is not a controls-panel dismissal.
                assert!(
                    app.event(Event::Key {
                        surface: "weather",
                        key: lucent_api::Key::Escape
                    })
                    .is_none()
                );
            }
            assert!(matches!(
                app.event(Event::Key {
                    surface: "weather-controls",
                    key: lucent_api::Key::Escape
                }),
                Some(Message::Weather(crate::weather::Message::Close))
            ));
        }
    }

    #[test]
    fn bar_controls_keep_their_geometry_and_messages_in_every_panel_host() {
        use lucent_ui::{Interaction, Layout};
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.viewport = (1920., 1080.);
        let layout = Layout::new(std::sync::Arc::new(
            app.fonts()
                .into_iter()
                .map(|bytes| lucent_ui::text::Font::from_bytes(bytes, Default::default()).unwrap())
                .collect(),
        ));
        let cx = context("bar");
        let normal = layout.build(
            &app.view(&cx),
            cx.width,
            cx.height,
            &Interaction::default(),
            cx.now,
        );
        let expected = normal
            .hits
            .iter()
            .find(|hit| hit.id == "bar-audio")
            .unwrap()
            .rect;
        for panel in PANELS {
            app.update(open(panel), &mut Effects::default());
            assert!(!contains(&app.view(&context("bar")), "bar-audio"));
            let cx = context(panel.surface());
            let scene = layout.build(
                &app.view(&cx),
                cx.width,
                cx.height,
                &Interaction::default(),
                cx.now,
            );
            let hits: Vec<_> = scene
                .hits
                .iter()
                .filter(|hit| hit.id == "bar-audio")
                .collect();
            assert_eq!(hits.len(), 1, "{panel:?}");
            assert_eq!(hits[0].rect, expected, "{panel:?}");
            let (x, y) = (expected.x + expected.w / 2., expected.y + expected.h / 2.);
            let mut interaction = Interaction::default();
            interaction.press(&scene, x, y);
            assert!(matches!(
                interaction.release(x, y),
                Some(Message::Audio(crate::audio::Message::Toggle))
            ));
        }
        app.update(
            Message::Notifications(crate::notifications::Message::Hide),
            &mut Effects::default(),
        );
        assert!(contains(&app.view(&context("bar")), "bar-audio"));
        assert!(app.surfaces().iter().any(|s| s.id == "bar"
            && s.visible
            && s.exclusive_zone == lucent_design::component::panel::BAR_HEIGHT as i32));
        app.update(open(Panel::Launcher), &mut Effects::default());
        app.update(Message::Bar(false), &mut Effects::default());
        assert!(!contains(&app.view(&context("dock")), "bar-audio"));
    }
    #[test]
    fn moving_bar_contents_keeps_other_monitors_independent() {
        use lucent_api::{DisplayOutput, OutputName};
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.update(
            Message::Outputs(
                ["LEFT", "RIGHT"]
                    .into_iter()
                    .enumerate()
                    .map(|(index, name)| DisplayOutput {
                        global: index as u32 + 1,
                        name: Some(OutputName::new(name).unwrap()),
                        logical_size: Some((1920, 1080)),
                        integer_scale: 1,
                    })
                    .collect(),
            ),
            &mut Effects::default(),
        );
        let selected = lucent_domain::MonitorId::new("LEFT").unwrap();
        app.update(
            Message::SelectMonitor(selected.clone()),
            &mut Effects::default(),
        );
        app.update(open(Panel::Launcher), &mut Effects::default());
        for (id, monitor) in &app.monitors.connected {
            let mut cx = context("bar");
            cx.instance = Some(monitor.instance.clone());
            assert_eq!(contains(&app.view(&cx), "bar-audio"), *id != selected);
        }
        let selected = lucent_domain::MonitorId::new("RIGHT").unwrap();
        let mut effects = Effects::default();
        app.update(Message::SelectMonitor(selected.clone()), &mut effects);
        assert!(effects.redraw.contains("bar"));
        for (id, monitor) in &app.monitors.connected {
            let mut cx = context("bar");
            cx.instance = Some(monitor.instance.clone());
            assert_eq!(contains(&app.view(&cx), "bar-audio"), *id != selected);
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
                assert_eq!(exclusive[0].pointer_input, PointerInput::All);
                assert_eq!(exclusive[0].redraw_group, Some("bar"));
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
