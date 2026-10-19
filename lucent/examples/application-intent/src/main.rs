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
            Message::Request(operation) if !self.busy => {
                self.busy = true;
                self.fresh_token = false;
                self.status = "Requesting intent from this input event…".into();
                let app_id = self
                    .application
                    .id
                    .as_str()
                    .strip_suffix(".desktop")
                    .unwrap_or(self.application.id.as_str());
                effects
                    .activation_token(app_id, move |outcome| Message::Prepared(operation, outcome));
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
            keyboard: Keyboard::OnDemand,
            visible: true,
            pointer_input: PointerInput::Content,
        }]
    }
    fn event(&self, event: Event) -> Option<Message> {
        match event {
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
        busy: false,
        completed: 0,
        fresh_token: false,
        status: "Click a control to request fresh compositor activation authority.".into(),
    })
}
