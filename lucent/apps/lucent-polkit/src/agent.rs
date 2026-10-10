//! Standard libpolkit-agent listener/session adapter. No custom PAM or authority protocol.
use gio::prelude::CancellableExtManual;
use glib::{prelude::*, subclass::prelude::*};
use lucent_domain::*;
use polkit_agent_rs::{self as polkit_agent, gio, polkit, subclass::ListenerImpl, traits::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

enum Event {
    Prompt(AuthPrompt),
    Finished(bool),
}
enum Reply {
    Answer(Secret),
    Cancel,
    Closed,
}
struct ConversationAdapter {
    events: Mutex<mpsc::Receiver<Event>>,
    replies: async_channel::Sender<Reply>,
    cancelled: Arc<AtomicBool>,
}
impl AuthenticationPort for ConversationAdapter {
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    fn authenticate(&self, _: &str, conversation: &mut dyn AuthConversation) -> Result<()> {
        let events = self
            .events
            .lock()
            .map_err(|_| DomainError::Failed("Authorization channel failed".into()))?;
        loop {
            match events
                .recv()
                .map_err(|_| DomainError::Failed("Authorization ended".into()))?
            {
                Event::Prompt(prompt) => {
                    let needs_reply =
                        matches!(prompt.kind, PromptKind::Visible | PromptKind::Secret);
                    let answer = conversation.prompt(prompt)?;
                    if needs_reply {
                        self.replies
                            .send_blocking(Reply::Answer(answer))
                            .map_err(|_| DomainError::Failed("Authorization ended".into()))?;
                    }
                }
                Event::Finished(true) => return Ok(()),
                Event::Finished(false) => {
                    return Err(DomainError::Failed("Authorization was not granted".into()));
                }
            }
        }
    }
}
mod implementation {
    use super::*;
    #[derive(Default)]
    pub struct Agent {
        pub busy: Rc<Cell<bool>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Agent {
        const NAME: &'static str = "LucentPolkitAgent";
        type Type = super::Agent;
        type ParentType = polkit_agent::Listener;
    }
    impl ObjectImpl for Agent {}
    impl ListenerImpl for Agent {
        type Message = bool;
        fn initiate_authentication(
            &self,
            _action_id: &str,
            message: &str,
            _icon: &str,
            _details: &polkit::Details,
            cookie: &str,
            identities: Vec<polkit::Identity>,
            cancellable: gio::Cancellable,
            task: gio::Task<bool>,
        ) {
            // Never invent an identity: only the authority's offered users are eligible.
            let identity = identities
                .iter()
                .find(|identity| {
                    identity
                        .downcast_ref::<polkit::UnixUser>()
                        .and_then(|user| user.name())
                        .is_some_and(|name| {
                            Some(name.as_str()) == std::env::var("USER").ok().as_deref()
                        })
                })
                .or_else(|| identities.first());
            let Some(identity) = identity.filter(|_| !self.busy.get()) else {
                // This task has one completion path and has never been propagated.
                unsafe {
                    task.return_result(Err(glib::Error::new(
                        gio::IOErrorEnum::Cancelled,
                        "Authorization agent is busy or has no eligible identity",
                    )));
                }
                return;
            };
            self.busy.set(true);
            let name = identity
                .downcast_ref::<polkit::UnixUser>()
                .and_then(|user| user.name())
                .map(|name| name.to_string())
                .unwrap_or_else(|| polkit::traits::IdentityExt::to_string(identity).to_string());
            let session = polkit_agent::Session::new(identity, cookie);
            let (events, receive) = mpsc::channel();
            let (replies, commands) = async_channel::bounded(4);
            let cancelled = Arc::new(AtomicBool::new(false));
            let adapter = Arc::new(ConversationAdapter {
                events: Mutex::new(receive),
                replies: replies.clone(),
                cancelled: cancelled.clone(),
            });
            let send = events.clone();
            session.connect_request(move |_, text, echo| {
                let _ = send.send(Event::Prompt(AuthPrompt {
                    text: text.into(),
                    kind: if echo {
                        PromptKind::Visible
                    } else {
                        PromptKind::Secret
                    },
                }));
            });
            let send = events.clone();
            session.connect_show_error(move |_, text| {
                let _ = send.send(Event::Prompt(AuthPrompt {
                    text: text.into(),
                    kind: PromptKind::Error,
                }));
            });
            let send = events.clone();
            session.connect_show_info(move |_, text| {
                let _ = send.send(Event::Prompt(AuthPrompt {
                    text: text.into(),
                    kind: PromptKind::Information,
                }));
            });
            let task = Rc::new(RefCell::new(Some(task)));
            let done = task.clone();
            let complete_cancelled = cancelled.clone();
            session.connect_completed(move |_, granted| {
                let _ = events.send(Event::Finished(granted));
                complete_cancelled.store(true, Ordering::Release);
                if let Some(task) = done.borrow_mut().take() {
                    // Completion means the conversation ended. libpolkit's trusted
                    // helper, not this UI result, reports authorization to polkitd.
                    unsafe {
                        task.return_result(Ok(true));
                    }
                }
            });
            let cancellation = cancelled.clone();
            let sender = replies.clone();
            cancellable.connect_cancelled(move |_| {
                cancellation.store(true, Ordering::Release);
                let _ = sender.try_send(Reply::Cancel);
            });
            let busy = self.busy.clone();
            let active = session.clone();
            glib::MainContext::default().spawn_local(async move {
                while let Ok(reply) = commands.recv().await {
                    match reply {
                        Reply::Cancel => active.cancel(),
                        Reply::Answer(secret) => {
                            if task.borrow().is_some() && !cancelled.load(Ordering::Acquire) {
                                active.response(secret.expose());
                            }
                        }
                        Reply::Closed => {
                            cancelled.store(true, Ordering::Release);
                            active.cancel();
                            if let Some(task) = task.borrow_mut().take() {
                                unsafe {
                                    task.return_result(Err(glib::Error::new(
                                        gio::IOErrorEnum::Cancelled,
                                        "Authorization cancelled",
                                    )));
                                }
                            }
                            busy.set(false);
                            break;
                        }
                    }
                }
            });
            let message = message.to_string();
            std::thread::spawn(move || {
                let light = lucent_services::JsonSettings::default()
                    .load()
                    .map(|s| s.light)
                    .unwrap_or(false);
                let screen = lucent_session::SessionScreen::new(
                    lucent_session::Mode::Authorization,
                    name,
                    adapter,
                )
                .with_palette(
                    lucent_services::JsonSettings::default()
                        .load()
                        .ok()
                        .and_then(|s| s.palette),
                )
                .with_theme(light)
                .with_authorization(message);
                let _ = lucent_wayland::run(screen);
                let _ = replies.send_blocking(Reply::Closed);
            });
            session.initiate();
        }
        fn initiate_authentication_finish(
            &self,
            task: std::result::Result<gio::Task<bool>, glib::Error>,
        ) -> bool {
            // The library invokes finish once, after the callback completed this task.
            task.and_then(|task| unsafe { task.propagate() })
                .unwrap_or(false)
        }
    }
}
glib::wrapper! { pub struct Agent(ObjectSubclass<implementation::Agent>) @extends polkit_agent::Listener; }
pub fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let context = glib::MainContext::default();
    let _guard = context.acquire()?;
    let subject = if let Ok(id) = std::env::var("XDG_SESSION_ID") {
        polkit::UnixSession::new(&id)
    } else {
        polkit::UnixSession::new_for_process_sync(
            std::process::id() as i32,
            None::<&gio::Cancellable>,
        )?
    };
    let agent: Agent = glib::Object::new();
    let _registration = agent.register(
        polkit_agent::RegisterFlags::NONE,
        &subject,
        "/org/lucent/PolicyKit1/AuthenticationAgent",
        None::<&gio::Cancellable>,
    )?;
    println!("Lucent authorization agent registered");
    glib::MainLoop::new(Some(&context), false).run();
    Ok(())
}
