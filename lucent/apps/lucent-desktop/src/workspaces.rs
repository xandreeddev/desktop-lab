//! Desktop presentation policy, separate from the compositor's reported state.
use lucent_domain::CompositorSnapshot;
use std::collections::BTreeMap;

/// A destination shown in the bar may exist only as a shortcut; it is not a
/// claim that the compositor currently has a workspace with this ID.
pub struct WorkspaceButton {
    pub id: i32,
    pub occupied: bool,
    pub active: bool,
}
pub fn buttons(snapshot: &CompositorSnapshot) -> Vec<WorkspaceButton> {
    let mut buttons: BTreeMap<_, _> = (1..=6)
        .map(|id| {
            (
                id,
                WorkspaceButton {
                    id,
                    occupied: false,
                    active: false,
                },
            )
        })
        .collect();
    for workspace in &snapshot.workspaces {
        // The standard workspace strip does not offer special workspace actions.
        // The adapter still reports those workspaces to other consumers.
        if workspace.id > 0 {
            buttons.insert(
                workspace.id,
                WorkspaceButton {
                    id: workspace.id,
                    occupied: workspace.windows > 0,
                    active: workspace.active,
                },
            );
        }
    }
    buttons.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_domain::Workspace;
    #[test]
    fn shortcuts_do_not_mutate_reported_workspaces_and_preserve_active_destinations() {
        let snapshot = CompositorSnapshot {
            workspaces: vec![
                Workspace {
                    id: 9,
                    name: "Work".into(),
                    windows: 2,
                    active: true,
                },
                Workspace {
                    id: -99,
                    name: "special:scratchpad".into(),
                    windows: 1,
                    active: false,
                },
            ],
            windows: vec![],
        };
        let buttons = buttons(&snapshot);
        assert_eq!(
            buttons.iter().map(|b| b.id).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6, 9]
        );
        assert!(buttons.last().unwrap().active && buttons.last().unwrap().occupied);
        assert!(buttons[..6].iter().all(|b| !b.active && !b.occupied));
        assert_eq!(snapshot.workspaces.len(), 2);
        assert_eq!(snapshot.workspaces[1].id, -99);
    }
}
