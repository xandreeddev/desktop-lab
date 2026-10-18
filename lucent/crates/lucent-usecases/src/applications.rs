//! Explicit declared application actions, independent of transport and UI state.
use lucent_domain::*;

pub fn application_action<'a>(
    app: &'a Application,
    id: &ApplicationActionId,
) -> Result<&'a ApplicationAction> {
    let mut matches = app.actions.iter().filter(|action| &action.id == id);
    let Some(action) = matches.next() else {
        return Err(DomainError::Invalid(
            "Application action is no longer available".into(),
        ));
    };
    if matches.next().is_some()
        || action.name.trim().is_empty()
        || action.name.chars().any(char::is_control)
    {
        return Err(DomainError::Invalid(
            "Application action is ambiguous or invalid".into(),
        ));
    }
    Ok(action)
}

/// Actions always dispatch the chosen action; they never focus a matching
/// running window or silently select the application's primary launch command.
pub fn launch_application_action(
    port: &dyn ApplicationPort,
    app: &Application,
    id: &ApplicationActionId,
) -> Result<()> {
    application_action(app, id)?;
    port.launch_action(app, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Adapter(Mutex<Vec<ApplicationActionId>>);
    impl ApplicationPort for Adapter {
        fn discover(&self) -> Result<Vec<Application>> {
            Ok(Vec::new())
        }
        fn watch(&self, _: &mut dyn FnMut(Result<Vec<Application>>), _: &dyn StopSignal) {}
        fn launch(&self, _: &Application) -> Result<()> {
            panic!("action must not use main launch")
        }
        fn launch_action(&self, _: &Application, action: &ApplicationActionId) -> Result<()> {
            self.0.lock().unwrap().push(action.clone());
            Ok(())
        }
    }
    fn app() -> Application {
        let launch = ApplicationLaunch::Command(LaunchCommand {
            program: "/fixture".into(),
            args: vec![],
            directory: None,
            terminal: false,
        });
        Application {
            id: AppId::new("org.example.App.desktop").unwrap(),
            name: "Fixture".into(),
            description: String::new(),
            keywords: vec![],
            icon: String::new(),
            startup_class: String::new(),
            launch: launch.clone(),
            actions: vec![ApplicationAction {
                id: ApplicationActionId::new("New-Window").unwrap(),
                name: "New window".into(),
                icon: String::new(),
                launch,
            }],
        }
    }
    #[test]
    fn exact_declared_action_dispatches_without_main_launch_and_rejects_ambiguity() {
        let port = Adapter::default();
        let mut app = app();
        let id = app.actions[0].id.clone();
        launch_application_action(&port, &app, &id).unwrap();
        assert_eq!(port.0.lock().unwrap().as_slice(), std::slice::from_ref(&id));
        assert!(
            launch_application_action(
                &port,
                &app,
                &ApplicationActionId::new("new-window").unwrap()
            )
            .is_err()
        );
        app.actions.push(app.actions[0].clone());
        assert!(launch_application_action(&port, &app, &id).is_err());
        app.actions.pop();
        app.actions[0].name = " \t ".into();
        assert!(launch_application_action(&port, &app, &id).is_err());
        assert_eq!(port.0.lock().unwrap().len(), 1);
    }
}
