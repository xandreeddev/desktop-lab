//! Output selection policy shared by the native adapter and deterministic tests.
use lucent_api::{DisplayOutput, OutputName};
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Target<T> {
    Compositor,
    Waiting,
    Named(T),
}
pub(super) fn resolve<T: Clone + PartialEq>(
    name: Option<&OutputName>,
    bindings: &[(DisplayOutput, T)],
    closed: Option<&(OutputName, T)>,
) -> Target<T> {
    let Some(name) = name else {
        return Target::Compositor;
    };
    let mut matches = bindings
        .iter()
        .filter(|(info, _)| info.name.as_ref() == Some(name));
    let Some((_, target)) = matches.next() else {
        return Target::Waiting;
    };
    if matches.next().is_some()
        || closed.is_some_and(|(previous, bound)| previous == name && bound == target)
    {
        return Target::Waiting;
    }
    Target::Named(target.clone())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn binding(name: Option<&str>, handle: u32) -> (DisplayOutput, u32) {
        (
            DisplayOutput {
                global: 1,
                name: name.map(|name| OutputName::new(name).unwrap()),
                logical_size: Some((1920, 1080)),
                integer_scale: 1,
            },
            handle,
        )
    }
    #[test]
    fn missing_unnamed_and_ambiguous_targets_never_fall_back_to_another_display() {
        let name = OutputName::new("DP-1").unwrap();
        assert_eq!(resolve::<u32>(None, &[], None), Target::Compositor);
        assert_eq!(
            resolve(
                Some(&name),
                &[binding(None, 1), binding(Some("DP-2"), 2)],
                None
            ),
            Target::Waiting
        );
        assert_eq!(
            resolve(
                Some(&name),
                &[binding(Some("DP-1"), 1), binding(Some("DP-1"), 2)],
                None
            ),
            Target::Waiting
        );
        assert_eq!(
            resolve(Some(&name), &[binding(Some("DP-1"), 1)], None),
            Target::Named(1)
        );
    }
    #[test]
    fn a_closed_target_waits_until_a_new_native_binding_replaces_it() {
        let name = OutputName::new("DP-1").unwrap();
        let closed = (name.clone(), 7);
        assert_eq!(
            resolve(Some(&name), &[binding(Some("DP-1"), 7)], Some(&closed)),
            Target::Waiting
        );
        assert_eq!(resolve(Some(&name), &[], Some(&closed)), Target::Waiting);
        // The protocol global may be reused; current object identity matters.
        assert_eq!(
            resolve(Some(&name), &[binding(Some("DP-1"), 8)], Some(&closed)),
            Target::Named(8)
        );
        assert_eq!(
            resolve(None, &[binding(Some("DP-1"), 7)], Some(&closed)),
            Target::Compositor
        );
    }
}
