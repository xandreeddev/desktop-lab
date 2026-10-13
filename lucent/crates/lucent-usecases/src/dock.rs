//! Pin identity/order policy. Geometry, default suggestions and gestures are client policy.
use lucent_domain::*;
use std::collections::BTreeSet;
fn unique(pins: &[AppId]) -> Vec<AppId> {
    let mut seen = BTreeSet::new();
    pins.iter()
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect()
}
pub fn effective_pins(settings: &DesktopSettings, defaults: &[AppId]) -> Vec<AppId> {
    unique(
        if settings.pinned.is_empty() && settings.dock.default_pins {
            defaults
        } else {
            &settings.pinned
        },
    )
}
fn commit(settings: &mut DesktopSettings, pins: Vec<AppId>) -> bool {
    let changed = settings.pinned != pins || settings.dock.default_pins;
    settings.pinned = pins;
    settings.dock.default_pins = false;
    changed
}
pub fn set_pin(
    settings: &mut DesktopSettings,
    id: &AppId,
    pinned: bool,
    current: &[AppId],
    apps: &[Application],
) -> Result<bool> {
    if pinned && !apps.iter().any(|app| &app.id == id) {
        return Err(DomainError::Invalid(
            "Application is no longer installed".into(),
        ));
    }
    let mut pins = unique(current);
    pins.retain(|pin| pin != id);
    if pinned {
        let position = current
            .iter()
            .position(|pin| pin == id)
            .unwrap_or(pins.len())
            .min(pins.len());
        pins.insert(position, id.clone());
    }
    Ok(commit(settings, pins))
}
/// Move a pin before another current pin; None places it last. Missing saved apps stay saved.
pub fn move_pin(
    settings: &mut DesktopSettings,
    id: &AppId,
    before: Option<&AppId>,
    current: &[AppId],
    apps: &[Application],
) -> Result<bool> {
    if !current.contains(id) && !apps.iter().any(|app| &app.id == id) {
        return Err(DomainError::Invalid(
            "Application is no longer installed".into(),
        ));
    }
    if before.is_some_and(|target| target != id && !current.contains(target)) {
        return Err(DomainError::Invalid("Dock destination changed".into()));
    }
    let mut pins = unique(current);
    if before == Some(id) {
        return Ok(false);
    }
    pins.retain(|pin| pin != id);
    let index = before
        .and_then(|target| pins.iter().position(|pin| pin == target))
        .unwrap_or(pins.len());
    pins.insert(index, id.clone());
    Ok(commit(settings, pins))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn id(name: &str) -> AppId {
        AppId::new(format!("{name}.desktop")).unwrap()
    }
    #[test]
    fn explicit_empty_pins_survive_defaults_and_missing_apps_keep_identity() {
        let mut settings = DesktopSettings::default();
        let defaults = vec![id("one")];
        assert_eq!(effective_pins(&settings, &defaults), defaults);
        assert!(set_pin(&mut settings, &id("one"), false, &defaults, &[]).unwrap());
        assert!(effective_pins(&settings, &defaults).is_empty());
        settings.pinned = vec![id("missing"), id("other")];
        let current = settings.pinned.clone();
        move_pin(
            &mut settings,
            &id("other"),
            Some(&id("missing")),
            &current,
            &[],
        )
        .unwrap();
        assert_eq!(settings.pinned, vec![id("other"), id("missing")]);
    }
    #[test]
    fn retired_targets_and_destinations_do_not_mutate_preferences() {
        let mut settings = DesktopSettings::default();
        let saved = settings.clone();
        assert!(set_pin(&mut settings, &id("missing"), true, &[], &[]).is_err());
        assert_eq!(settings, saved);
        assert!(
            move_pin(
                &mut settings,
                &id("one"),
                Some(&id("gone")),
                &[id("one")],
                &[]
            )
            .is_err()
        );
        assert_eq!(settings, saved);
    }
}
