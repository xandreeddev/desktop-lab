//! Validate actions against current, application-owned menu state.
use lucent_domain::*;
pub fn menu_entry(root: &TrayMenuEntry, id: i32) -> Option<&TrayMenuEntry> {
    if root.id == id {
        Some(root)
    } else {
        root.children.iter().find_map(|child| menu_entry(child, id))
    }
}
pub fn validate_menu_action(menu: &TrayMenu, revision: u32, id: i32) -> Result<()> {
    if menu.revision != revision {
        return Err(DomainError::Invalid(
            "Tray menu changed; open it again".into(),
        ));
    }
    let entry = menu_entry(&menu.root, id)
        .ok_or_else(|| DomainError::Invalid("Tray menu entry retired".into()))?;
    fn reachable(node: &TrayMenuEntry, id: i32) -> bool {
        node.visible
            && node.enabled
            && (node.id == id || node.children.iter().any(|child| reachable(child, id)))
    }
    if !reachable(&menu.root, id)
        || id == menu.root.id
        || !entry.visible
        || !entry.enabled
        || entry.separator
        || entry.is_submenu()
    {
        return Err(DomainError::Invalid(
            "Tray menu entry is not an action".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(id: i32) -> TrayMenuEntry {
        TrayMenuEntry {
            id,
            label: String::new(),
            enabled: true,
            visible: true,
            separator: false,
            check: TrayCheck::None,
            submenu: false,
            children: vec![],
        }
    }
    #[test]
    fn menu_actions_require_current_revision_and_visible_enabled_ancestry() {
        let mut root = entry(0);
        let mut parent = entry(1);
        parent.children.push(entry(2));
        root.children.push(parent);
        let mut menu = TrayMenu {
            item: TrayItemId::new(1).unwrap(),
            revision: 3,
            root,
        };
        assert!(validate_menu_action(&menu, 3, 2).is_ok());
        for (revision, id) in [(2, 2), (3, 0), (3, 1), (3, 99)] {
            assert!(validate_menu_action(&menu, revision, id).is_err());
        }
        menu.root.children[0].visible = false;
        assert!(validate_menu_action(&menu, 3, 2).is_err());
        menu.root.children[0].visible = true;
        menu.root.children[0].enabled = false;
        assert!(validate_menu_action(&menu, 3, 2).is_err());
    }
}
