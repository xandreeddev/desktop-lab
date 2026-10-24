//! Desktop foreground policy. Child components own their state/cancellation;
//! the composition root coordinates which ordinary panel has the foreground.
use super::bar_presentation::Carrier;
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
    Displays,
    Tray,
    LiveWallpaper,
    Notifications,
}
const PANELS: [Panel; 11] = [
    Panel::Launcher,
    Panel::Audio,
    Panel::Media,
    Panel::Weather,
    Panel::Network,
    Panel::Bluetooth,
    Panel::Power,
    Panel::Displays,
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
            Self::Displays => "displays",
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
            Panel::Displays => self.displays.open(),
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
            self.close_panel(other, effects);
        }
    }

    /// Dismiss ordinary foreground UI through each child's lifecycle.
    /// Visit closed children too: an admission or direct bar action may be pending.
    pub(super) fn dismiss_panels(&mut self, effects: &mut Effects<Message>) {
        for panel in PANELS {
            self.close_panel(panel, effects);
        }
    }

    fn close_panel(&mut self, panel: Panel, effects: &mut Effects<Message>) {
        match panel {
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
            Panel::Displays => effects.delegate(
                |e| self.displays.update(crate::displays::Message::Close, e),
                Message::Displays,
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
            // Toast-origin work may await its own ack with history closed.
            // A new foreground owner retires that queued handoff as well.
            Panel::Notifications => {
                if self.notifications.cancel_link() {
                    effects.redraw("notifications");
                }
            }
        }
    }

    pub(super) fn panel_surface(&self) -> Option<&'static str> {
        PANELS
            .into_iter()
            .find(|panel| self.panel_open(*panel))
            .map(Panel::surface)
    }

    pub(super) fn reconcile_bar_presentation(&mut self, now: f64) -> bool {
        let displays = if self.monitors.legacy() {
            vec![Carrier {
                role: "bar",
                generation: 0,
                instance: None,
                output: None,
            }]
        } else {
            self.monitors
                .connected
                .values()
                .map(|monitor| Carrier {
                    role: "bar",
                    generation: monitor.generation,
                    instance: Some(monitor.instance.clone()),
                    output: Some(monitor.target.clone()),
                })
                .collect()
        };
        let active = self
            .monitors
            .current()
            .map(|(_, monitor)| monitor.instance.clone());
        let panel = self.panel_surface();
        self.bar_presentation
            .reconcile(displays, active.as_ref(), panel, self.bar_visible, now)
    }

    pub(crate) fn dock_viewport(&self) -> (f32, f32) {
        self.bar_presentation
            .retained("dock")
            .and_then(|carrier| carrier.instance.as_ref())
            .and_then(|instance| {
                self.monitors
                    .connected
                    .values()
                    .find(|monitor| &monitor.instance == instance)
            })
            .map_or(self.viewport, |monitor| monitor.viewport)
    }

    pub(super) fn standalone_bar(&self, cx: &ViewContext) -> Element<Message> {
        if self.bar_visible && self.bar_presentation.standalone(cx.instance.as_ref()) {
            self.bar(cx)
        } else {
            // Keep exactly the same reserving native surface throughout handoff.
            Element::empty()
        }
    }

    pub(super) fn carrier_bar(&self, cx: &ViewContext, carrier: &Carrier) -> Element<Message> {
        let bar_context = ViewContext {
            surface: "bar",
            instance: carrier.instance.clone(),
            width: cx.width,
            height: lucent_design::component::panel::BAR_HEIGHT,
            now: cx.now,
            density: cx.density,
        };
        self.bar(&bar_context)
            .size(cx.width, bar_context.height)
            .at(0., 0.)
    }

    pub(super) fn retain_bar_carriers(&self, surfaces: &mut [lucent_api::SurfaceSpec]) {
        use lucent_api::{Anchor, Keyboard, Layer, PointerInput};
        for surface in surfaces {
            if let Some(carrier) = self.bar_presentation.retained(surface.id) {
                // The child may already be unmounted. Keep only its last bar's
                // native carrier until the next owner submits; never retain its
                // controls, keyboard ownership or child operations.
                surface.output = carrier.output.clone();
                surface.visible = true;
                surface.layer = Layer::Overlay;
                surface.anchor = Anchor::Fill;
                surface.width = 0;
                surface.height = 0;
                surface.keyboard = Keyboard::None;
                surface.pointer_input = PointerInput::None;
                surface.exclusive_zone = -1;
                surface.redraw_group = Some("bar");
            }
        }
    }

    pub(super) fn compose_panel_bar(
        &self,
        cx: &ViewContext,
        content: Element<Message>,
    ) -> Element<Message> {
        if let Some(carrier) = self.bar_presentation.retained(cx.surface) {
            // Closed children keep their exit paint. On output retarget, the
            // still-open child belongs to the incoming output, so only its old
            // bar remains on this carrier. Neither case retains action authority.
            let content = if self.panel_surface() == Some(cx.surface) {
                Element::empty()
            } else {
                content
            };
            return Element::stack(vec![content, self.carrier_bar(cx, carrier)])
                .fill()
                .map(|_| Message::Ignore);
        }
        let Some(carrier) = self
            .bar_presentation
            .incoming(cx.surface)
            .filter(|_| self.bar_visible)
        else {
            return content;
        };
        // The exclusive panel still owns the real controls on Hyprland. Keep
        // the previous pixel owner until this complete incoming buffer submits.
        // Semantic capsule surfaces are opaque; overlap is finite admission
        // state, not an opacity animation or a cross-surface atomicity claim.
        Element::stack(vec![content, self.carrier_bar(cx, carrier)]).fill()
    }
}

#[cfg(test)]
mod dismissal_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_api::{Application, Component, Event, Keyboard, PointerInput};
    pub(super) fn open(panel: Panel) -> Message {
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
            Panel::Displays => Message::Displays(crate::displays::Message::Open),
            Panel::Tray => Message::Tray(crate::tray::Message::Open),
            Panel::LiveWallpaper => Message::LiveWallpaper(crate::live_wallpaper::Message::Open),
            Panel::Notifications => Message::Notifications(crate::notifications::Message::Show),
        }
    }
    fn contains(tree: &Element<Message>, id: &str) -> bool {
        tree.id == id || tree.children.iter().any(|child| contains(child, id))
    }
    fn submitted(
        app: &mut Desktop,
        surface: &'static str,
        instance: Option<lucent_api::SurfaceInstance>,
    ) {
        submitted_at(app, surface, instance, 2.);
    }
    fn submitted_at(
        app: &mut Desktop,
        surface: &'static str,
        instance: Option<lucent_api::SurfaceInstance>,
        now: f64,
    ) -> Effects<Message> {
        let event = Event::FrameSubmitted {
            surface,
            width: 1920.,
            height: 1080.,
            now,
        };
        let event = instance.map_or(event.clone(), |instance| Event::Scoped {
            instance,
            event: Box::new(event),
        });
        let mut effects = Effects {
            now,
            ..Default::default()
        };
        if let Some(message) = app.event(event) {
            app.update(message, &mut effects);
        }
        effects
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
    fn opening_and_switching_keep_pixels_but_retire_old_panel_controls() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        let preferences = serde_json::to_value(&app.settings).unwrap();
        app.update(open(Panel::Audio), &mut Effects::default());
        assert!(contains(&app.view(&context("bar")), "bar-audio"));
        assert!(contains(&app.view(&context("audio")), "audio-panel"));
        submitted(&mut app, "audio", None);
        assert!(!contains(&app.view(&context("bar")), "bar-audio"));

        app.update(open(Panel::Network), &mut Effects::default());
        let outgoing = app
            .surfaces()
            .into_iter()
            .find(|s| s.id == "audio")
            .unwrap();
        assert!(outgoing.visible);
        assert_eq!(outgoing.keyboard, Keyboard::None);
        assert_eq!(outgoing.pointer_input, PointerInput::None);
        let retained = app.view(&context("audio"));
        assert!(contains(&retained, "bar-audio"));
        assert!(contains(&retained, "audio-panel"));
        assert!(
            app.event(Event::Key {
                surface: "audio",
                key: lucent_api::Key::Escape
            })
            .is_none()
        );
        assert!(
            app.event(Event::Resize {
                surface: "audio",
                width: 400.,
                height: 300.,
                scale: 1,
                density: lucent_api::Density::ONE
            })
            .is_none()
        );
        assert!(contains(&app.view(&context("network")), "network-panel"));
        submitted(&mut app, "network", None);
        assert!(app.bar_presentation.retained("audio").is_none());
        app.update(
            Message::Network(crate::network::Message::Close),
            &mut Effects::default(),
        );
        assert!(contains(&app.view(&context("bar")), "bar-audio"));
        assert!(contains(&app.view(&context("network")), "bar-audio"));
        submitted(&mut app, "bar", None);
        assert!(app.bar_presentation.retained("network").is_none());
        assert_eq!(serde_json::to_value(&app.settings).unwrap(), preferences);
        assert!(app.surfaces().iter().any(|s| s.id == "bar"
            && s.visible
            && s.exclusive_zone == lucent_design::component::panel::BAR_HEIGHT as i32));
    }

    #[test]
    fn retargeted_panel_stays_passive_on_old_output_until_its_bar_submits() {
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
        app.update(open(Panel::Audio), &mut Effects::default());
        submitted(&mut app, "audio", None);
        let left = app
            .monitors
            .connected
            .values()
            .find(|m| m.target.as_str() == "LEFT")
            .unwrap()
            .instance
            .clone();
        app.update(
            Message::SelectMonitor(lucent_domain::MonitorId::new("RIGHT").unwrap()),
            &mut Effects::default(),
        );
        let pending = app
            .surfaces()
            .into_iter()
            .find(|s| s.id == "audio")
            .unwrap();
        assert_eq!(pending.output.as_ref().unwrap().as_str(), "LEFT");
        assert_eq!(pending.keyboard, Keyboard::None);
        assert_eq!(pending.pointer_input, PointerInput::None);
        assert!(!contains(&app.view(&context("audio")), "audio-panel"));
        submitted(&mut app, "bar", Some(left));
        let incoming = app
            .surfaces()
            .into_iter()
            .find(|s| s.id == "audio")
            .unwrap();
        assert_eq!(incoming.output.as_ref().unwrap().as_str(), "RIGHT");
        assert_eq!(incoming.keyboard, Keyboard::Exclusive);
        assert_eq!(incoming.pointer_input, PointerInput::All);
        assert!(contains(&app.view(&context("audio")), "audio-panel"));
    }

    #[test]
    fn closing_paint_continues_unchanged_across_bar_acknowledgement() {
        fn panel(tree: &Element<Message>) -> Option<&Element<Message>> {
            if tree.id == "audio-panel" {
                Some(tree)
            } else {
                tree.children.iter().find_map(panel)
            }
        }
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.update(open(Panel::Audio), &mut Effects::default());
        submitted_at(&mut app, "audio", None, 2.);
        app.update(
            Message::Audio(crate::audio::Message::Close),
            &mut Effects {
                now: 3.,
                ..Default::default()
            },
        );
        let mut cx = context("audio");
        cx.now = 3.05;
        let before = app.view(&cx);
        let before = panel(&before).unwrap();
        assert!(before.style.opacity > 0. && before.style.opacity < 1.);
        let expected = (before.style.opacity, before.style.position);
        submitted_at(&mut app, "bar", None, 3.05);
        let after = app.view(&cx);
        let after = panel(&after).unwrap();
        assert_eq!((after.style.opacity, after.style.position), expected);
        assert_eq!(
            app.surfaces()
                .iter()
                .find(|s| s.id == "audio")
                .unwrap()
                .pointer_input,
            PointerInput::None
        );
    }

    #[test]
    fn compacted_launcher_keeps_fullscreen_passive_carrier_until_bar_submits() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.viewport = (1920., 1080.);
        app.update(open(Panel::Launcher), &mut Effects::default());
        submitted_at(&mut app, "dock", None, 1.);
        app.update(
            Message::Launcher(crate::launcher::Message::Close),
            &mut Effects {
                now: 2.,
                ..Default::default()
            },
        );
        let compacted = submitted_at(&mut app, "dock", None, 10.);
        assert!(
            compacted
                .recreate_surfaces
                .contains(&lucent_api::SurfaceAddress::new("dock")),
            "child recreation retains the last good fullscreen carrier through the SDK"
        );
        assert!(
            !app.launcher.expanded(),
            "child's real closing-frame bookkeeping was delivered"
        );
        let retained = app.surfaces().into_iter().find(|s| s.id == "dock").unwrap();
        assert_eq!(retained.anchor, lucent_api::Anchor::Fill);
        assert_eq!((retained.width, retained.height), (0, 0));
        assert_eq!(retained.pointer_input, PointerInput::None);
        assert_eq!(retained.keyboard, Keyboard::None);
        assert!(contains(&app.view(&context("dock")), "bar-audio"));
        let released = submitted_at(&mut app, "bar", None, 11.);
        assert!(
            released
                .recreate_surfaces
                .contains(&lucent_api::SurfaceAddress::new("dock")),
            "release must renew the actual compact origin"
        );
        let compact = app.surfaces().into_iter().find(|s| s.id == "dock").unwrap();
        assert_eq!(compact.anchor, lucent_api::Anchor::Bottom);
        assert_eq!(
            compact.height,
            crate::shell_layout::dock_surface_height(1080.)
        );
    }

    #[test]
    fn scoped_background_submission_keeps_its_explicit_native_instance() {
        use lucent_api::{DisplayOutput, OutputName};
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.update(
            Message::Outputs(vec![DisplayOutput {
                global: 1,
                name: Some(OutputName::new("LEFT").unwrap()),
                logical_size: Some((1920, 1080)),
                integer_scale: 1,
            }]),
            &mut Effects::default(),
        );
        let instance = app
            .monitors
            .connected
            .values()
            .next()
            .unwrap()
            .instance
            .clone();
        let message = app
            .event(Event::Scoped {
                instance: instance.clone(),
                event: Box::new(Event::FrameSubmitted {
                    surface: "background",
                    width: 1920.,
                    height: 1080.,
                    now: 1.,
                }),
            })
            .unwrap();
        assert!(
            matches!(message, Message::BackgroundPresented(Some(ref actual), at) if actual == &instance && at == 1.)
        );
        app.update(
            message,
            &mut Effects {
                now: 1.,
                ..Default::default()
            },
        );
        assert!(app.backgrounds.contains_key(&Some(instance)));
    }

    #[test]
    fn same_name_new_output_generation_starts_with_fresh_bar_and_rejects_receipt() {
        use lucent_api::{DisplayOutput, OutputName};
        let catalog = |global| {
            Message::Outputs(vec![DisplayOutput {
                global,
                name: Some(OutputName::new("LEFT").unwrap()),
                logical_size: Some((1920, 1080)),
                integer_scale: 1,
            }])
        };
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.update(catalog(1), &mut Effects::default());
        let old_generation = app.monitors.connected.values().next().unwrap().generation;
        let instance = app
            .monitors
            .connected
            .values()
            .next()
            .unwrap()
            .instance
            .clone();
        app.update(open(Panel::Audio), &mut Effects::default());
        submitted_at(&mut app, "audio", None, 1.);
        app.update(
            open(Panel::Network),
            &mut Effects {
                now: 2.,
                ..Default::default()
            },
        );
        let stale = app
            .bar_presentation
            .receipt(&Event::FrameSubmitted {
                surface: "network",
                width: 1920.,
                height: 1080.,
                now: 3.,
            })
            .unwrap();
        app.update(
            catalog(2),
            &mut Effects {
                now: 4.,
                ..Default::default()
            },
        );
        assert_ne!(
            app.monitors.connected.values().next().unwrap().generation,
            old_generation
        );
        assert!(!app.bar_presentation.submitted(stale));
        assert!(app.bar_presentation.retained("audio").is_none());
        let mut cx = context("bar");
        cx.instance = Some(instance);
        assert!(contains(&app.view(&cx), "bar-audio"));
        assert!(app.network.open());
        submitted_at(&mut app, "network", None, 5.);
        assert!(!contains(&app.view(&cx), "bar-audio"));
    }

    #[test]
    fn stale_bar_receipt_does_not_discard_independent_child_frame_bookkeeping() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.update(open(Panel::Weather), &mut Effects::default());
        submitted_at(&mut app, "weather-controls", None, 1.);
        app.update(
            Message::Weather(crate::weather::Message::Close),
            &mut Effects {
                now: 2.,
                ..Default::default()
            },
        );
        let receipt = app
            .bar_presentation
            .receipt(&Event::FrameSubmitted {
                surface: "bar",
                width: 1920.,
                height: 1080.,
                now: 3.,
            })
            .unwrap();
        app.update(
            open(Panel::Audio),
            &mut Effects {
                now: 4.,
                ..Default::default()
            },
        );
        assert!(app.weather.mounted);
        app.update(
            Message::BarPresented(
                receipt,
                Some(Box::new(Message::Weather(crate::weather::Message::Frame(
                    10.,
                )))),
            ),
            &mut Effects {
                now: 10.,
                ..Default::default()
            },
        );
        assert!(!app.weather.mounted);
        assert!(
            app.bar_presentation.retained("weather-controls").is_some(),
            "child unmount does not remove old pixels before incoming submission"
        );
    }

    #[test]
    #[ignore = "requires Vulkan; run explicitly with --ignored"]
    fn bar_handoff_carriers_keep_identical_pixels() {
        use lucent_ui::{Interaction, Layout};
        use std::sync::Arc;
        let app = Desktop::new(crate::test_adapters::adapters());
        let fonts = Arc::new(
            app.fonts()
                .into_iter()
                .map(|bytes| lucent_ui::text::Font::from_bytes(bytes, Default::default()).unwrap())
                .collect(),
        );
        let layout = Layout::new(Arc::clone(&fonts));
        let mut canvas = lucent_render::Canvas::new(lucent_render::Gpu::headless(fonts).unwrap());
        let mut failures = Vec::new();
        for light in [false, true] {
            for width in [390_u32, 960] {
                let mut app = Desktop::new(crate::test_adapters::adapters());
                app.settings.light = light;
                let mut bar_cx = context("bar");
                bar_cx.width = width as f32;
                bar_cx.height = lucent_design::component::panel::BAR_HEIGHT
                    + lucent_design::component::tooltip::SURFACE_ROOM;
                let baseline = layout.build(
                    &app.view(&bar_cx),
                    bar_cx.width,
                    bar_cx.height,
                    &Interaction::default(),
                    1.,
                );
                app.update(open(Panel::Audio), &mut Effects::default());
                let mut panel_cx = context("audio");
                panel_cx.width = width as f32;
                panel_cx.height = 580.;
                let incoming = app
                    .theme()
                    .apply(app.compose_panel_bar(&panel_cx, Element::empty()));
                let incoming = layout.build(
                    &incoming,
                    panel_cx.width,
                    panel_cx.height,
                    &Interaction::default(),
                    1.,
                );
                submitted(&mut app, "audio", None);
                app.update(open(Panel::Network), &mut Effects::default());
                let retained = layout.build(
                    &app.view(&panel_cx),
                    panel_cx.width,
                    panel_cx.height,
                    &Interaction::default(),
                    1.,
                );
                for scale in [1, 2] {
                    let native_expected = canvas
                        .snapshot(width, bar_cx.height as u32, scale, &baseline.paint)
                        .unwrap();
                    let same_extent_expected = canvas
                        .snapshot(width, panel_cx.height as u32, scale, &baseline.paint)
                        .unwrap();
                    let bar_bytes = width as usize
                        * scale as usize
                        * lucent_design::component::panel::BAR_HEIGHT as usize
                        * scale as usize
                        * 4;
                    for (carrier, scene) in [("incoming", &incoming), ("retained", &retained)] {
                        let actual = canvas
                            .snapshot(width, panel_cx.height as u32, scale, &scene.paint)
                            .unwrap();
                        // The same viewport must preserve the exact recipe.
                        // Different native extents use the existing compact-dock
                        // contract: at most one 8-bit projection quantization step.
                        for (comparison, expected, allowed) in [
                            ("same-extent", &same_extent_expected, 0),
                            ("native-extent", &native_expected, 1),
                        ] {
                            let expected = &expected[..bar_bytes];
                            let actual = &actual[..bar_bytes];
                            let max_error = expected
                                .iter()
                                .zip(actual)
                                .map(|(a, b)| a.abs_diff(*b))
                                .max()
                                .unwrap();
                            let changed_channels =
                                expected.iter().zip(actual).filter(|(a, b)| a != b).count();
                            let mut changed_pixels = 0;
                            let mut bounds = None::<(usize, usize, usize, usize)>;
                            for (index, (a, b)) in expected
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .zip(actual.as_chunks::<4>().0)
                                .enumerate()
                            {
                                if a != b {
                                    changed_pixels += 1;
                                    let (x, y) = (
                                        index % (width * scale) as usize,
                                        index / (width * scale) as usize,
                                    );
                                    bounds = Some(bounds.map_or((x, y, x + 1, y + 1), |b| {
                                        (b.0.min(x), b.1.min(y), b.2.max(x + 1), b.3.max(y + 1))
                                    }));
                                }
                            }
                            let label =
                                format!("{comparison}-{carrier}-light{light}-{width}x{scale}");
                            eprintln!(
                                "bar pixels {label}: max_error={max_error} changed_channels={changed_channels} changed_pixels={changed_pixels} bounds_xyxy={bounds:?}"
                            );
                            if max_error > allowed {
                                crate::visual_tests::save_bar_handoff_comparison(
                                    &label,
                                    width * scale,
                                    lucent_design::component::panel::BAR_HEIGHT as u32 * scale,
                                    expected,
                                    actual,
                                );
                                failures.push(format!("{label}: max_error={max_error}"));
                            }
                        }
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "bar pixel comparisons failed: {failures:?}"
        );
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
            submitted(&mut app, panel.surface(), None);
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
        submitted(&mut app, "bar", None);
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
        submitted(&mut app, "dock", None);
        for (id, monitor) in &app.monitors.connected {
            let mut cx = context("bar");
            cx.instance = Some(monitor.instance.clone());
            assert_eq!(contains(&app.view(&cx), "bar-audio"), *id != selected);
        }
        let selected = lucent_domain::MonitorId::new("RIGHT").unwrap();
        let mut effects = Effects::default();
        app.update(Message::SelectMonitor(selected.clone()), &mut effects);
        assert!(effects.redraw.contains("bar"));
        let old_instance = app
            .monitors
            .connected
            .iter()
            .find(|(id, _)| **id != selected)
            .unwrap()
            .1
            .instance
            .clone();
        submitted(&mut app, "bar", Some(old_instance));
        submitted(&mut app, "dock", None);
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

    fn notification_link_note() -> lucent_domain::Notification {
        lucent_domain::Notification {
            id: 1,
            revision: 1,
            received_unix_ms: 1,
            desktop_entry: None,
            app: "URI Fixture".into(),
            summary: "Owned link".into(),
            body: "<a href='https://example.test/owned'>Open</a>".into(),
            image: None,
            actions: vec![],
            critical: false,
            resident: false,
            transient: false,
            timeout_ms: 0,
        }
    }

    fn begin_notification_link(app: &mut Desktop, effects: &mut Effects<Message>) {
        let note = notification_link_note();
        let target = note.target().unwrap();
        app.update(
            Message::Notifications(crate::notifications::Message::Snapshot(Ok(
                lucent_domain::NotificationSnapshot {
                    history: vec![note],
                    ..Default::default()
                },
            ))),
            effects,
        );
        app.update(open(Panel::Notifications), effects);
        app.update(
            Message::Notifications(crate::notifications::Message::Group(
                "name:URI Fixture".into(),
            )),
            effects,
        );
        app.update(
            Message::Notifications(crate::notifications::Message::OpenLink(
                target,
                "https://example.test/owned".into(),
            )),
            effects,
        );
    }

    fn notification_link_completion(app: &mut Desktop, effects: &mut Effects<Message>) -> Message {
        begin_notification_link(app, effects);
        let prepared = effects
            .activation
            .pop()
            .unwrap()
            .finish(lucent_api::ActivationOutcome::NoUserInput);
        app.update(prepared, effects);
        let released = effects
            .input_release
            .pop()
            .unwrap()
            .finish(lucent_api::InputReleaseOutcome::Released)
            .unwrap();
        app.update(released, effects);
        let index = effects
            .tasks
            .iter()
            .rposition(|task| task.queue == crate::work::CONTROLS)
            .unwrap();
        let done = effects.tasks.remove(index).run();
        assert!(matches!(
            &done,
            Message::Notifications(crate::notifications::Message::LinkDone(_, _))
        ));
        done
    }

    #[test]
    fn pending_link_keeps_history_paint_but_releases_input_before_any_adapter_task() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        let mut effects = Effects::default();
        begin_notification_link(&mut app, &mut effects);
        let surface = app
            .surfaces()
            .into_iter()
            .find(|surface| surface.id == "notifications")
            .unwrap();
        assert!(surface.visible && app.notifications.history_open());
        assert_eq!(surface.keyboard, Keyboard::None);
        assert_eq!(surface.pointer_input, PointerInput::None);
        assert!(contains(
            &app.view(&context("notifications")),
            "notification-history-close"
        ));
        assert_eq!(effects.input_release.len(), 1);
        let prepared = effects
            .activation
            .pop()
            .unwrap()
            .finish(lucent_api::ActivationOutcome::NoUserInput);
        app.update(prepared, &mut effects);
        assert!(app.notifications.yielding_input());
        let release = effects.input_release.pop().unwrap();
        app.update(open(Panel::Audio), &mut effects);
        assert!(release.cancellation.cancelled());
        assert!(
            release
                .finish(lucent_api::InputReleaseOutcome::Released)
                .is_none()
        );
        assert!(app.audio.open());
        assert!(!app.notifications.yielding_input());
        assert_eq!(app.panel_surface(), Some("audio"));
    }

    #[test]
    fn toast_link_does_not_release_an_existing_unrelated_foreground_owner() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        let mut effects = Effects::default();
        app.update(
            Message::Notifications(crate::notifications::Message::Snapshot(Ok(
                lucent_domain::NotificationSnapshot {
                    active: vec![notification_link_note()],
                    ..Default::default()
                },
            ))),
            &mut effects,
        );
        app.update(open(Panel::Audio), &mut effects);
        let activation_count = effects.activation.len();
        let release_count = effects.input_release.len();
        effects.redraw.clear();
        app.update(
            Message::Notifications(crate::notifications::Message::OpenLink(
                lucent_domain::NotificationTarget::new(1, 1).unwrap(),
                "https://example.test/owned".into(),
            )),
            &mut effects,
        );
        assert!(app.audio.open());
        assert_eq!(app.panel_surface(), Some("audio"));
        assert!(!app.notifications.yielding_input());
        assert_eq!(effects.activation.len(), activation_count);
        assert_eq!(effects.input_release.len(), release_count);
        assert!(effects.redraw.contains("notifications"));
        assert!(effects.redraw.contains("bar"));
    }

    #[test]
    fn toast_pending_handoff_retires_when_another_panel_takes_foreground() {
        for context_first in [false, true] {
            let mut app = Desktop::new(crate::test_adapters::adapters());
            let mut effects = Effects::default();
            let note = notification_link_note();
            let target = note.target().unwrap();
            app.update(
                Message::Notifications(crate::notifications::Message::Snapshot(Ok(
                    lucent_domain::NotificationSnapshot {
                        active: vec![note],
                        ..Default::default()
                    },
                ))),
                &mut effects,
            );
            app.update(
                Message::Notifications(crate::notifications::Message::OpenLink(
                    target,
                    "https://example.test/owned".into(),
                )),
                &mut effects,
            );
            assert!(!app.notifications.history_open());
            assert!(app.notifications.yielding_input());
            let token = effects
                .activation
                .pop()
                .unwrap()
                .finish(lucent_api::ActivationOutcome::NoUserInput);
            let request = match &token {
                Message::Notifications(crate::notifications::Message::LinkPrepared(request, _)) => {
                    *request
                }
                _ => panic!("notification activation completion required"),
            };
            let release = effects.input_release.pop().unwrap();
            if context_first {
                app.update(token.clone(), &mut effects);
            }
            effects.redraw.clear();
            app.update(open(Panel::Audio), &mut effects);
            let task_count = effects.tasks.len();
            assert!(effects.redraw.contains("notifications"));
            assert!(release.cancellation.cancelled());
            assert!(
                release
                    .finish(lucent_api::InputReleaseOutcome::Released)
                    .is_none()
            );
            // Already-delivered phase messages have the same retirement guard.
            app.update(token, &mut effects);
            app.update(
                Message::Notifications(crate::notifications::Message::LinkInputReleased(
                    request,
                    lucent_api::InputReleaseOutcome::Released,
                )),
                &mut effects,
            );
            assert_eq!(effects.tasks.len(), task_count);
            assert_eq!(app.panel_surface(), Some("audio"));
            assert!(app.audio.open());
            assert!(!app.notifications.yielding_input());
            let surface = app
                .surfaces()
                .into_iter()
                .find(|surface| surface.id == "notifications")
                .unwrap();
            assert_eq!(surface.keyboard, Keyboard::None);
            assert_eq!(surface.pointer_input, PointerInput::Content);
        }
    }

    #[test]
    fn successful_history_link_releases_input_and_keeps_exit_paint_and_bar_carrier() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        let mut effects = Effects::default();
        let done = notification_link_completion(&mut app, &mut effects);
        submitted_at(&mut app, "notifications", None, 0.5);
        effects.now = 1.;
        app.update(done, &mut effects);
        assert!(!app.notifications.history_open());
        assert!(app.notifications.history_closing());
        assert!(app.bar_presentation.retained("notifications").is_some());
        let specs = app.surfaces();
        let history = specs
            .iter()
            .find(|surface| surface.id == "notifications")
            .unwrap();
        assert!(history.visible);
        assert_eq!(history.keyboard, Keyboard::None);
        assert_eq!(history.pointer_input, PointerInput::None);
        let tree = app.view(&context("notifications"));
        assert!(contains(&tree, "notification-history-close"));
        assert!(contains(&tree, "bar-audio"));
        let bar = specs.iter().find(|surface| surface.id == "bar").unwrap();
        assert!(bar.visible);
        assert_eq!(
            bar.exclusive_zone,
            lucent_design::component::panel::BAR_HEIGHT as i32
        );
        submitted_at(&mut app, "notifications", None, 2.);
        submitted_at(&mut app, "bar", None, 2.);
        assert!(!app.notifications.history_closing());
        assert!(app.bar_presentation.retained("notifications").is_none());
        assert!(
            !app.surfaces()
                .iter()
                .find(|surface| surface.id == "notifications")
                .unwrap()
                .visible
        );
        assert!(contains(&app.view(&context("bar")), "bar-audio"));
    }

    #[test]
    fn passive_toasts_keep_pointer_actions_and_history_retains_keyboard_navigation() {
        use lucent_ui::{Interaction, Layout};
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.viewport = (1920., 1080.);
        let mut effects = Effects::default();
        app.update(
            Message::Notifications(crate::notifications::Message::Snapshot(Ok(
                lucent_domain::NotificationSnapshot {
                    active: vec![notification_link_note()],
                    ..Default::default()
                },
            ))),
            &mut effects,
        );
        let toast = app
            .surfaces()
            .into_iter()
            .find(|surface| surface.id == "notifications")
            .unwrap();
        assert!(toast.visible);
        assert_eq!(toast.keyboard, Keyboard::None);
        assert_eq!(toast.pointer_input, PointerInput::Content);
        let layout = Layout::new(std::sync::Arc::new(
            app.fonts()
                .into_iter()
                .map(|bytes| lucent_ui::text::Font::from_bytes(bytes, Default::default()).unwrap())
                .collect(),
        ));
        let cx = context("notifications");
        let scene = layout.build(
            &app.view(&cx),
            cx.width,
            cx.height,
            &Interaction::default(),
            cx.now,
        );
        let dismiss = scene
            .hits
            .iter()
            .find(|hit| hit.id == "notification-dismiss-1-1")
            .unwrap();
        let (x, y) = (
            dismiss.rect.x + dismiss.rect.w / 2.,
            dismiss.rect.y + dismiss.rect.h / 2.,
        );
        let mut pointer = Interaction::default();
        pointer.press(&scene, x, y);
        assert!(matches!(
            pointer.release(x, y),
            Some(Message::Notifications(
                crate::notifications::Message::Dismiss(_)
            ))
        ));
        app.update(open(Panel::Notifications), &mut effects);
        let history = app
            .surfaces()
            .into_iter()
            .find(|surface| surface.id == "notifications")
            .unwrap();
        assert_eq!(history.keyboard, Keyboard::Exclusive);
        assert_eq!(history.pointer_input, PointerInput::All);
        effects.now = 1.;
        app.update(
            Message::Notifications(crate::notifications::Message::Hide),
            &mut effects,
        );
        let closing = app
            .surfaces()
            .into_iter()
            .find(|surface| surface.id == "notifications")
            .unwrap();
        assert_eq!(closing.keyboard, Keyboard::None);
        assert_eq!(closing.pointer_input, PointerInput::None);
        submitted_at(&mut app, "notifications", None, 2.);
        let toast = app
            .surfaces()
            .into_iter()
            .find(|surface| surface.id == "notifications")
            .unwrap();
        assert!(toast.visible);
        assert_eq!(toast.keyboard, Keyboard::None);
        assert_eq!(toast.pointer_input, PointerInput::Content);
    }

    #[test]
    fn successful_uri_handoff_never_restores_keyboard_ownership_to_remaining_toasts() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        let mut effects = Effects::default();
        let done = notification_link_completion(&mut app, &mut effects);
        app.update(
            Message::Notifications(crate::notifications::Message::Snapshot(Ok(
                lucent_domain::NotificationSnapshot {
                    active: vec![notification_link_note()],
                    ..Default::default()
                },
            ))),
            &mut effects,
        );
        effects.now = 1.;
        app.update(done, &mut effects);
        let fading = app
            .surfaces()
            .into_iter()
            .find(|surface| surface.id == "notifications")
            .unwrap();
        assert_eq!(fading.keyboard, Keyboard::None);
        submitted_at(&mut app, "notifications", None, 2.);
        assert!(!app.notifications.history_open());
        assert!(!app.notifications.history_closing());
        let toast = app
            .surfaces()
            .into_iter()
            .find(|surface| surface.id == "notifications")
            .unwrap();
        assert!(toast.visible, "the live toast remains after history exit");
        assert_eq!(toast.keyboard, Keyboard::None);
        assert_eq!(toast.pointer_input, PointerInput::Content);
    }

    #[test]
    fn failed_history_link_keeps_current_foreground_keyboard_and_pointer_ownership() {
        struct FailedUri;
        impl lucent_domain::UriPort for FailedUri {
            fn open(
                &self,
                _: &lucent_domain::ExternalUri,
                _: lucent_domain::ApplicationLaunchContext,
            ) -> lucent_domain::Result<()> {
                Err(lucent_domain::DomainError::Failed(
                    "Controlled URI failure".into(),
                ))
            }
        }
        let mut adapters = crate::test_adapters::adapters();
        adapters.uri = std::sync::Arc::new(FailedUri);
        let mut app = Desktop::new(adapters);
        let mut effects = Effects::default();
        let done = notification_link_completion(&mut app, &mut effects);
        effects.now = 1.;
        app.update(done, &mut effects);
        assert!(app.notifications.history_open());
        assert!(!app.notifications.history_closing());
        assert_eq!(app.panel_surface(), Some("notifications"));
        let history = app
            .surfaces()
            .into_iter()
            .find(|surface| surface.id == "notifications")
            .unwrap();
        assert!(history.visible);
        assert_eq!(history.keyboard, Keyboard::Exclusive);
        assert_eq!(history.pointer_input, PointerInput::All);
    }

    #[test]
    fn old_link_completion_cannot_dismiss_another_panel_or_reopened_history() {
        for reopen_history in [false, true] {
            let mut app = Desktop::new(crate::test_adapters::adapters());
            let preferences = serde_json::to_value(&app.settings).unwrap();
            let mut effects = Effects::default();
            let done = notification_link_completion(&mut app, &mut effects);
            effects.now = 0.5;
            app.update(open(Panel::Audio), &mut effects);
            if reopen_history {
                app.update(open(Panel::Notifications), &mut effects);
            }
            effects.now = 1.;
            app.update(done, &mut effects);
            assert_eq!(
                app.panel_surface(),
                Some(if reopen_history {
                    "notifications"
                } else {
                    "audio"
                })
            );
            assert_eq!(app.notifications.history_open(), reopen_history);
            assert_eq!(app.audio.open(), !reopen_history);
            let expected = if reopen_history {
                "notifications"
            } else {
                "audio"
            };
            let foreground = app
                .surfaces()
                .into_iter()
                .find(|surface| surface.id == expected)
                .unwrap();
            assert_eq!(foreground.keyboard, Keyboard::Exclusive);
            assert_eq!(foreground.pointer_input, PointerInput::All);
            assert_eq!(serde_json::to_value(&app.settings).unwrap(), preferences);
        }
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
