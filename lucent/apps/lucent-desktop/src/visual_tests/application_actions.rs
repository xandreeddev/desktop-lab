//! Exercise real child/root routing with injected ports and engine hit capture.
use super::*;
use lucent_domain::{ApplicationActionId, ApplicationPort, DomainError, StopSignal};
use std::sync::Mutex;

#[derive(Default)]
struct Applications {
    actions: Mutex<Vec<(App, ApplicationActionId)>>,
    main: Mutex<usize>,
    failure: Mutex<Option<DomainError>>,
}
impl ApplicationPort for Applications {
    fn discover(&self) -> lucent_domain::Result<Vec<App>> {
        Ok(vec![])
    }
    fn watch(&self, _: &mut dyn FnMut(lucent_domain::Result<Vec<App>>), _: &dyn StopSignal) {}
    fn launch(&self, _: &App) -> lucent_domain::Result<()> {
        *self.main.lock().unwrap() += 1;
        Ok(())
    }
    fn launch_action(&self, app: &App, action: &ApplicationActionId) -> lucent_domain::Result<()> {
        self.actions
            .lock()
            .unwrap()
            .push((app.clone(), action.clone()));
        self.failure.lock().unwrap().clone().map_or(Ok(()), Err)
    }
}

fn inspect(app: &Desktop) -> serde_json::Value {
    serde_json::from_str(&app.inspect()).unwrap()
}
fn choose(app: &mut Desktop, effects: &mut Effects<Message>) {
    let message = app.command("launcher action Action-0").unwrap().unwrap();
    app.update(message, effects);
}

#[test]
fn captured_application_action_uses_controls_port_and_acknowledges_only_its_menu() {
    let mut app = fixture(2, false);
    let port = Arc::new(Applications::default());
    app.adapters.apps = port.clone();
    prepare_application_actions(&mut app, 2);
    let captured = app.apps[0].clone();
    let mut effects = Effects::default();
    choose(&mut app, &mut effects);
    assert_eq!(effects.tasks.len(), 1);
    assert_eq!(effects.tasks[0].queue, crate::work::CONTROLS);
    assert!(
        inspect(&app)["application_actions"]["busy"]
            .as_bool()
            .unwrap()
    );
    assert!(
        port.actions.lock().unwrap().is_empty(),
        "UI admission performs no port call"
    );
    assert!(app.command("launcher action Action-0").is_err());
    let task = effects.tasks.remove(0);
    app.update(Message::Launcher(launcher::Message::Back), &mut effects);
    app.apps[0].actions[0].name = "New current label".into();
    app.update(
        Message::Catalog(crate::catalog::Message::Observed(Ok(app.apps.clone()))),
        &mut effects,
    );
    let open = app
        .command("launcher actions foot.desktop")
        .unwrap()
        .unwrap();
    app.update(open, &mut effects);
    app.update(task.run(), &mut effects);
    assert_eq!(
        *port.actions.lock().unwrap(),
        vec![(captured, ApplicationActionId::new("Action-0").unwrap())]
    );
    assert_eq!(
        *port.main.lock().unwrap(),
        0,
        "an action bypasses main activation/focus"
    );
    assert!(app.launcher.open() && app.launcher.actions_open());
    assert_eq!(inspect(&app)["application_actions"]["error"], "");
    assert_eq!(inspect(&app)["application_actions"]["busy"], false);
    choose(&mut app, &mut effects);
    let task = effects.tasks.pop().unwrap();
    app.update(task.run(), &mut effects);
    assert!(
        !app.launcher.open(),
        "current successful dispatch acknowledges its originating menu"
    );
}

#[test]
fn action_failure_is_local_and_queue_failure_cannot_affect_a_retired_menu() {
    let mut app = fixture(2, false);
    let port = Arc::new(Applications::default());
    *port.failure.lock().unwrap() = Some(DomainError::Unavailable("Action denied".into()));
    app.adapters.apps = port;
    prepare_application_actions(&mut app, 2);
    app.error = "Unrelated earlier operation".into();
    let mut effects = Effects::default();
    choose(&mut app, &mut effects);
    let completion = effects.tasks.remove(0).run();
    app.update(completion, &mut effects);
    assert!(
        inspect(&app)["application_actions"]["error"]
            .as_str()
            .unwrap()
            .contains("Action denied")
    );
    assert_eq!(app.error, "Unrelated earlier operation");
    app.update(Message::Completed(Ok(())), &mut effects);
    assert!(
        inspect(&app)["application_actions"]["error"]
            .as_str()
            .unwrap()
            .contains("Action denied")
    );
    choose(&mut app, &mut effects);
    let mut task = effects.tasks.remove(0);
    let fail = task
        .take_failure_handler()
        .expect("task-local rejection/panic mapper");
    app.update(Message::Launcher(launcher::Message::Back), &mut effects);
    let open = app
        .command("launcher actions foot.desktop")
        .unwrap()
        .unwrap();
    app.update(open, &mut effects);
    app.update(
        fail(RuntimeFailure {
            source: "effect queue controls".into(),
            reason: "queue full".into(),
        }),
        &mut effects,
    );
    assert!(app.launcher.open() && app.launcher.actions_open());
    assert_eq!(inspect(&app)["application_actions"]["error"], "");
    assert_eq!(inspect(&app)["application_actions"]["busy"], false);
    choose(&mut app, &mut effects);
    let failure = effects.tasks.remove(0).take_failure_handler().unwrap();
    app.update(
        failure(RuntimeFailure {
            source: "effect queue controls".into(),
            reason: "worker panic".into(),
        }),
        &mut effects,
    );
    assert!(
        inspect(&app)["application_actions"]["error"]
            .as_str()
            .unwrap()
            .contains("outcome is uncertain")
    );
}

#[test]
fn application_action_press_retires_with_changed_removed_or_unavailable_catalog() {
    for change in ["changed", "removed", "unavailable"] {
        let mut app = fixture(2, false);
        let port = Arc::new(Applications::default());
        app.adapters.apps = port.clone();
        prepare_application_actions(&mut app, 2);
        let layout = Layout::new(fonts(&app));
        let mut input = Interaction::default();
        let before = scene(&app, &layout, &mut input, 1.);
        let hit = before
            .hits
            .iter()
            .find(|hit| hit.id == "application-action-Action-0")
            .unwrap();
        let (x, y) = (hit.rect.x + hit.rect.w / 2., hit.rect.y + hit.rect.h / 2.);
        assert!(input.press(&before, x, y).is_none());
        let update = if change == "unavailable" {
            Err(DomainError::Unavailable("Catalog stopped".into()))
        } else {
            let mut applications = app.apps.clone();
            if change == "changed" {
                applications[0].actions[0].launch = applications[1].launch.clone();
                applications[0].actions[0].name = "Replaced".into();
            } else {
                applications.remove(0);
            }
            Ok(applications)
        };
        let mut effects = Effects::default();
        app.update(
            Message::Catalog(crate::catalog::Message::Observed(update)),
            &mut effects,
        );
        assert!(!app.launcher.actions_open(), "{change}");
        let after = scene(&app, &layout, &mut input, 2.);
        assert!(
            !after
                .hits
                .iter()
                .any(|hit| hit.id.starts_with("application-action-"))
        );
        if let Some(stale) = input.release(x, y) {
            app.update(stale, &mut effects);
        }
        assert!(app.command("launcher action Action-0").is_err());
        assert!(port.actions.lock().unwrap().is_empty());
        assert!(
            !inspect(&app)["application_actions"]["busy"]
                .as_bool()
                .unwrap()
        );
    }
}

#[test]
fn application_actions_have_native_hit_authority_and_narrow_keyboard_reachability() {
    for width in [390., 640.] {
        let mut app = fixture(2, false);
        resize_fixture(&mut app, width, 320.);
        prepare_application_actions(&mut app, 32);
        app.update(
            Message::Launcher(launcher::Message::Back),
            &mut Effects::default(),
        );
        app.update(
            Message::Launcher(launcher::Message::Query("ter".into())),
            &mut Effects::default(),
        );
        let layout = Layout::new(fonts(&app));
        let mut input = Interaction::default();
        let applications = scene(&app, &layout, &mut input, 1.);
        let row = applications
            .hits
            .iter()
            .find(|hit| hit.id == "result-0")
            .unwrap();
        let secondary = row
            .secondary_click
            .clone()
            .expect("native secondary button action");
        app.update(secondary, &mut Effects::default());
        assert_eq!(app.launcher.query(), "ter");
        app.update(
            Message::Launcher(launcher::Message::Navigate(i32::MAX)),
            &mut Effects::default(),
        );
        let actions = scene(&app, &layout, &mut input, 2.);
        let back = actions
            .hits
            .iter()
            .find(|hit| hit.id == "application-actions-back")
            .unwrap();
        assert!(fully_visible(back.rect, back.clip.bounds));
        let choices = actions
            .hits
            .iter()
            .filter(|hit| hit.id.starts_with("application-action-"))
            .collect::<Vec<_>>();
        assert!(!choices.is_empty() && choices.len() < 32);
        for hit in choices {
            assert!(
                fully_visible(hit.rect, hit.clip.bounds),
                "{} at {width}",
                hit.id
            );
        }
        let last = actions
            .hits
            .iter()
            .find(|hit| hit.id == "application-action-Action-31")
            .unwrap();
        assert!(fully_visible(last.rect, last.clip.bounds));
        assert_eq!(
            inspect(&app)["application_actions"]["selected"],
            "Action-31"
        );
        let back_message = back.click.clone().unwrap();
        app.update(back_message, &mut Effects::default());
        assert!(app.launcher.open() && !app.launcher.actions_open());
        assert_eq!(app.launcher.query(), "ter");
        key(&mut app, Key::ContextMenu);
        assert!(app.launcher.actions_open());
        key(&mut app, Key::Escape);
        assert!(app.launcher.open() && !app.launcher.actions_open());
        key(&mut app, Key::Escape);
        assert!(!app.launcher.open());
    }
}
