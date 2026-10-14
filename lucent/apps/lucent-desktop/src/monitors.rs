//! Desktop monitor hosting. Protocol observations map into domain identities here;
//! no monitor-specific policy lives in the framework or renderer.
use crate::{ports::BackgroundTarget, widgets};
use lucent_api::{DisplayOutput, OutputName, SurfaceInstance};
use lucent_domain::MonitorId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub id: MonitorId,
    pub generation: u64,
}
pub struct Monitor {
    pub target: OutputName,
    pub instance: SurfaceInstance,
    pub generation: u64,
    global: u32,
    reported_size: Option<(u32, u32)>,
    pub viewport: (f32, f32),
    pub scale: u32,
    pub density: lucent_api::Density,
    pub widgets: widgets::Host,
}
impl Monitor {
    pub fn route(&self, id: &MonitorId) -> Route {
        Route {
            id: id.clone(),
            generation: self.generation,
        }
    }
}
#[derive(Default)]
pub struct Monitors {
    pub connected: BTreeMap<MonitorId, Monitor>,
    pub active: Option<MonitorId>,
    observed: bool,
    fallback: bool,
    next: u64,
}
impl Monitors {
    pub fn legacy(&self) -> bool {
        !self.observed || self.fallback
    }
    pub fn accepts(&self, route: &Route) -> bool {
        self.connected
            .get(&route.id)
            .is_some_and(|monitor| monitor.generation == route.generation)
    }
    pub fn route(&self, instance: &SurfaceInstance) -> Option<Route> {
        self.connected
            .iter()
            .find(|(_, monitor)| &monitor.instance == instance)
            .map(|(id, monitor)| monitor.route(id))
    }
    pub fn current(&self) -> Option<(&MonitorId, &Monitor)> {
        let id = self.active.as_ref()?;
        self.connected.get(id).map(|monitor| (id, monitor))
    }
    pub fn reconcile(&mut self, catalog: Vec<DisplayOutput>, fallback_viewport: (f32, f32)) {
        self.observed = true;
        let mut named = BTreeMap::new();
        let mut ambiguous = BTreeSet::new();
        let unnamed = catalog.iter().any(|output| output.name.is_none());
        for output in catalog {
            let Some(name) = output.name.as_ref() else {
                continue;
            };
            let Ok(id) = MonitorId::new(name.as_str()) else {
                continue;
            };
            if named.insert(id.clone(), output).is_some() {
                ambiguous.insert(id);
            }
        }
        named.retain(|id, _| !ambiguous.contains(id));
        self.fallback = named.is_empty() && ambiguous.is_empty() && unnamed;
        self.connected.retain(|id, monitor| {
            named
                .get(id)
                .is_some_and(|output| output.global == monitor.global)
        });
        for (id, output) in named {
            let target = output.name.expect("named catalog member");
            let viewport = output
                .logical_size
                .map(|(w, h)| (w as f32, h as f32))
                .unwrap_or(fallback_viewport);
            if let Some(monitor) = self.connected.get_mut(&id) {
                // Actual background configure is authoritative for dimensions.
                // A catalog scale change cancels old gesture geometry immediately.
                if monitor.scale != output.integer_scale
                    || monitor.reported_size != output.logical_size
                {
                    monitor.widgets.cancel_interactions();
                    monitor.scale = output.integer_scale;
                    monitor.reported_size = output.logical_size;
                }
            } else {
                let Some(next) = self.next.checked_add(1) else {
                    continue;
                };
                self.next = next;
                self.connected.insert(
                    id,
                    Monitor {
                        instance: SurfaceInstance::new(target.as_str()).expect("validated name"),
                        target,
                        generation: next,
                        global: output.global,
                        reported_size: output.logical_size,
                        viewport,
                        scale: output.integer_scale,
                        density: lucent_api::Density::integer(output.integer_scale)
                            .unwrap_or_default(),
                        widgets: widgets::Host::default(),
                    },
                );
            }
        }
        if !self
            .active
            .as_ref()
            .is_some_and(|id| self.connected.contains_key(id))
        {
            self.active = self.connected.keys().next().cloned();
        }
    }
    pub fn cancel_interactions(&mut self) {
        for monitor in self.connected.values_mut() {
            monitor.widgets.cancel_interactions();
        }
    }
    pub fn background_target(&self) -> lucent_domain::Result<Option<BackgroundTarget>> {
        let mut extent = (0, 0);
        for monitor in self.connected.values() {
            let target = BackgroundTarget::from_density(
                monitor.viewport.0,
                monitor.viewport.1,
                monitor.density,
            )?
            .size();
            extent.0 = extent.0.max(target.0);
            extent.1 = extent.1.max(target.1);
        }
        if extent == (0, 0) {
            Ok(None)
        } else {
            BackgroundTarget::new(extent.0, extent.1).map(Some)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::{Desktop, Message};
    use lucent_api::{
        Application, ClipboardOutcome, Component, DragEvent, Effects, Event, TextEdit,
    };
    use lucent_domain::{Placement, WidgetLayout};
    #[test]
    fn configured_fractional_density_survives_unchanged_catalog_observation() {
        let mut app = desktop();
        let density = lucent_api::Density::new(150, 120).unwrap();
        send(
            &mut app,
            "DP-2",
            Message::Resize("background", 1280., 720., density),
        );
        app.update(Message::Outputs(catalog()), &mut Effects::default());
        assert_eq!(app.monitors.connected[&id("DP-2")].density, density);
        assert_eq!(
            app.monitors.background_target().unwrap().unwrap().size(),
            (1920, 1080)
        );
        assert!(app.settings.monitor_layouts.is_empty());
    }
    fn output(name: &str, global: u32, size: (u32, u32), scale: u32) -> DisplayOutput {
        DisplayOutput {
            global,
            name: Some(OutputName::new(name).unwrap()),
            logical_size: Some(size),
            integer_scale: scale,
        }
    }
    fn catalog() -> Vec<DisplayOutput> {
        vec![
            output("DP-1", 1, (1920, 1080), 1),
            output("DP-2", 2, (1280, 720), 2),
        ]
    }
    fn id(name: &str) -> MonitorId {
        MonitorId::new(name).unwrap()
    }
    fn desktop() -> Desktop {
        let mut app = Desktop::new(crate::test_adapters::adapters());
        app.update(Message::Outputs(catalog()), &mut Effects::default());
        app
    }
    fn send(app: &mut Desktop, name: &str, message: Message) -> Effects<Message> {
        let route = app.monitors.connected[&id(name)].route(&id(name));
        let mut effects = Effects::default();
        app.update(Message::OnMonitor(route, Box::new(message)), &mut effects);
        effects
    }
    fn drag(finished: bool) -> Message {
        Message::Widgets(widgets::Message::Move(
            "calendar".into(),
            DragEvent {
                dx: 19.,
                dy: 25.,
                finished,
            },
        ))
    }
    #[test]
    fn catalog_and_clock_do_not_create_profiles_or_duplicate_service_subscriptions() {
        let mut app = desktop();
        assert!(app.settings.monitor_layouts.is_empty());
        app.update(Message::Clock(app.clock.clone()), &mut Effects::default());
        assert!(app.settings.monitor_layouts.is_empty());
        let specs = app.surfaces();
        let keys: BTreeSet<_> = specs.iter().map(|s| s.address()).collect();
        assert_eq!(keys.len(), specs.len());
        for role in ["bar", "background"] {
            let copies: Vec<_> = specs.iter().filter(|spec| spec.id == role).collect();
            assert_eq!(copies.len(), 2);
            assert!(
                copies
                    .iter()
                    .all(|spec| spec.instance.is_some() && spec.output.is_some())
            );
        }
        let dock: Vec<_> = specs.iter().filter(|spec| spec.id == "dock").collect();
        assert_eq!(dock.len(), 1);
        assert_eq!(dock[0].output.as_ref().unwrap().as_str(), "DP-1");
        let subscriptions = app.subscriptions();
        let unique: BTreeSet<_> = subscriptions.iter().map(|s| &s.id).collect();
        assert_eq!(unique.len(), subscriptions.len());
        assert_eq!(
            app.monitors.background_target().unwrap().unwrap().size(),
            (2560, 1440)
        );
    }
    #[test]
    fn only_released_output_layout_changes_and_exact_redraw_does_not_cross_outputs() {
        let mut app = desktop();
        let defaults = app.settings.layout.clone();
        send(&mut app, "DP-2", Message::Widgets(widgets::Message::Press));
        let preview = send(&mut app, "DP-2", drag(false));
        assert_eq!(app.settings.monitor_layouts[&id("DP-2")], defaults);
        assert!(app.monitors.connected[&id("DP-2")].widgets.grid_visible());
        assert!(!app.monitors.connected[&id("DP-1")].widgets.grid_visible());
        assert!(!preview.redraw.contains("widgets"));
        assert!(preview.redraw_instances.iter().all(|address| {
            address
                .instance
                .as_ref()
                .is_some_and(|instance| instance.as_str() == "DP-2")
        }));
        send(&mut app, "DP-2", drag(true));
        assert!(
            app.settings.monitor_layouts[&id("DP-2")]
                .positions
                .contains_key("calendar")
        );
        assert_eq!(app.settings.layout, defaults);
        assert!(!app.settings.monitor_layouts.contains_key(&id("DP-1")));
        let saved = app.settings.monitor_layouts[&id("DP-2")].clone();
        app.update(
            Message::Outputs(vec![output("DP-1", 1, (1920, 1080), 1)]),
            &mut Effects::default(),
        );
        assert_eq!(app.settings.monitor_layouts[&id("DP-2")], saved);
        app.update(Message::Outputs(catalog()), &mut Effects::default());
        assert_eq!(app.settings.monitor_layouts[&id("DP-2")], saved);
    }
    #[test]
    fn recycled_protocol_globals_cannot_revive_old_component_callbacks() {
        let mut app = desktop();
        let old = app.monitors.connected[&id("DP-2")].route(&id("DP-2"));
        app.update(Message::Outputs(vec![]), &mut Effects::default());
        assert!(app.surfaces().is_empty());
        app.update(Message::Outputs(catalog()), &mut Effects::default());
        assert!(!app.monitors.accepts(&old));
        app.update(
            Message::OnMonitor(
                old,
                Box::new(Message::Widgets(widgets::Message::Toggle("clock".into()))),
            ),
            &mut Effects::default(),
        );
        assert!(app.settings.monitor_layouts.is_empty());
    }
    #[test]
    fn configure_reflow_cancels_capture_without_overwriting_saved_positions() {
        let mut app = desktop();
        let mut profile = WidgetLayout::default();
        let position = Placement::new(1200., 600.).unwrap();
        profile.positions.insert("calendar".into(), position);
        app.settings
            .monitor_layouts
            .insert(id("DP-2"), profile.clone());
        send(&mut app, "DP-2", Message::Widgets(widgets::Message::Press));
        send(&mut app, "DP-2", drag(false));
        send(
            &mut app,
            "DP-2",
            Message::Resize(
                "background",
                640.,
                480.,
                lucent_api::Density::integer(2).unwrap(),
            ),
        );
        send(&mut app, "DP-2", drag(true));
        assert_eq!(app.settings.monitor_layouts[&id("DP-2")], profile);
        assert!(!app.monitors.connected[&id("DP-2")].widgets.grid_visible());
        for source in app.monitors.connected[&id("DP-2")]
            .widgets
            .surfaces(&profile, (640., 480.))
        {
            assert!(
                source.rect.x >= 0.
                    && source.rect.y >= 0.
                    && source.rect.x + source.rect.w <= 640.
                    && source.rect.y + source.rect.h <= 480.
            );
        }
    }
    #[test]
    fn notes_clipboard_keeps_instance_and_does_not_switch_monitor_on_completion() {
        let mut app = desktop();
        app.settings.layout.visible_widgets = vec!["notes".into()];
        let mut effects = send(
            &mut app,
            "DP-1",
            Message::Widgets(widgets::Message::EditNotes(TextEdit::Paste)),
        );
        let request = effects.clipboard.pop().unwrap();
        assert_eq!(request.target.instance.as_ref().unwrap().as_str(), "DP-1");
        app.update(Message::SelectMonitor(id("DP-2")), &mut Effects::default());
        app.update(
            request.finish(ClipboardOutcome::Error(
                lucent_api::ClipboardError::Cancelled,
            )),
            &mut Effects::default(),
        );
        assert_eq!(app.monitors.active, Some(id("DP-2")));
        assert!(app.settings.notes.is_empty());
        send(
            &mut app,
            "DP-2",
            Message::Widgets(widgets::Message::EditNotes(TextEdit::Replace(
                "Shared notes".into(),
            ))),
        );
        assert_eq!(app.settings.notes, "Shared notes");
    }
    #[test]
    fn scoped_physical_input_targets_its_current_widget_host() {
        let mut app = desktop();
        send(&mut app, "DP-2", drag(false));
        let monitor = &app.monitors.connected[&id("DP-2")];
        let source = monitor
            .widgets
            .surfaces(app.widget_layout(Some(&id("DP-2"))), monitor.viewport)[0]
            .id;
        let message = app
            .event(Event::Scoped {
                instance: monitor.instance.clone(),
                event: Box::new(Event::PointerButton {
                    surface: source,
                    button: lucent_api::MouseButton::Primary,
                    pressed: true,
                    x: 0.,
                    y: 0.,
                }),
            })
            .unwrap();
        app.update(message, &mut Effects::default());
        assert!(!app.monitors.connected[&id("DP-2")].widgets.grid_visible());
        assert!(app.command("monitor select DP-2").unwrap().is_some());
        assert!(app.command("monitor select absent").is_err());
    }
    #[test]
    fn unnamed_fallback_and_ambiguous_named_catalog_never_duplicate_roles() {
        let mut monitors = Monitors::default();
        monitors.reconcile(
            vec![DisplayOutput {
                global: 1,
                name: None,
                logical_size: None,
                integer_scale: 1,
            }],
            (1920., 1080.),
        );
        assert!(monitors.legacy());
        let mut app = desktop();
        app.update(
            Message::Outputs(vec![
                output("DP-1", 1, (1920, 1080), 1),
                output("DP-1", 2, (1920, 1080), 1),
            ]),
            &mut Effects::default(),
        );
        assert!(!app.monitors.legacy());
        assert!(app.monitors.connected.is_empty());
        assert!(app.surfaces().is_empty());
    }
}
