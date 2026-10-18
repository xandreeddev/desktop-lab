//! Captured desktop-action authority and task-local presentation state.
use super::*;

pub(super) struct Menu {
    pub revision: u64,
    pub app: Application,
    pub selected: usize,
    pub scroll: usize,
    pub error: String,
}
pub(super) struct Pending {
    pub revision: u64,
    pub menu: u64,
    pub app: Application,
    pub action: ApplicationActionId,
}

impl Launcher {
    pub fn actions_open(&self) -> bool {
        self.action_menu.is_some()
    }
    pub fn actions_inspect(&self) -> serde_json::Value {
        let menu = self.action_menu.as_ref();
        serde_json::json!({
            "open": menu.is_some(),
            "application": menu.map(|menu| menu.app.id.as_str()),
            "selected": menu.and_then(|menu| menu.app.actions.get(menu.selected)).map(|action| action.id.as_str()),
            "actions": menu.map(|menu| menu.app.actions.iter().map(|action| serde_json::json!({"id":action.id.as_str(),"name":action.name})).collect::<Vec<_>>()).unwrap_or_default(),
            "busy": self.action_pending.is_some(),
            "error": menu.map_or("", |menu| menu.error.as_str()),
            "pending": self.action_pending.as_ref().map(|pending| serde_json::json!({"revision":pending.revision,"application":pending.app.id.as_str(),"action":pending.action.as_str()})),
        })
    }
    /// IPC may choose only an action in the currently captured menu.
    pub fn action_message(&self, value: &str) -> Result<Message, String> {
        let action = ApplicationActionId::new(value).map_err(|error| error.to_string())?;
        let menu = self
            .action_menu
            .as_ref()
            .ok_or("Open application actions first")?;
        if self.action_pending.is_some() {
            return Err("An application action is still pending".into());
        }
        if menu
            .app
            .actions
            .iter()
            .filter(|entry| entry.id == action)
            .count()
            != 1
        {
            return Err("Application action is unavailable".into());
        }
        Ok(Message::ChooseAction {
            menu: menu.revision,
            action,
        })
    }
    pub fn retire_actions(&mut self) {
        self.action_menu = None;
    }
    pub(super) fn presentation_count(&self) -> usize {
        self.action_menu
            .as_ref()
            .map_or(self.results.len(), |menu| menu.app.actions.len())
    }
    fn current_action_app(apps: &[Application], captured: &Application) -> bool {
        let mut matching = apps.iter().filter(|app| app.id == captured.id);
        matching.next() == Some(captured) && matching.next().is_none()
    }
    pub(super) fn revalidate_actions(&mut self, apps: &[Application]) {
        if self
            .action_menu
            .as_ref()
            .is_some_and(|menu| !Self::current_action_app(apps, &menu.app))
        {
            self.action_menu = None;
        }
    }
    pub(super) fn open_actions(
        &mut self,
        app: Application,
        input: Input<'_>,
        geometry: Geometry,
        now: f64,
    ) {
        if !matches!(input.application_status, crate::catalog::Status::Ready)
            || !Self::current_action_app(input.apps, &app)
        {
            return;
        }
        if !self.open || self.mode != Mode::Apps {
            self.open_mode(Mode::Apps, input, geometry, now);
        }
        if !self
            .results
            .iter()
            .any(|index| input.apps.get(*index) == Some(&app))
        {
            return;
        }
        let Some(revision) = self.action_revision.checked_add(1) else {
            return;
        };
        self.action_revision = revision;
        self.query_field.cancel();
        self.action_menu = Some(Menu {
            revision,
            app,
            selected: 0,
            scroll: 0,
            error: String::new(),
        });
        self.reconcile(geometry, now);
    }
    pub(super) fn choose_action(&mut self, index: usize, geometry: Geometry) {
        let Some(menu) = &mut self.action_menu else {
            return;
        };
        let count = menu.app.actions.len();
        menu.selected = index.min(count.saturating_sub(1));
        let rows =
            shell_layout::visible_rows(Mode::Apps, count, geometry.viewport, geometry.preferred);
        menu.scroll = menu
            .scroll
            .min(menu.selected)
            .max((menu.selected + 1).saturating_sub(rows))
            .min(count.saturating_sub(rows));
    }
    pub(super) fn admit_action(
        &mut self,
        revision: u64,
        action: ApplicationActionId,
        input: Input<'_>,
    ) -> Option<Intent> {
        if self.action_pending.is_some() {
            return None;
        }
        let menu = self.action_menu.as_mut()?;
        if menu.revision != revision {
            return None;
        }
        if !matches!(input.application_status, crate::catalog::Status::Ready)
            || !Self::current_action_app(input.apps, &menu.app)
        {
            self.action_menu = None;
            return None;
        }
        if menu
            .app
            .actions
            .iter()
            .filter(|entry| entry.id == action)
            .count()
            != 1
        {
            return None;
        }
        let Some(revision) = self.action_revision.checked_add(1) else {
            menu.error = "Application action request limit reached".into();
            return None;
        };
        self.action_revision = revision;
        menu.error.clear();
        let app = menu.app.clone();
        self.action_pending = Some(Pending {
            revision,
            menu: menu.revision,
            app: app.clone(),
            action: action.clone(),
        });
        Some(Intent::LaunchAction {
            revision,
            app,
            action,
        })
    }
    pub(super) fn finish_action(
        &mut self,
        revision: u64,
        result: lucent_domain::Result<()>,
        geometry: Geometry,
        now: f64,
    ) {
        if !self
            .action_pending
            .as_ref()
            .is_some_and(|pending| pending.revision == revision)
        {
            return;
        }
        let pending = self.action_pending.take().unwrap();
        let Some(menu) = &mut self.action_menu else {
            return;
        };
        if menu.revision != pending.menu || menu.app != pending.app {
            return;
        }
        match result {
            Ok(()) => self.close(geometry, now),
            Err(error) => menu.error = error.to_string(),
        }
    }
}
