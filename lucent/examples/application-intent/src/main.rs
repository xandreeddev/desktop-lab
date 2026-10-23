//! Independent graphical consumer: physical input -> SDK intent -> domain port.
//! The composition root selects the XDG adapter; the component only holds a port.
use lucent_api::*;
use lucent_design::{Theme, component::example, font, radius, space};
use lucent_domain as domain;
use lucent_services::applications::XdgApplications;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Copy)]
enum Operation {
    Launch,
    Open,
    Action,
}
#[derive(Clone)]
enum Message {
    Interact,
    Request(Operation),
    Prepared(Operation, ActivationOutcome),
    Finished(Result<(), String>),
    Quit,
}
struct IntentExample {
    applications: Arc<dyn domain::ApplicationPort>,
    application: domain::Application,
    open: domain::ApplicationOpenRequest,
    action: Option<domain::ApplicationActionId>,
    keyboard_interactive: bool,
    busy: bool,
    completed: u64,
    fresh_token: bool,
    status: String,
}
impl Component for IntentExample {
    type Message = Message;
    fn view(&self, _: &ViewContext) -> Element<Message> {
        let theme = Theme::new(false);
        let mut buttons = vec![
            theme
                .button("Launch", Message::Request(Operation::Launch))
                .id("intent-launch")
                .width(Length::Fill),
            theme
                .button("Open", Message::Request(Operation::Open))
                .id("intent-open")
                .width(Length::Fill),
        ];
        if self.action.is_some() {
            buttons.push(
                theme
                    .button("Action", Message::Request(Operation::Action))
                    .id("intent-action")
                    .width(Length::Fill),
            );
        }
        theme.apply(
            Element::column(vec![
                Element::text("Native application intent").font(font::TITLE),
                Element::text(self.application.name.clone())
                    .font(font::BODY)
                    .wrap(2),
                Element::row(buttons).gap(space::SM),
                Element::text(self.status.clone())
                    .font(font::CAPTION)
                    .wrap(3),
                theme.button("Close", Message::Quit).id("intent-close"),
            ])
            .padding(space::XXL)
            .gap(space::MD)
            .size(example::WIDTH, example::HEIGHT)
            .background(theme.surface)
            .radius(radius::PANEL),
        )
    }
    fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Interact if !self.busy => self.keyboard_interactive = true,
            Message::Request(operation) if !self.busy => {
                // Like the desktop launcher, yield in the same update that
                // requests authority from the current physical input. The
                // runtime captures that input before reconciling this policy.
                self.keyboard_interactive = false;
                self.busy = true;
                self.fresh_token = false;
                self.status = "Requesting intent from this input event…".into();
                let app_id = self
                    .application
                    .id
                    .as_str()
                    .strip_suffix(".desktop")
                    .unwrap_or(self.application.id.as_str());
                effects.activation_token(Some(app_id.to_owned()), move |outcome| {
                    Message::Prepared(operation, outcome)
                });
            }
            Message::Prepared(operation, outcome) if self.busy => {
                let context = match outcome {
                    ActivationOutcome::Token(token) => {
                        // The SDK has already checked the opaque token. Neither
                        // logs, inspection nor process arguments expose its value.
                        self.fresh_token = true;
                        domain::ApplicationLaunchContext::with_activation_token(token.into_value())
                            .expect("SDK-checked activation token")
                    }
                    _ => domain::ApplicationLaunchContext::default(),
                };
                let port = self.applications.clone();
                let app = self.application.clone();
                let open = self.open.clone();
                let action = self.action.clone();
                self.status = "Waiting for the application adapter…".into();
                effects.task_on_with_failure(
                    "applications",
                    move || {
                        Message::Finished(
                            match operation {
                                Operation::Launch => port.launch_with_context(&app, context),
                                Operation::Open => port.open(&app, &open, context),
                                Operation::Action => match action {
                                    Some(action) => {
                                        port.launch_action_with_context(&app, &action, context)
                                    }
                                    None => Err(domain::DomainError::Invalid(
                                        "No declared action was selected".into(),
                                    )),
                                },
                            }
                            .map_err(|error| error.to_string()),
                        )
                    },
                    |failure| Message::Finished(Err(failure.to_string())),
                );
            }
            Message::Finished(result) if self.busy => {
                self.busy = false;
                self.completed += 1;
                self.status = match result {
                    Ok(()) => format!(
                        "Adapter acknowledged · fresh input token: {}. Check the application window.",
                        self.fresh_token
                    ),
                    Err(error) => error,
                };
            }
            Message::Quit => effects.quit(),
            _ => return,
        }
        effects.redraw("intent");
    }
}
impl Application for IntentExample {
    fn name(&self) -> &'static str {
        "application-intent"
    }
    fn activation_enabled(&self) -> bool {
        true
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "intent",
            instance: None,
            output: None,
            layer: Layer::Overlay,
            anchor: Anchor::Center,
            margins: Default::default(),
            redraw_group: None,
            width: example::WIDTH as u32,
            height: example::HEIGHT as u32,
            exclusive_zone: 0,
            keyboard: if self.keyboard_interactive {
                Keyboard::OnDemand
            } else {
                Keyboard::None
            },
            visible: true,
            pointer_input: PointerInput::Content,
        }]
    }
    fn event(&self, event: Event) -> Option<Message> {
        match event {
            Event::PointerButton {
                surface: "intent",
                pressed: true,
                ..
            } if !self.busy => Some(Message::Interact),
            Event::Key { key, .. } if matches!(key.unmodified(), Key::Escape) => {
                Some(Message::Quit)
            }
            _ => None,
        }
    }
    fn command(&self, command: &str) -> Result<Option<Message>, String> {
        match command {
            // A socket request deliberately has no physical input authority.
            "launch" => Ok(Some(Message::Request(Operation::Launch))),
            "open" => Ok(Some(Message::Request(Operation::Open))),
            "action" if self.action.is_some() => Ok(Some(Message::Request(Operation::Action))),
            "quit" => Ok(Some(Message::Quit)),
            _ => Err("Choose launch, open, action or quit".into()),
        }
    }
    fn inspect(&self) -> String {
        serde_json::json!({
            "busy":self.busy, "completed":self.completed, "fresh_token":self.fresh_token,
        })
        .to_string()
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut roots = vec![];
    let mut id = None;
    let mut action = None;
    let mut targets = vec![];
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" => {
                println!(
                    "application-intent --app APP_ID --target ABSOLUTE_FILE_OR_URI [--target ...] [--action DECLARED_ID] [--data-dir ABSOLUTE_ROOT]"
                );
                return Ok(());
            }
            "--app" if id.is_none() => {
                id = Some(domain::AppId::new(args.next().ok_or("--app needs an ID")?)?);
            }
            "--action" if action.is_none() => {
                action = Some(domain::ApplicationActionId::new(
                    args.next().ok_or("--action needs an ID")?,
                )?);
            }
            "--data-dir" => roots.push(PathBuf::from(
                args.next().ok_or("--data-dir needs an absolute root")?,
            )),
            "--target" => {
                let value = args.next().ok_or("--target needs a file or URI")?;
                targets.push(if value.starts_with('/') {
                    domain::ApplicationOpenTarget::file(value)?
                } else {
                    domain::ApplicationOpenTarget::uri_value(value)?
                });
            }
            _ => return Err("Unknown or repeated argument; use --help".into()),
        }
    }
    let id = id.ok_or("--app is required")?;
    let open = domain::ApplicationOpenRequest::new(targets)?;
    // Production adapter selection occurs here, outside the component and view.
    let applications: Arc<dyn domain::ApplicationPort> = Arc::new(if roots.is_empty() {
        XdgApplications::default()
    } else {
        XdgApplications::with_source(roots, "Hyprland".into(), String::new())?
    });
    let application = applications
        .discover()?
        .into_iter()
        .find(|application| application.id == id)
        .ok_or("The selected application is unavailable")?;
    if action
        .as_ref()
        .is_some_and(|id| !application.actions.iter().any(|action| &action.id == id))
    {
        return Err("The selected application does not declare that action".into());
    }
    lucent_wayland::run(IntentExample {
        applications,
        application,
        open,
        action,
        keyboard_interactive: true,
        busy: false,
        completed: 0,
        fresh_token: false,
        status: "Click a control to request fresh compositor activation authority.".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingApplications {
        calls: Mutex<Vec<(&'static str, Option<String>)>>,
    }
    impl RecordingApplications {
        fn record(
            &self,
            operation: &'static str,
            context: domain::ApplicationLaunchContext,
        ) -> domain::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push((operation, context.activation_token().map(str::to_owned)));
            Ok(())
        }
    }
    impl domain::ApplicationPort for RecordingApplications {
        fn discover(&self) -> domain::Result<Vec<domain::Application>> {
            unreachable!("the component does not discover applications")
        }
        fn watch(
            &self,
            _: &mut dyn FnMut(domain::Result<Vec<domain::Application>>),
            _: &dyn domain::StopSignal,
        ) {
            unreachable!("the component does not watch applications")
        }
        fn launch(&self, _: &domain::Application) -> domain::Result<()> {
            self.record("launch", domain::ApplicationLaunchContext::default())
        }
        fn launch_with_context(
            &self,
            _: &domain::Application,
            context: domain::ApplicationLaunchContext,
        ) -> domain::Result<()> {
            self.record("launch", context)
        }
        fn open(
            &self,
            _: &domain::Application,
            _: &domain::ApplicationOpenRequest,
            context: domain::ApplicationLaunchContext,
        ) -> domain::Result<()> {
            self.record("open", context)
        }
        fn launch_action(
            &self,
            _: &domain::Application,
            _: &domain::ApplicationActionId,
        ) -> domain::Result<()> {
            self.record("action", domain::ApplicationLaunchContext::default())
        }
        fn launch_action_with_context(
            &self,
            _: &domain::Application,
            _: &domain::ApplicationActionId,
            context: domain::ApplicationLaunchContext,
        ) -> domain::Result<()> {
            self.record("action", context)
        }
    }

    fn example() -> (IntentExample, Arc<RecordingApplications>) {
        let applications = Arc::new(RecordingApplications::default());
        let action = domain::ApplicationActionId::new("OpenOwned").unwrap();
        let launch = domain::ApplicationLaunch::DesktopService { fallback: None };
        (
            IntentExample {
                applications: applications.clone(),
                application: domain::Application {
                    id: domain::AppId::new("org.lucent.Intent.desktop").unwrap(),
                    name: "Intent fixture".into(),
                    description: String::new(),
                    keywords: vec![],
                    icon: String::new(),
                    startup_class: String::new(),
                    launch: launch.clone(),
                    actions: vec![domain::ApplicationAction {
                        id: action.clone(),
                        name: "Open owned".into(),
                        icon: String::new(),
                        launch,
                    }],
                },
                open: domain::ApplicationOpenRequest::new(vec![
                    domain::ApplicationOpenTarget::file("/owned/a b%雪").unwrap(),
                ])
                .unwrap(),
                action: Some(action),
                keyboard_interactive: true,
                busy: false,
                completed: 0,
                fresh_token: false,
                status: String::new(),
            },
            applications,
        )
    }
    fn apply(app: &mut IntentExample, message: Message) -> Effects<Message> {
        let mut effects = Effects::default();
        app.update(message, &mut effects);
        effects
    }
    fn press(surface: &'static str, pressed: bool) -> Event {
        Event::PointerButton {
            surface,
            button: MouseButton::Primary,
            pressed,
            x: 20.,
            y: 20.,
        }
    }

    #[test]
    fn request_yields_before_token_completion_or_adapter_dispatch() {
        for operation in [Operation::Launch, Operation::Open, Operation::Action] {
            let (mut app, adapter) = example();
            assert_eq!(app.surfaces()[0].keyboard, Keyboard::OnDemand);
            let effects = apply(&mut app, Message::Request(operation));
            let surface = app.surfaces().remove(0);
            assert_eq!(surface.keyboard, Keyboard::None);
            assert!(surface.visible);
            assert_eq!(surface.pointer_input, PointerInput::Content);
            assert!(app.busy);
            assert_eq!(effects.activation.len(), 1);
            assert_eq!(
                effects.activation[0].app_id.as_deref(),
                Some("org.lucent.Intent")
            );
            assert!(effects.tasks.is_empty());
            assert!(effects.redraw.contains("intent"));
            assert!(adapter.calls.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn prepared_operations_forward_context_without_reclaiming_keyboard() {
        for (operation, name) in [
            (Operation::Launch, "launch"),
            (Operation::Open, "open"),
            (Operation::Action, "action"),
        ] {
            let (mut app, adapter) = example();
            let mut requested = apply(&mut app, Message::Request(operation));
            let prepared = requested
                .activation
                .remove(0)
                .finish(ActivationOutcome::Token(
                    ActivationToken::new("controlled-token".into()).unwrap(),
                ));
            let mut effects = apply(&mut app, prepared);
            assert_eq!(app.surfaces()[0].keyboard, Keyboard::None);
            assert!(effects.activation.is_empty());
            assert_eq!(effects.tasks.len(), 1);
            assert!(adapter.calls.lock().unwrap().is_empty());
            let completed = effects.tasks.remove(0).run();
            assert_eq!(
                *adapter.calls.lock().unwrap(),
                vec![(name, Some("controlled-token".into()))]
            );
            apply(&mut app, completed);
            assert!(!app.busy);
            assert_eq!(app.completed, 1);
            assert!(app.fresh_token);
            assert_eq!(app.surfaces()[0].keyboard, Keyboard::None);
        }
    }

    #[test]
    fn completion_errors_do_not_reclaim_keyboard() {
        let (mut app, _) = example();
        apply(&mut app, Message::Request(Operation::Launch));
        apply(
            &mut app,
            Message::Finished(Err("adapter unavailable".into())),
        );
        assert!(!app.busy);
        assert_eq!(app.surfaces()[0].keyboard, Keyboard::None);
    }

    #[test]
    fn only_an_idle_press_on_the_own_surface_rearms_keyboard() {
        let (mut app, _) = example();
        apply(&mut app, Message::Request(Operation::Launch));
        assert!(app.event(press("intent", true)).is_none());
        apply(&mut app, Message::Interact);
        assert_eq!(app.surfaces()[0].keyboard, Keyboard::None);
        apply(&mut app, Message::Finished(Ok(())));
        assert!(app.event(press("intent", false)).is_none());
        assert!(app.event(press("other", true)).is_none());
        assert!(
            app.event(Event::PointerInside {
                surface: "intent",
                inside: true,
            })
            .is_none()
        );
        assert_eq!(app.surfaces()[0].keyboard, Keyboard::None);
        let message = app.event(press("intent", true)).unwrap();
        let effects = apply(&mut app, message);
        assert_eq!(app.surfaces()[0].keyboard, Keyboard::OnDemand);
        assert!(effects.activation.is_empty());
        assert!(effects.tasks.is_empty());
        assert!(effects.redraw.contains("intent"));
    }

    #[test]
    fn socket_request_neither_rearms_keyboard_nor_invents_a_token() {
        let (mut app, adapter) = example();
        app.keyboard_interactive = false;
        let message = app.command("open").unwrap().unwrap();
        let mut requested = apply(&mut app, message);
        assert_eq!(app.surfaces()[0].keyboard, Keyboard::None);
        let prepared = requested
            .activation
            .remove(0)
            .finish(ActivationOutcome::NoUserInput);
        let mut effects = apply(&mut app, prepared);
        let completed = effects.tasks.remove(0).run();
        apply(&mut app, completed);
        assert_eq!(*adapter.calls.lock().unwrap(), vec![("open", None)]);
        assert!(!app.fresh_token);
        assert_eq!(app.surfaces()[0].keyboard, Keyboard::None);
    }
}
