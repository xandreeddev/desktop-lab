//! Logical roles and concrete instances are independent of native object handles.
use std::sync::Arc;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SurfaceInstance(Arc<str>);
impl SurfaceInstance {
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err("A surface instance must be bounded, nonempty and contain no controls");
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// Typed keys avoid delimiter collisions and preserve existing uninstanced roles.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SurfaceAddress {
    pub role: &'static str,
    pub instance: Option<SurfaceInstance>,
}
impl SurfaceAddress {
    pub fn new(role: &'static str) -> Self {
        Self {
            role,
            instance: None,
        }
    }
    pub fn instance(mut self, instance: SurfaceInstance) -> Self {
        self.instance = Some(instance);
        self
    }
}
impl std::fmt::Display for SurfaceAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.instance {
            Some(instance) => write!(f, "{} [{}]", self.role, instance.as_str()),
            None => f.write_str(self.role),
        }
    }
}
impl crate::SurfaceSpec {
    pub fn scope_event(&self, event: crate::Event) -> crate::Event {
        match &self.instance {
            Some(instance) => crate::Event::Scoped {
                instance: instance.clone(),
                event: Box::new(event),
            },
            None => event,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    fn spec(instance: Option<SurfaceInstance>) -> SurfaceSpec {
        SurfaceSpec {
            id: "bar",
            instance,
            output: None,
            layer: Layer::Top,
            anchor: Anchor::Top,
            margins: Default::default(),
            redraw_group: None,
            width: 0,
            height: 64,
            exclusive_zone: 0,
            keyboard: Keyboard::OnDemand,
            visible: true,
            pointer_input: PointerInput::Content,
        }
    }
    #[test]
    fn dynamic_instances_are_validated_and_typed_addresses_cannot_collide() {
        for value in ["", "bad\0id", "bad\nid"] {
            assert!(SurfaceInstance::new(value).is_err());
        }
        assert!(SurfaceInstance::new("x".repeat(257)).is_err());
        let instance = SurfaceInstance::new("DP-1").unwrap();
        let concrete = SurfaceAddress::new("bar").instance(instance);
        assert_ne!(concrete, SurfaceAddress::new("bar [DP-1]"));
        assert_ne!(concrete, SurfaceAddress::new("bar"));
        let mut keys = std::collections::BTreeSet::new();
        keys.insert(concrete);
        keys.insert(SurfaceAddress::new("bar [DP-1]"));
        assert_eq!(keys.len(), 2);
    }
    #[test]
    fn native_events_preserve_role_and_scope_without_changing_legacy_events() {
        let event = Event::Resize {
            surface: "bar",
            width: 1920.,
            height: 64.,
            scale: 2,
            density: Density::ONE,
        };
        assert!(matches!(
            spec(None).scope_event(event.clone()),
            Event::Resize { scale: 2, .. }
        ));
        let instance = SurfaceInstance::new("DP-2").unwrap();
        assert!(
            matches!(spec(Some(instance.clone())).scope_event(event),Event::Scoped{instance:actual,event} if actual==instance&&matches!(*event,Event::Resize{surface:"bar",scale:2,..}))
        );
    }
    #[test]
    fn child_delegation_keeps_targeted_redraw_and_clipboard_instance_identity() {
        let instance = SurfaceInstance::new("DP-2").unwrap();
        let address = SurfaceAddress::new("bar").instance(instance.clone());
        let mut parent = Effects::<String>::default();
        parent.delegate(
            |child: &mut Effects<&str>| {
                child.redraw_surface(address.clone());
                child.clipboard.push(ClipboardRequest::new(
                    ClipboardTarget::new("bar", "edit").in_instance(instance),
                    ClipboardAction::Read,
                    |_| "done",
                ));
            },
            str::to_owned,
        );
        assert!(parent.redraw_instances.contains(&address));
        assert!(parent.redraw.is_empty());
        let request = parent.clipboard.pop().unwrap();
        assert_eq!(request.target.address(), address);
        assert_eq!(
            request.finish(ClipboardOutcome::Error(ClipboardError::Cancelled)),
            "done"
        );
    }
}
