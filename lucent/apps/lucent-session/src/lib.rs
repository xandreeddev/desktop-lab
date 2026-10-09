//! Shared authentication presentation. PAM/greetd are selected by the executables.
//! Secret input is held outside Element/Scene; only bullets reach the renderer.
use lucent_api::{self as api, *};
use lucent_design::{Theme, component::authentication as token, font, radius, space};
use lucent_domain::{self as domain, *};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use zeroize::Zeroize;
pub mod platform;
/// Presentation dependency. The component receives decoded pixels, never paths.
pub trait SessionAssets: Send + Sync {
    fn wallpaper(&self) -> Option<Arc<ImageData>>;
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Lock,
    Login,
}
#[derive(Clone)]
pub enum Message {
    Wallpaper(Option<Arc<ImageData>>),
    Key(Key),
    Submit,
    Prompt(AuthPrompt),
    Finished(domain::Result<()>),
    Retry,
}
struct Attempt {
    send: mpsc::SyncSender<Secret>,
    receive: Arc<Mutex<mpsc::Receiver<Secret>>>,
}
pub struct SessionScreen {
    mode: Mode,
    identity: String,
    secret: Secret,
    prompt: Option<AuthPrompt>,
    status: String,
    attempt: Option<Attempt>,
    verified: bool,
    backend: Arc<dyn AuthenticationPort>,
    assets: Option<Arc<dyn SessionAssets>>,
    wallpaper: Option<Arc<ImageData>>,
}
impl SessionScreen {
    pub fn new(mode: Mode, identity: String, backend: Arc<dyn AuthenticationPort>) -> Self {
        Self {
            mode,
            identity,
            secret: Secret::default(),
            prompt: None,
            status: String::new(),
            attempt: None,
            verified: false,
            backend,
            assets: None,
            wallpaper: None,
        }
    }
    pub fn with_assets(mut self, assets: Arc<dyn SessionAssets>) -> Self {
        self.assets = Some(assets);
        self
    }
    fn begin(&mut self) {
        self.secret.clear();
        self.prompt = None;
        self.status = "Connecting…".into();
        let (send, receive) = mpsc::sync_channel(1);
        self.attempt = Some(Attempt {
            send,
            receive: Arc::new(Mutex::new(receive)),
        });
    }
    fn input_enabled(&self) -> bool {
        self.prompt
            .as_ref()
            .is_some_and(|p| matches!(p.kind, PromptKind::Secret | PromptKind::Visible))
            || (self.mode == Mode::Login && self.attempt.is_none())
    }
}
impl Component for SessionScreen {
    type Message = Message;
    fn view(&self, cx: &ViewContext) -> Element<Message> {
        let t = Theme::new(false);
        let title = if self.mode == Mode::Lock {
            "Welcome back"
        } else {
            "Welcome to Lucent"
        };
        let username_step = self.mode == Mode::Login && self.attempt.is_none();
        let value = if username_step {
            self.identity.clone()
        } else if self
            .prompt
            .as_ref()
            .is_some_and(|p| p.kind == PromptKind::Visible)
        {
            self.secret.expose().to_string()
        } else {
            "•".repeat(self.secret.len().min(32))
        };
        let label = if username_step {
            "Username"
        } else {
            self.prompt
                .as_ref()
                .map(|p| p.text.as_str())
                .unwrap_or("Please wait")
        };
        let mut children = vec![
            Element::text("LUCENT").font(font::CAPTION).color(t.primary),
            Element::text(title)
                .font(token::TITLE_SIZE)
                .color(t.on_surface)
                .wrap(2),
            Element::text(if self.mode == Mode::Lock {
                &self.identity
            } else {
                "Your desktop, ready when you are"
            })
            .font(font::BODY)
            .color(t.on_surface),
            Element::text(label)
                .font(font::BODY)
                .color(t.on_surface)
                .wrap(3),
            Element::text(if value.is_empty() { " " } else { &value })
                .font(token::INPUT_SIZE)
                .color(t.on_surface)
                .width(Length::Fill)
                .height(Length::Fixed(token::INPUT_HEIGHT))
                .padding_xy(space::MD, space::SM)
                .background(t.surface_container)
                .radius(radius::CONTROL)
                .selected(self.input_enabled())
                .id("authentication-input"),
        ];
        if self.input_enabled() {
            children.push(
                t.button(
                    if username_step {
                        "Continue"
                    } else if self.mode == Mode::Lock {
                        "Unlock"
                    } else {
                        "Sign in"
                    },
                    Message::Submit,
                )
                .width(Length::Fill)
                .background(t.primary)
                .color(t.on_primary)
                .id("authentication-submit"),
            );
        }
        if self.mode == Mode::Lock && self.attempt.is_none() && !self.verified {
            children.push(
                t.button("Try again", Message::Retry)
                    .id("authentication-retry"),
            );
        }
        children.push(
            Element::text(&self.status)
                .font(font::CAPTION)
                .color(t.on_surface)
                .width(Length::Fill)
                .wrap(3),
        );
        let width = token::WIDTH.min(cx.width - space::XL * 2.);
        let mut layers = Vec::new();
        if let Some(wallpaper) = &self.wallpaper {
            layers.push(
                Element::image(wallpaper.clone())
                    .fill()
                    .cover()
                    .id("session-wallpaper"),
            );
        }
        layers.push(
            Element::column(children)
                .gap(space::LG)
                .padding(token::PADDING)
                .width(Length::Fixed(width))
                .at(
                    (cx.width - width) / 2.,
                    (cx.height - token::HEIGHT).max(space::XL) / 2.,
                )
                .radius(radius::PANEL)
                .background(t.surface_container)
                .shadow(),
        );
        t.apply(Element::stack(layers).fill().background(t.surface))
    }
    fn update(&mut self, message: Message, e: &mut Effects<Message>) {
        match message {
            Message::Wallpaper(image) => self.wallpaper = image,
            Message::Prompt(prompt) => {
                self.secret.clear();
                if matches!(prompt.kind, PromptKind::Information | PromptKind::Error) {
                    self.status = prompt.text;
                } else {
                    self.prompt = Some(prompt);
                    self.status.clear();
                }
            }
            Message::Finished(result) => {
                self.attempt = None;
                self.secret.clear();
                self.prompt = None;
                match result {
                    Ok(()) => {
                        self.verified = true;
                        self.status = "Welcome".into();
                        if self.mode == Mode::Login {
                            e.quit();
                        }
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
            Message::Retry if self.attempt.is_none() => self.begin(),
            Message::Submit => {
                if self.mode == Mode::Login && self.attempt.is_none() {
                    if !self.identity.trim().is_empty() {
                        self.identity = self.identity.trim().into();
                        self.begin();
                    }
                } else if self.prompt.is_some()
                    && let Some(attempt) = &self.attempt
                {
                    let answer = std::mem::take(&mut self.secret);
                    if attempt.send.try_send(answer).is_ok() {
                        self.prompt = None;
                        self.status = "Verifying…".into();
                    }
                }
            }
            Message::Key(key) => {
                if matches!(key, Key::Enter) {
                    if self.mode == Mode::Lock && self.attempt.is_none() {
                        self.begin();
                    } else {
                        self.update(Message::Submit, e);
                    }
                } else if self.input_enabled() {
                    match key {
                        Key::Text(mut text) => {
                            if self.mode == Mode::Login && self.attempt.is_none() {
                                if self.identity.len() + text.len() <= 128 {
                                    self.identity.push_str(&text);
                                }
                            } else {
                                self.secret.push(&text);
                            }
                            text.zeroize();
                        }
                        Key::Backspace => {
                            if self.mode == Mode::Login && self.attempt.is_none() {
                                self.identity.pop();
                            } else {
                                self.secret.pop();
                            }
                        }
                        Key::Escape | Key::ClearInput => self.secret.clear(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        e.redraw(if self.mode == Mode::Lock {
            "lock"
        } else {
            "greeter"
        });
    }
}
struct Conversation {
    out: Emitter<Message>,
    receive: Arc<Mutex<mpsc::Receiver<Secret>>>,
    cancel: Cancellation,
}
impl AuthConversation for Conversation {
    fn prompt(&mut self, prompt: AuthPrompt) -> domain::Result<Secret> {
        let informational = matches!(prompt.kind, PromptKind::Information | PromptKind::Error);
        self.out.send(Message::Prompt(prompt));
        if informational {
            return Ok(Secret::default());
        }
        loop {
            if self.cancel.cancelled() {
                return Err(DomainError::Failed("Authentication cancelled".into()));
            }
            match self
                .receive
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_millis(100))
            {
                Ok(value) => return Ok(value),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(DomainError::Failed("Authentication cancelled".into())),
            }
        }
    }
}
impl api::Application for SessionScreen {
    fn name(&self) -> &'static str {
        if self.mode == Mode::Lock {
            "lucent-lock"
        } else {
            "lucent-greeter"
        }
    }
    fn fonts(&self) -> Vec<&'static [u8]> {
        vec![include_bytes!("../../lucent-desktop/assets/LucentSans.ttf")]
    }
    fn surfaces(&self) -> Vec<SurfaceSpec> {
        vec![SurfaceSpec {
            id: "greeter",
            layer: Layer::Overlay,
            anchor: Anchor::Fill,
            width: 0,
            height: 0,
            exclusive_zone: -1,
            keyboard: Keyboard::Exclusive,
            visible: true,
            capture_all: true,
        }]
    }
    fn init(&mut self, effects: &mut Effects<Message>) {
        if let Some(assets) = self.assets.clone() {
            // Secure surfaces and authentication start before image decoding.
            effects.task(move || Message::Wallpaper(assets.wallpaper()));
        }
        if self.mode == Mode::Lock {
            self.begin();
        }
    }
    fn subscriptions(&self) -> Vec<Subscription<Message>> {
        let Some(attempt) = &self.attempt else {
            return vec![];
        };
        let receive = attempt.receive.clone();
        let identity = self.identity.clone();
        let backend = self.backend.clone();
        vec![Subscription::stream(
            "authentication",
            move |out, cancel| {
                let result = backend.authenticate(
                    &identity,
                    &mut Conversation {
                        out: out.clone(),
                        receive,
                        cancel,
                    },
                );
                out.send(Message::Finished(result));
            },
        )]
    }
    fn event(&self, event: Event) -> Option<Message> {
        match event {
            Event::Key { key, .. } => Some(Message::Key(key)),
            _ => None,
        }
    }
}
impl lucent_wayland::LockApplication for SessionScreen {
    fn authenticated(&self) -> bool {
        self.mode == Mode::Lock && self.verified
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Deny;
    impl AuthenticationPort for Deny {
        fn authenticate(&self, _: &str, _: &mut dyn AuthConversation) -> domain::Result<()> {
            Err(DomainError::Failed("denied".into()))
        }
    }
    #[test]
    fn wallpaper_covers_each_output_without_stretching_and_fallback_is_opaque() {
        use lucent_ui::{Interaction, Layout, Paint};
        let mut app = SessionScreen::new(Mode::Lock, "fixture".into(), Arc::new(Deny));
        let fonts = Arc::new(vec![
            fontdue::Font::from_bytes(api::Application::fonts(&app)[0], Default::default())
                .unwrap(),
        ]);
        let layout = Layout::new(fonts);
        for (width, height) in [(1920., 1080.), (1080., 1920.)] {
            let cx = ViewContext {
                surface: "lock",
                width,
                height,
                now: 0.,
            };
            assert_eq!(app.view(&cx).style.background.3, 1.);
            app.update(
                Message::Wallpaper(Some(Arc::new(ImageData {
                    key: "wallpaper-fixture".into(),
                    width: 4,
                    height: 2,
                    rgba: vec![255; 32],
                }))),
                &mut Effects::default(),
            );
            let scene = layout.build(&app.view(&cx), width, height, &Interaction::default(), 0.);
            let (rect, clip) = scene
                .paint
                .iter()
                .find_map(|p| match p {
                    Paint::Image { rect, clip, .. } => Some((rect, clip)),
                    _ => None,
                })
                .expect("wallpaper must be painted");
            assert!(rect.w >= width && rect.h >= height);
            assert_eq!(rect.w / rect.h, 2.);
            assert_eq!(rect.x + rect.w / 2., width / 2.);
            assert_eq!(rect.y + rect.h / 2., height / 2.);
            assert_eq!(*clip, Rect::new(0., 0., width, height));
            app.update(Message::Wallpaper(None), &mut Effects::default());
        }
    }
    #[test]
    fn secret_never_enters_the_view_and_failed_authentication_cannot_unlock() {
        use lucent_wayland::LockApplication;
        let mut app = SessionScreen::new(Mode::Lock, "fixture".into(), Arc::new(Deny));
        app.begin();
        app.update(
            Message::Prompt(AuthPrompt {
                kind: PromptKind::Secret,
                text: "Password".into(),
            }),
            &mut Effects::default(),
        );
        app.update(
            Message::Key(Key::Text("test-secret-not-a-credential".into())),
            &mut Effects::default(),
        );
        let tree = app.view(&ViewContext {
            surface: "lock",
            width: 1920.,
            height: 1080.,
            now: 0.,
        });
        fn inspect(e: &Element<Message>) {
            match &e.kind {
                Kind::Text(t) => assert!(!t.contains("test-secret")),
                Kind::Input { .. } => panic!("secret cannot be an ordinary input"),
                _ => {}
            }
            for c in &e.children {
                inspect(c);
            }
        }
        inspect(&tree);
        assert!(!app.authenticated());
        app.update(
            Message::Finished(Err(DomainError::Failed("denied".into()))),
            &mut Effects::default(),
        );
        assert!(!app.authenticated());
        assert!(app.secret.is_empty());
        app.update(Message::Key(Key::Escape), &mut Effects::default());
        assert!(!app.authenticated());
        app.update(Message::Finished(Ok(())), &mut Effects::default());
        assert!(app.authenticated());
    }
}
