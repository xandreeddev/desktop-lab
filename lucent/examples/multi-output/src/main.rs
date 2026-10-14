//! One logical bar, independent state/input/clipboard for each named output.
use lucent_api::*;
use lucent_design::{Theme, component, font, radius, space};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone)]
enum Message {
    Outputs(Vec<DisplayOutput>),
    Count(SurfaceInstance),
    Edit(SurfaceInstance, TextEdit),
    Quit,
}
struct Entry {
    count: u32,
    value: String,
    field: TextFieldState,
}
impl Entry {
    fn new(instance: &SurfaceInstance) -> Self {
        let value = format!("Text for {}", instance.as_str());
        let mut field = TextFieldState::default();
        field.reset(&value);
        Self {
            count: 0,
            value,
            field,
        }
    }
}
#[derive(Default)]
struct Example {
    outputs: Vec<DisplayOutput>,
    entries: BTreeMap<SurfaceInstance, Entry>,
}
impl Example {
    fn bindings(&self) -> BTreeMap<SurfaceInstance, OutputName> {
        let mut result = BTreeMap::new();
        let mut ambiguous = BTreeSet::new();
        for name in self
            .outputs
            .iter()
            .filter_map(|output| output.name.as_ref())
        {
            let Ok(instance) = SurfaceInstance::new(name.as_str()) else {
                continue;
            };
            if result.insert(instance.clone(), name.clone()).is_some() {
                ambiguous.insert(instance);
            }
        }
        result.retain(|instance, _| !ambiguous.contains(instance));
        result
    }
    fn live(&self, instance: &SurfaceInstance) -> bool {
        self.bindings().contains_key(instance)
    }

    fn address(instance: SurfaceInstance) -> SurfaceAddress {
        SurfaceAddress::new("bar").instance(instance)
    }
}
impl Component for Example {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        let Some(instance) = cx.instance.as_ref() else {
            return Element::empty();
        };
        let Some(entry) = self.entries.get(instance) else {
            return Element::empty();
        };
        let theme = Theme::new(false);
        let selected = instance.clone();
        let edit_instance = instance.clone();
        theme.apply(
            Element::column(vec![
                Element::row(vec![
                    Element::text(format!("{} · {} clicks", instance.as_str(), entry.count))
                        .font(font::TITLE)
                        .color(theme.on_surface)
                        .width(Length::Fill),
                    theme.button("Count", Message::Count(selected)).id("count"),
                    theme.button("Close all", Message::Quit).id("close"),
                ])
                .width(Length::Fill)
                .gap(space::SM)
                .align(Align::Center),
                Element::text_input(
                    &entry.value,
                    entry.field.selection(),
                    "Independent text",
                    move |edit| Message::Edit(edit_instance.clone(), edit),
                )
                .id("edit")
                .width(Length::Fill)
                .height(Length::Fixed(component::input::HEIGHT))
                .padding_xy(
                    component::input::PADDING_INLINE,
                    component::input::PADDING_BLOCK,
                )
                .font(font::BODY)
                .color(theme.on_surface)
                .background(theme.surface_container)
                .radius(radius::CONTROL),
            ])
            .padding(space::LG)
            .gap(space::SM)
            .fill()
            .background(theme.surface)
            .radius(radius::PANEL),
        )
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Outputs(outputs) => {
                self.outputs = outputs;
                // This example owns only connected state. Persistent desktop
                // layouts are a separate client/domain policy.
                let live: BTreeSet<_> = self.bindings().into_keys().collect();
                self.entries.retain(|instance, _| live.contains(instance));
                for instance in live {
                    self.entries
                        .entry(instance.clone())
                        .or_insert_with(|| Entry::new(&instance));
                }
                effects.redraw("bar");
            }
            Message::Count(instance) if self.live(&instance) => {
                if let Some(entry) = self.entries.get_mut(&instance) {
                    entry.count = entry.count.saturating_add(1);
                    effects.redraw_surface(Self::address(instance));
                }
            }
            Message::Edit(instance, edit) if self.live(&instance) => {
                if let Some(entry) = self.entries.get_mut(&instance) {
                    let target = ClipboardTarget::new("bar", "edit").in_instance(instance.clone());
                    let message_instance = instance.clone();
                    let _ = entry.field.update(
                        &mut entry.value,
                        edit,
                        4096,
                        target,
                        effects,
                        move |edit| Message::Edit(message_instance.clone(), edit),
                    );
                    effects.redraw_surface(Self::address(instance));
                }
            }
            Message::Count(_) | Message::Edit(_, _) => {}
            Message::Quit => effects.quit(),
        }
    }
}
impl Application for Example {
    fn name(&self) -> &'static str {
        "lucent-multi-output"
    }
    fn clipboard_enabled(&self) -> bool {
        true
    }
    fn fonts(&self) -> Vec<&'static [u8]> {
        vec![include_bytes!(
            "../../../apps/lucent-desktop/assets/LucentSans.ttf"
        )]
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        self.bindings()
            .into_iter()
            .map(|(instance, name)| SurfaceSpec {
                id: "bar",
                instance: Some(instance),
                output: Some(name),
                layer: Layer::Top,
                anchor: Anchor::Top,
                margins: Default::default(),
                redraw_group: None,
                width: 0,
                height: (component::input::HEIGHT * 3.) as u32,
                exclusive_zone: 0,
                keyboard: Keyboard::OnDemand,
                visible: true,
                pointer_input: PointerInput::Content,
            })
            .collect()
    }
    fn event(&self, event: Event) -> Option<Message> {
        if let Event::OutputsChanged(outputs) = event {
            Some(Message::Outputs(outputs))
        } else {
            None
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--help") {
        println!(
            "multi-output: one native bar and independent text/clipboard per named output; unnamed outputs are omitted"
        );
        return Ok(());
    }
    lucent_wayland::run(Example::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn output(name: &str, global: u32) -> DisplayOutput {
        DisplayOutput {
            global,
            name: Some(OutputName::new(name).unwrap()),
            logical_size: Some((1920, 1080)),
            integer_scale: 1,
        }
    }
    #[test]
    fn independent_roles_share_no_counter_edit_or_redraw_state() {
        let mut app = Example::default();
        let mut effects = Effects::default();
        app.update(
            Message::Outputs(vec![output("DP-1", 1), output("DP-2", 2)]),
            &mut effects,
        );
        let specs = app.surfaces();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].id, specs[1].id);
        assert_ne!(specs[0].address(), specs[1].address());
        effects.redraw.clear();
        let first = SurfaceInstance::new("DP-1").unwrap();
        let second = SurfaceInstance::new("DP-2").unwrap();
        app.update(Message::Count(first.clone()), &mut effects);
        assert_eq!(app.entries[&first].count, 1);
        assert_eq!(app.entries[&second].count, 0);
        assert_eq!(
            effects.redraw_instances,
            std::collections::BTreeSet::from([Example::address(first.clone())])
        );
        app.update(
            Message::Edit(first.clone(), TextEdit::Insert("!".into())),
            &mut effects,
        );
        assert!(app.entries[&first].value.ends_with('!'));
        assert!(!app.entries[&second].value.ends_with('!'));
        app.update(Message::Outputs(vec![output("DP-2", 2)]), &mut effects);
        app.update(Message::Count(first.clone()), &mut effects);
        assert!(!app.entries.contains_key(&first));
        assert_eq!(app.entries[&second].count, 0);
    }
    #[test]
    fn duplicate_names_do_not_create_two_ambiguous_instances() {
        let mut app = Example::default();
        app.update(
            Message::Outputs(vec![output("DP-1", 1), output("DP-1", 2)]),
            &mut Effects::default(),
        );
        assert!(app.surfaces().is_empty());
        assert!(app.entries.is_empty());
    }
}
