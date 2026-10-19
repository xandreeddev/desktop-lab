//! Top-bar presentation and explicit controls. The clock capsule is passive.
use crate::{
    actions::Action,
    desktop::{Desktop, Message},
    shell_layout,
};
use lucent_api::*;
use lucent_design::*;

impl Desktop {
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
        let monitor = cx
            .instance
            .as_ref()
            .and_then(|instance| self.monitors.route(instance));
        let buttons = crate::workspaces::buttons_for_monitor(
            &self.compositor,
            monitor.as_ref().map(|route| &route.id),
        );
        let geometry =
            shell_layout::BarLayout::new(cx.width, buttons.len()).with_tray(self.tray.bar_count());
        // Prefer the final active overlay slot, keeping a visible special
        // control reachable while its regular workspace is also active.
        let active = buttons.iter().rposition(|w| w.active).unwrap_or(0);
        let start = (active + 1)
            .saturating_sub(geometry.workspace_count)
            .min(buttons.len().saturating_sub(geometry.workspace_count));
        let bar_icon = |name: &str| {
            self.icon(name, icon::TINY)
                .size(component::bar::CONTROL_SIZE, component::bar::CONTROL_SIZE)
                .padding(space::XS)
        };
        let workspaces = buttons
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
                            .background(self.ink().alpha(if w.occupied {
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
                let control = if let Some(label) = &w.label {
                    self.label(label.clone(), font::CAPTION)
                        .color(if w.active {
                            self.theme().on_primary
                        } else {
                            self.ink().alpha(if w.occupied {
                                opacity::STRONG
                            } else {
                                opacity::QUIET
                            })
                        })
                        .align(Align::Center)
                        .size(
                            component::bar::WORKSPACE_SIZE,
                            component::bar::WORKSPACE_SIZE,
                        )
                        .radius(radius::CONTROL)
                        .background(if w.active {
                            self.accent()
                        } else {
                            Color::TRANSPARENT
                        })
                } else {
                    ring
                };
                control
                    .on_click(Message::Workspace(w.command()))
                    .id(w.element_id())
            })
            .collect();
        let spaces = self
            .pill(workspaces)
            .gap(space::SM)
            .size(geometry.workspaces.w, geometry.workspaces.h)
            .at(geometry.workspaces.x, geometry.workspaces.y);
        let title = self.media.label();
        let media_control = if let Some(command) = self.media.toggle_command() {
            bar_icon(
                if matches!(command, lucent_domain::MediaCommand::Pause(_)) {
                    "pause"
                } else {
                    "play"
                },
            )
            .on_click(Message::Media(crate::media::Message::Request(command)))
            .id("bar-play")
        } else {
            bar_icon("play").opacity(opacity::DISABLED)
        };
        let media = self.pill(vec![
            bar_icon("music")
                .on_click(Message::Media(crate::media::Message::Toggle))
                .id("bar-media"),
            self.label(title, font::SMALL)
                .width(Length::Fixed(component::bar::MEDIA_TEXT_WIDTH))
                .clip()
                .on_click(Message::Media(crate::media::Message::Toggle))
                .id("bar-media-title"),
            media_control,
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
                        weekdays[date.weekday() as usize],
                        date.day(),
                        date.month()
                    ),
                    font::CAPTION,
                )
                .width(Length::Fixed(component::bar::DATE_WIDTH))
                .align(Align::Center),
                time,
                bar_icon(
                    self.weather
                        .report()
                        .map(|report| {
                            crate::weather::condition_symbol(report.condition, report.is_day)
                        })
                        .unwrap_or("cloud"),
                ),
                self.label(self.weather.bar_summary(), font::CAPTION)
                    .width(Length::Fixed(component::bar::CONTROL_SIZE))
                    .align(Align::Center),
            ])
            // Keep this capsule in the native input region without making it
            // a launcher control or handing clicks to the application below.
            .on_click(Message::Ignore)
            .tab_stop(false)
            .hover_feedback(false)
            .id("bar-clock");
        let mut controls = vec![
            bar_icon("notifications")
                .on_click(Message::Notifications(
                    crate::notifications::Message::Toggle,
                ))
                .id("bar-notifications"),
            bar_icon("network")
                .on_click(Message::Network(crate::network::Message::Toggle))
                .id("bar-network"),
            bar_icon("bluetooth")
                .on_click(Message::Bluetooth(crate::bluetooth::Message::Toggle))
                .id("bar-bluetooth"),
            bar_icon("power")
                .on_click(Message::Power(crate::power::Message::Toggle))
                .id("bar-power"),
            self.label(self.audio.label(), font::CAPTION)
                .on_click(Message::Audio(crate::audio::Message::Toggle))
                .id("bar-audio")
                .width(Length::Fixed(component::bar::CONTROL_SIZE))
                .align(Align::Center),
            bar_icon("lock")
                .on_click(Message::Action(Action::Lock))
                .id("bar-lock"),
        ];
        if geometry.show_tray {
            controls.insert(
                0,
                bar_icon("apps")
                    .on_click(Message::Tray(crate::tray::Message::Toggle))
                    .id("bar-tray"),
            );
        }
        if geometry.show_mute {
            controls.insert(
                4,
                bar_icon("volume")
                    .on_click(Message::Action(Action::Mute))
                    .id("bar-mute"),
            );
        }
        controls.splice(
            0..0,
            self.tray
                .bar_icons(geometry.tray_count, self.theme())
                .into_iter()
                .map(|element| element.map(Message::Tray)),
        );
        let right = self
            .pill(controls)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_ui::{Interaction, Layout};
    use std::sync::Arc;

    fn element<'a>(tree: &'a Element<Message>, id: &str) -> Option<&'a Element<Message>> {
        if tree.id == id {
            return Some(tree);
        }
        tree.children.iter().find_map(|child| element(child, id))
    }

    #[test]
    fn root_closes_other_panels_only_when_tray_actually_opens() {
        use crate::tray::{BarAction, Message as T};
        let mut app = Desktop::new(crate::test_adapters::adapters());
        let mut effects = Effects::default();
        app.update(
            Message::Tray(T::Observed(Ok(crate::tray::tests::fixture()))),
            &mut effects,
        );
        app.update(Message::Audio(crate::audio::Message::Open), &mut effects);
        let id = lucent_domain::TrayItemId::new(1).unwrap();
        app.update(Message::Tray(T::Bar(id, BarAction::Activate)), &mut effects);
        assert!(app.audio.open());
        assert!(!app.tray.open());
        // Retire the unexecuted effect; actual adapter calls are covered by the child.
        app.update(Message::Tray(T::Close), &mut effects);
        app.update(Message::Tray(T::Bar(id, BarAction::Menu)), &mut effects);
        assert!(app.tray.open());
        assert!(!app.audio.open());
    }

    #[test]
    fn clock_capsule_absorbs_pointer_without_opening_menu_or_taking_focus() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.update(
            Message::Media(crate::media::Message::Observed(Ok(crate::media::fixture()))),
            &mut Effects::default(),
        );
        app.viewport = (1920., 1080.);
        let cx = ViewContext {
            surface: "bar",
            instance: None,
            width: 1920.,
            height: component::panel::BAR_HEIGHT,
            now: 1.,
            density: Density::ONE,
        };
        let tree = app.view(&cx);
        let capsule = element(&tree, "bar-clock").expect("center capsule is visible");
        assert!(matches!(capsule.click, Some(Message::Ignore)));
        assert!(!capsule.style.tab_stop);
        assert!(!capsule.style.hover_feedback);

        let fonts = Arc::new(
            app.fonts()
                .into_iter()
                .map(|bytes| fontdue::Font::from_bytes(bytes, Default::default()).unwrap())
                .collect(),
        );
        let layout = Layout::new(fonts);
        let mut input = Interaction::default();
        input.focus = Some("bar-network".into());
        let scene = layout.build(&tree, cx.width, cx.height, &input, cx.now);
        let hit = scene.hits.iter().find(|hit| hit.id == "bar-clock").unwrap();
        // Date, time and weather all belong to the same passive barrier.
        for fraction in [0.1, 0.5, 0.9] {
            let x = hit.rect.x + hit.rect.w * fraction;
            let y = hit.rect.y + hit.rect.h / 2.;
            input.press(&scene, x, y);
            let message = input
                .release(x, y)
                .expect("click is absorbed by the capsule");
            assert!(matches!(message, Message::Ignore));
            let mut effects = Effects::default();
            app.update(message, &mut effects);
            assert!(!app.launcher.open());
            assert!(effects.tasks.is_empty());
            assert!(effects.redraw.is_empty());
            assert_eq!(input.focus.as_deref(), Some("bar-network"));
        }

        assert!(matches!(
            element(&tree, "bar-play").unwrap().click,
            Some(Message::Media(crate::media::Message::Request(
                lucent_domain::MediaCommand::Pause(_)
            )))
        ));
        assert!(matches!(
            element(&tree, "bar-network").unwrap().click,
            Some(Message::Network(crate::network::Message::Toggle))
        ));
        assert!(matches!(
            element(&tree, "bar-audio").unwrap().click,
            Some(Message::Audio(crate::audio::Message::Toggle))
        ));
        assert!(matches!(
            element(&tree, "bar-lock").unwrap().click,
            Some(Message::Action(Action::Lock))
        ));
    }

    #[test]
    fn tray_capacity_keeps_existing_modules_and_spacing_grid() {
        for width in [320., 390., 640., 960., 1024., 1280., 1366., 1920., 3840.] {
            for items in [0, 1, 4, 8, 64] {
                let base = shell_layout::BarLayout::new(width, 8);
                let actual = shell_layout::BarLayout::new(width, 8).with_tray(items);
                assert!(actual.tray_count <= items.min(component::bar::MAX_TRAY_ITEMS as usize));
                assert_eq!(actual.workspaces, base.workspaces);
                assert_eq!(actual.clock, base.clock);
                assert_eq!(actual.media, base.media);
                assert_eq!(
                    actual.system.x + actual.system.w,
                    base.system.x + base.system.w
                );
                let mut groups = vec![actual.workspaces, actual.system];
                groups.extend(actual.clock);
                groups.extend(actual.media);
                groups.retain(|rect| rect.w > 0.);
                groups.sort_by(|a, b| a.x.total_cmp(&b.x));
                for rect in &groups {
                    for value in [rect.x, rect.y, rect.w, rect.h] {
                        assert_eq!(value % layout::SHELL_STEP, 0.);
                    }
                    assert!(
                        rect.x >= layout::SHELL_INSET
                            && rect.x + rect.w <= width - layout::SHELL_INSET
                    );
                }
                for pair in groups.windows(2) {
                    assert!(pair[1].x - pair[0].x - pair[0].w >= layout::SECTION_GAP);
                }
            }
        }
    }

    #[test]
    fn visible_tray_icons_use_bounded_token_geometry_and_keep_overflow_reachable() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.update(
            Message::Tray(crate::tray::Message::Observed(Ok(
                crate::tray::tests::fixture(),
            ))),
            &mut Effects::default(),
        );
        let fonts = Arc::new(
            app.fonts()
                .into_iter()
                .map(|bytes| fontdue::Font::from_bytes(bytes, Default::default()).unwrap())
                .collect(),
        );
        let engine = Layout::new(fonts);
        for width in [390., 640., 1024., 1920.] {
            let cx = ViewContext {
                surface: "bar",
                instance: None,
                width,
                height: component::panel::BAR_HEIGHT,
                now: 1.,
                density: Density::ONE,
            };
            let tree = app.view(&cx);
            let scene = engine.build(&tree, width, cx.height, &Interaction::default(), cx.now);
            let geometry = shell_layout::BarLayout::new(
                width,
                crate::workspaces::buttons_for_monitor(&app.compositor, None).len(),
            )
            .with_tray(8);
            let hits: Vec<_> = scene
                .hits
                .iter()
                .filter(|hit| hit.id.starts_with("bar-tray-item-"))
                .collect();
            assert_eq!(hits.len(), geometry.tray_count);
            for (index, hit) in hits.iter().enumerate() {
                assert_eq!(hit.rect.w, component::bar::CONTROL_SIZE);
                assert_eq!(hit.rect.h, component::bar::CONTROL_SIZE);
                assert_eq!(
                    hit.rect.x,
                    geometry.system.x
                        + component::pill::PADDING_INLINE
                        + index as f32 * (component::bar::CONTROL_SIZE + space::SM)
                );
                assert_eq!(hit.rect.x % layout::UNIT, 0.);
                assert_eq!(
                    hit.rect.y,
                    geometry.system.y + component::pill::PADDING_BLOCK
                );
                assert!(
                    hit.middle_click.is_some()
                        && hit.secondary_click.is_some()
                        && hit.wheel.is_some()
                );
            }
            if geometry.show_tray {
                assert!(scene.hits.iter().any(|hit| hit.id == "bar-tray"));
            }
        }
    }
    #[test]
    fn bar_play_captures_the_displayed_player_and_explicit_pause() {
        use lucent_domain::*;
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.viewport = (1920., 1080.);
        let mut snapshot = crate::media::fixture();
        let mut other = snapshot.players[0].clone();
        other.id = MediaPlayerId::new(2).unwrap();
        snapshot.players.push(other);
        app.update(
            Message::Media(crate::media::Message::Observed(Ok(snapshot))),
            &mut Effects::default(),
        );
        let tree = app.view(&ViewContext {
            surface: "bar",
            instance: None,
            width: 1920.,
            height: component::panel::BAR_HEIGHT,
            now: 1.,
            density: Density::ONE,
        });
        let captured = element(&tree, "bar-play").unwrap().click.clone().unwrap();
        assert!(
            matches!(captured, Message::Media(crate::media::Message::Request(MediaCommand::Pause(id))) if id.value() == 1)
        );
        app.update(
            Message::Media(crate::media::Message::Select(
                MediaPlayerId::new(2).unwrap(),
            )),
            &mut Effects::default(),
        );
        assert!(
            matches!(captured, Message::Media(crate::media::Message::Request(MediaCommand::Pause(id))) if id.value() == 1)
        );
        app.update(
            Message::Media(crate::media::Message::Observed(
                Ok(MediaSnapshot::default()),
            )),
            &mut Effects::default(),
        );
        let mut effects = Effects::default();
        app.update(captured, &mut effects);
        assert!(effects.tasks.is_empty());
    }
}
