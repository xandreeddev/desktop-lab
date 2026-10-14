//! Desktop presentation policy, separate from the compositor's reported state.
use lucent_domain::{CompositorSnapshot, MonitorId};
use std::collections::BTreeMap;

/// A destination shown in the bar may exist only as a shortcut; it is not a
/// claim that the compositor currently has a workspace with this ID.
pub struct WorkspaceButton {
    pub id: i32,
    pub occupied: bool,
    pub active: bool,
}
pub fn buttons_for_monitor(
    snapshot: &CompositorSnapshot,
    monitor: Option<&MonitorId>,
) -> Vec<WorkspaceButton> {
    let selected = monitor.and_then(|id| snapshot.monitor(id));
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
                    active: if monitor.is_some() {
                        selected.is_some_and(|monitor| {
                            monitor
                                .active_workspace
                                .as_ref()
                                .and_then(|reference| reference.number())
                                == Some(workspace.id)
                        })
                    } else {
                        workspace.active
                    },
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
    fn each_monitor_uses_its_observed_workspace_without_copying_global_focus() {
        use lucent_domain::{CompositorMonitor, WorkspaceRef};
        let left = MonitorId::new("DP-1").unwrap();
        let right = MonitorId::new("DP-2").unwrap();
        let snapshot = CompositorSnapshot {
            workspaces: vec![
                Workspace {
                    id: 2,
                    name: "2".into(),
                    windows: 1,
                    active: false,
                },
                Workspace {
                    id: 9,
                    name: "9".into(),
                    windows: 2,
                    active: true,
                },
            ],
            windows: vec![],
            monitors: vec![
                CompositorMonitor {
                    id: left.clone(),
                    focused: true,
                    active_workspace: Some(WorkspaceRef::numbered(9).unwrap()),
                    special_workspace: None,
                },
                CompositorMonitor {
                    id: right.clone(),
                    focused: false,
                    active_workspace: Some(WorkspaceRef::numbered(2).unwrap()),
                    special_workspace: Some(WorkspaceRef::named("special:notes").unwrap()),
                },
            ],
        };
        assert_eq!(
            buttons_for_monitor(&snapshot, Some(&left))
                .iter()
                .filter(|button| button.active)
                .map(|button| button.id)
                .collect::<Vec<_>>(),
            [9]
        );
        assert_eq!(
            buttons_for_monitor(&snapshot, Some(&right))
                .iter()
                .filter(|button| button.active)
                .map(|button| button.id)
                .collect::<Vec<_>>(),
            [2]
        );
        assert!(
            buttons_for_monitor(&snapshot, Some(&MonitorId::new("absent").unwrap()))
                .iter()
                .all(|button| !button.active)
        );
        assert_eq!(
            snapshot.monitors[1]
                .special_workspace
                .as_ref()
                .unwrap()
                .address(),
            Some("special:notes")
        );
    }
    #[test]
    fn shortcuts_do_not_mutate_reported_workspaces_and_preserve_active_destinations() {
        let snapshot = CompositorSnapshot {
            monitors: vec![],
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
        let buttons = buttons_for_monitor(&snapshot, None);
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
