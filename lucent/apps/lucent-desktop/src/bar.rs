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
        let geometry = shell_layout::BarLayout::new(cx.width, buttons.len());
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
                        weekdays[date.weekday() as usize],
                        date.day(),
                        date.month()
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
    fn clock_capsule_absorbs_pointer_without_opening_menu_or_taking_focus() {
        let mut app = Desktop::new(crate::test_adapters::adapters());
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
            Some(Message::Action(Action::PlayPause))
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
}
