//! Desktop presentation policy, separate from the compositor's reported state.
use lucent_domain::{CompositorSnapshot, MonitorId, WorkspaceCommand, WorkspaceRef};
use std::collections::{BTreeMap, BTreeSet};

/// Numbered shortcuts may exist before the compositor creates a workspace.
/// Named/special controls represent unique currently observed destinations.
pub struct WorkspaceButton {
    pub id: WorkspaceRef,
    pub label: Option<String>,
    pub occupied: bool,
    pub active: bool,
}
impl WorkspaceButton {
    pub fn command(&self) -> WorkspaceCommand {
        if self.id.is_special() {
            WorkspaceCommand::ToggleSpecial(self.id.clone())
        } else {
            WorkspaceCommand::Select(self.id.clone())
        }
    }
    pub fn element_id(&self) -> String {
        if let Some(number) = self.id.number() {
            return format!("workspace-{number}");
        }
        let kind = if self.id.is_special() {
            "special"
        } else {
            "named"
        };
        let name = self.id.name().expect("non-numbered workspace");
        let encoded: String = name
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("workspace-{kind}-{encoded}")
    }
}
pub fn buttons_for_monitor(
    snapshot: &CompositorSnapshot,
    monitor: Option<&MonitorId>,
) -> Vec<WorkspaceButton> {
    let selected = monitor.and_then(|id| snapshot.monitor(id));
    let local_active = |id: &WorkspaceRef| {
        selected.is_some_and(|monitor| {
            if id.is_special() {
                monitor.special_workspace.as_ref() == Some(id)
            } else {
                monitor.active_workspace.as_ref() == Some(id)
            }
        })
    };
    let mut buttons: BTreeMap<_, _> = (1..=6)
        .map(|number| {
            let id = WorkspaceRef::numbered(number).expect("positive shortcut");
            let active = monitor.is_some() && local_active(&id);
            (
                id.clone(),
                WorkspaceButton {
                    id,
                    label: None,
                    occupied: false,
                    active,
                },
            )
        })
        .collect();
    let mut ambiguous = BTreeSet::new();
    let mut observed = BTreeSet::new();
    for workspace in &snapshot.workspaces {
        if !observed.insert(workspace.id.clone()) {
            ambiguous.insert(workspace.id.clone());
        }
    }
    for workspace in &snapshot.workspaces {
        if ambiguous.contains(&workspace.id) {
            continue;
        }
        let label = workspace.id.name().map(|name| {
            if workspace.id.is_special() {
                "S".to_string()
            } else {
                name.chars().next().unwrap_or('?').to_uppercase().collect()
            }
        });
        buttons.insert(
            workspace.id.clone(),
            WorkspaceButton {
                id: workspace.id.clone(),
                label,
                occupied: workspace.windows > 0,
                active: if monitor.is_some() {
                    local_active(&workspace.id)
                } else {
                    workspace.active
                },
            },
        );
    }
    buttons.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucent_domain::Workspace;
    #[derive(Default)]
    struct CompositorFixture(std::sync::Mutex<Vec<WorkspaceCommand>>);
    impl lucent_domain::CompositorPort for CompositorFixture {
        fn snapshot(&self) -> lucent_domain::Result<CompositorSnapshot> {
            Ok(CompositorSnapshot::default())
        }
        fn workspace(&self, command: WorkspaceCommand) -> lucent_domain::Result<()> {
            self.0.lock().unwrap().push(command);
            Ok(())
        }
        fn focus_window(&self, _: &str) -> lucent_domain::Result<()> {
            Ok(())
        }
        fn watch(
            &self,
            _: &mut dyn FnMut(lucent_domain::Result<CompositorSnapshot>),
            _: &dyn lucent_domain::StopSignal,
        ) {
        }
    }
    #[test]
    fn client_workspace_effect_revalidates_authority_and_does_not_invent_active_or_persist() {
        use crate::desktop::{Desktop, Message};
        use lucent_api::{Component, Effects};
        let fixture = std::sync::Arc::new(CompositorFixture::default());
        let mut adapters = crate::test_adapters::adapters();
        adapters.compositor = Some(fixture.clone());
        let mut app = Desktop::new(adapters);
        let id = WorkspaceRef::named("Writing").unwrap();
        let workspace = Workspace {
            id: id.clone(),
            name: "Title".into(),
            windows: 1,
            active: false,
        };
        let mut effects = Effects::default();
        app.update(
            Message::Workspace(WorkspaceCommand::Select(id.clone())),
            &mut effects,
        );
        assert!(effects.tasks.is_empty(), "no available observation");
        app.update(
            Message::Compositor(Ok(CompositorSnapshot {
                workspaces: vec![workspace.clone()],
                ..Default::default()
            })),
            &mut effects,
        );
        let settings = app.settings.clone();
        let snapshot = app.compositor.clone();
        effects.tasks.clear();
        app.update(
            Message::Workspace(WorkspaceCommand::Select(id.clone())),
            &mut effects,
        );
        assert_eq!(effects.tasks.len(), 1);
        assert_eq!(effects.tasks[0].queue, crate::work::CONTROLS);
        let completed = effects.tasks.remove(0).run();
        app.update(completed, &mut effects);
        assert_eq!(
            *fixture.0.lock().unwrap(),
            [WorkspaceCommand::Select(id.clone())]
        );
        assert_eq!(app.settings, settings);
        assert_eq!(
            app.compositor, snapshot,
            "acknowledgement is not active-state observation"
        );
        app.compositor.workspaces.push(workspace);
        app.update(
            Message::Workspace(WorkspaceCommand::Select(id.clone())),
            &mut effects,
        );
        assert!(
            effects.tasks.is_empty(),
            "ambiguous identity cannot dispatch"
        );
        app.compositor.workspaces.clear();
        app.update(
            Message::Workspace(WorkspaceCommand::Select(id)),
            &mut effects,
        );
        assert!(
            effects.tasks.is_empty(),
            "retired named identity cannot dispatch"
        );
        app.update(
            Message::Workspace(WorkspaceCommand::Select(WorkspaceRef::numbered(6).unwrap())),
            &mut effects,
        );
        assert_eq!(
            effects.tasks.len(),
            1,
            "numbered shortcuts may create their destination"
        );
        effects.tasks.clear();
        app.update(
            Message::Compositor(Err(lucent_domain::DomainError::Unavailable(
                "fixture disconnect".into(),
            ))),
            &mut effects,
        );
        app.update(
            Message::Workspace(WorkspaceCommand::Select(WorkspaceRef::numbered(6).unwrap())),
            &mut effects,
        );
        assert!(
            effects.tasks.is_empty(),
            "disconnect retires action authority"
        );
    }
    #[test]
    fn named_and_special_controls_keep_separate_monitor_local_authority_and_actions() {
        let monitor = MonitorId::new("DP-1").unwrap();
        let named = WorkspaceRef::named("Writing").unwrap();
        let special = WorkspaceRef::special("notes").unwrap();
        let snapshot = CompositorSnapshot {
            workspaces: vec![
                Workspace {
                    id: named.clone(),
                    name: "Displayed title".into(),
                    windows: 1,
                    active: false,
                },
                Workspace {
                    id: special.clone(),
                    name: "Notes".into(),
                    windows: 1,
                    active: false,
                },
            ],
            monitors: vec![lucent_domain::CompositorMonitor {
                id: monitor.clone(),
                focused: true,
                active_workspace: Some(named.clone()),
                special_workspace: Some(special.clone()),
                has_fullscreen_window: false,
            }],
            ..Default::default()
        };
        let buttons = buttons_for_monitor(&snapshot, Some(&monitor));
        let writing = buttons.iter().find(|button| button.id == named).unwrap();
        let notes = buttons.iter().find(|button| button.id == special).unwrap();
        assert!(writing.active && notes.active);
        assert_eq!(writing.command(), WorkspaceCommand::Select(named));
        assert_eq!(notes.command(), WorkspaceCommand::ToggleSpecial(special));
        assert_ne!(writing.element_id(), notes.element_id());
        assert_eq!(buttons[1].element_id(), "workspace-2");
    }
    #[test]
    fn each_monitor_uses_its_observed_workspace_without_copying_global_focus() {
        use lucent_domain::{CompositorMonitor, WorkspaceRef};
        let left = MonitorId::new("DP-1").unwrap();
        let right = MonitorId::new("DP-2").unwrap();
        let snapshot = CompositorSnapshot {
            workspaces: vec![
                Workspace {
                    id: WorkspaceRef::numbered(2).unwrap(),
                    name: "2".into(),
                    windows: 1,
                    active: false,
                },
                Workspace {
                    id: WorkspaceRef::numbered(9).unwrap(),
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
                    has_fullscreen_window: false,
                },
                CompositorMonitor {
                    id: right.clone(),
                    focused: false,
                    active_workspace: Some(WorkspaceRef::numbered(2).unwrap()),
                    special_workspace: Some(WorkspaceRef::special("notes").unwrap()),
                    has_fullscreen_window: false,
                },
            ],
        };
        assert_eq!(
            buttons_for_monitor(&snapshot, Some(&left))
                .iter()
                .filter(|button| button.active)
                .filter_map(|button| button.id.number())
                .collect::<Vec<_>>(),
            [9]
        );
        assert_eq!(
            buttons_for_monitor(&snapshot, Some(&right))
                .iter()
                .filter(|button| button.active)
                .filter_map(|button| button.id.number())
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
                .name(),
            Some("notes")
        );
    }
    #[test]
    fn shortcuts_do_not_mutate_reported_workspaces_and_preserve_active_destinations() {
        let snapshot = CompositorSnapshot {
            monitors: vec![],
            workspaces: vec![
                Workspace {
                    id: WorkspaceRef::numbered(9).unwrap(),
                    name: "Work".into(),
                    windows: 2,
                    active: true,
                },
                Workspace {
                    id: WorkspaceRef::special("scratchpad").unwrap(),
                    name: "special:scratchpad".into(),
                    windows: 1,
                    active: false,
                },
            ],
            windows: vec![],
        };
        let buttons = buttons_for_monitor(&snapshot, None);
        assert_eq!(
            buttons
                .iter()
                .filter_map(|b| b.id.number())
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6, 9]
        );
        let nine = buttons
            .iter()
            .find(|button| button.id.number() == Some(9))
            .unwrap();
        assert!(nine.active && nine.occupied);
        assert!(buttons.last().unwrap().id.is_special());
        assert!(buttons[..6].iter().all(|b| !b.active && !b.occupied));
        assert_eq!(snapshot.workspaces.len(), 2);
        assert!(snapshot.workspaces[1].id.is_special());
    }
}
