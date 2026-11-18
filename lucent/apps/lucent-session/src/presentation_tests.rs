//! Public presentation tests use owned authentication fixtures and the real SDK
//! input/layout path. They do not start PAM, greetd, polkit or a native surface.
use super::*;
use lucent_ui::{Hit, Interaction, Layout, Paint, Scene};

struct Deny;
impl AuthenticationPort for Deny {
    fn authenticate(&self, _: &str, _: &mut dyn AuthConversation) -> domain::Result<()> {
        Err(DomainError::Failed("owned fixture denied".into()))
    }
}

fn screen(mode: Mode) -> SessionScreen {
    SessionScreen::new(mode, "fixture-user".into(), Arc::new(Deny))
        .with_authorization("Install updates".into())
}

fn prompt(app: &mut SessionScreen, kind: PromptKind, text: impl Into<String>) {
    app.update(
        Message::Prompt(AuthPrompt {
            kind,
            text: text.into(),
        }),
        &mut Effects::default(),
    );
}

fn ready(mode: Mode) -> SessionScreen {
    let mut app = screen(mode);
    if mode != Mode::Login {
        app.begin();
        prompt(&mut app, PromptKind::Secret, "Password");
    }
    app
}

struct Ui {
    layout: Layout,
    cx: ViewContext,
    interaction: Interaction<Message>,
}
impl Ui {
    fn new(app: &SessionScreen, width: f32, height: f32, density: Density) -> Self {
        let fonts = api::Application::fonts(app)
            .into_iter()
            .map(|bytes| lucent_ui::text::Font::from_bytes(bytes, Default::default()).unwrap())
            .collect();
        Self {
            layout: Layout::new(Arc::new(fonts)),
            cx: ViewContext {
                surface: app.surface_id(),
                instance: None,
                width,
                height,
                now: 0.,
                density,
            },
            interaction: Interaction::default(),
        }
    }

    fn scene(&mut self, app: &SessionScreen) -> Scene<Message> {
        let tree = app.view(&self.cx);
        let scene = self
            .layout
            .build(&tree, self.cx.width, self.cx.height, &self.interaction, 0.);
        self.interaction.synchronize(&scene);
        self.layout
            .build(&tree, self.cx.width, self.cx.height, &self.interaction, 0.)
    }

    fn key(&mut self, app: &mut SessionScreen, key: Key) -> Effects<Message> {
        let scene = self.scene(app);
        let messages = match api::Application::event(
            app,
            Event::Key {
                surface: app.surface_id(),
                key: key.clone(),
            },
        ) {
            Some(message) => vec![message],
            None => self.interaction.key(&scene, &key),
        };
        let mut effects = Effects::default();
        for message in messages {
            app.update(message, &mut effects);
        }
        self.scene(app);
        effects
    }

    fn focus(&mut self, app: &mut SessionScreen, id: &str) {
        self.scene(app);
        for _ in 0..12 {
            if self.interaction.focus.as_deref() == Some(id) {
                return;
            }
            assert!(!self.key(app, Key::Tab).exit);
        }
        panic!("Tab did not reach {id}");
    }

    fn click(&mut self, app: &mut SessionScreen, id: &str) -> Effects<Message> {
        let scene = self.scene(app);
        let target = hit(&scene, id);
        let x = target.rect.x + target.rect.w / 2.;
        let y = target.rect.y + target.rect.h / 2.;
        assert!(
            target.contains(x, y),
            "{id} must be reachable at its center"
        );
        assert!(self.interaction.press(&scene, x, y).is_none());
        let message = self
            .interaction
            .release(x, y)
            .expect("the actual button releases its action");
        let mut effects = Effects::default();
        app.update(message, &mut effects);
        self.scene(app);
        effects
    }
}

fn hit<'a>(scene: &'a Scene<Message>, id: &str) -> &'a Hit<Message> {
    scene
        .hits
        .iter()
        .find(|hit| hit.id == id)
        .unwrap_or_else(|| panic!("missing hit {id}"))
}

fn public_id(app: &SessionScreen) -> String {
    format!("authentication-public-{}", app.public_scroll_epoch)
}

fn card(scene: &Scene<Message>) -> Rect {
    // Button and panel radii share a token value. The enclosing card is the
    // largest rounded background; smaller action backgrounds are its children.
    scene
        .paint
        .iter()
        .filter_map(|paint| match paint {
            Paint::Shape {
                rect,
                radius: painted_radius,
                ..
            } if *painted_radius == radius::PANEL => Some(*rect),
            _ => None,
        })
        .max_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h)))
        .expect("the card has a painted background")
}

fn assert_inside(inner: Rect, outer: Rect) {
    let epsilon = 0.02;
    assert!(inner.w > 0. && inner.h > 0., "empty rectangle: {inner:?}");
    assert!(
        inner.x + epsilon >= outer.x
            && inner.y + epsilon >= outer.y
            && inner.x + inner.w <= outer.x + outer.w + epsilon
            && inner.y + inner.h <= outer.y + outer.h + epsilon,
        "{inner:?} is not inside {outer:?}"
    );
}

fn assert_text_visible(scene: &Scene<Message>, expected: &str) {
    let (rect, clip) = scene
        .paint
        .iter()
        .find_map(|paint| match paint {
            Paint::Text {
                text, rect, clip, ..
            } if text == expected => Some((*rect, clip)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing complete text {expected:?}"));
    assert_inside(rect, clip.bounds);
    assert!(clip.contains(rect.x + rect.w / 2., rect.y + rect.h / 2.));
}

fn lines(count: usize, prefix: &str) -> String {
    (1..=count)
        .map(|index| format!("{prefix}-{index:03}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn compact_screens_initially_paint_meaningful_copy_and_complete_controls() {
    for (width, height) in [(320., 240.), (390., 320.)] {
        for mode in [Mode::Login, Mode::Lock, Mode::Authorization] {
            let app = ready(mode);
            let mut ui = Ui::new(&app, width, height, Density::ONE);
            let scene = ui.scene(&app);
            let bounds = card(&scene);
            assert_inside(bounds, Rect::new(0., 0., width, height));
            assert!((bounds.x - (width - bounds.w) / 2.).abs() < 0.02);
            assert!((bounds.y - (height - bounds.h) / 2.).abs() < 0.02);
            assert!(bounds.h <= height - space::SM * 2. + 0.02);
            let public = hit(&scene, &public_id(&app)).scroll.as_ref().unwrap();
            let public_limit = height
                - space::SM * 4.
                - space::MD * 2.
                - token::INPUT_HEIGHT
                - token::ACTION_HEIGHT;
            assert!(public.viewport.h > 0. && public.viewport.h <= public_limit + 0.02);
            assert_eq!(public.offset, 0.);
            let title = match mode {
                Mode::Login => "Welcome to Lucent",
                Mode::Lock => "Welcome back",
                Mode::Authorization => "Permission required",
            };
            assert_text_visible(&scene, title);
            if mode == Mode::Authorization {
                assert_text_visible(&scene, "Install updates");
            }
            assert_text_visible(
                &scene,
                if mode == Mode::Login {
                    "Your desktop, ready when you are"
                } else {
                    "fixture-user"
                },
            );
            assert_text_visible(
                &scene,
                if mode == Mode::Login {
                    "Username"
                } else {
                    "Password"
                },
            );
            let input = hit(&scene, "authentication-input");
            assert_eq!(input.rect.h, token::INPUT_HEIGHT);
            assert_inside(input.rect, bounds);
            assert_inside(input.rect, input.clip.bounds);
            assert!(input.scroll_ancestors.is_empty());
            assert!(input.rect.y >= public.viewport.y + public.viewport.h);
            for id in ["authentication-submit", "authorization-cancel"] {
                if id == "authorization-cancel" && mode != Mode::Authorization {
                    continue;
                }
                let action = hit(&scene, id);
                assert_eq!(action.rect.h, token::ACTION_HEIGHT);
                assert_inside(action.rect, bounds);
                assert_inside(action.rect, action.clip.bounds);
                assert!(action.scroll_ancestors.is_empty());
                assert!(action.rect.y >= input.rect.y + input.rect.h);
            }
        }
    }
}

#[test]
fn short_and_capped_cards_center_their_actual_natural_height() {
    let mut app = ready(Mode::Authorization);
    let mut ui = Ui::new(&app, 1280., 800., Density::ONE);
    let short = card(&ui.scene(&app));
    assert!(short.h < token::HEIGHT);
    assert!((short.y - (800. - short.h) / 2.).abs() < 0.02);
    app.authorization = lines(60, "AUTHORIZATION");
    let long = card(&ui.scene(&app));
    assert!(long.h > short.h);
    assert!((long.h - token::HEIGHT).abs() < 0.02);
    assert!((long.y - (800. - long.h) / 2.).abs() < 0.02);
    assert_eq!(short.w, token::WIDTH);
    assert_eq!(long.w, short.w);
}

#[test]
fn minimum_screen_pointer_actions_submit_cancel_and_retry() {
    for mode in [Mode::Login, Mode::Lock, Mode::Authorization] {
        let mut app = ready(mode);
        let mut ui = Ui::new(&app, 320., 240., Density::ONE);
        if mode != Mode::Login {
            ui.key(&mut app, Key::Text("owned-response".into()));
        }
        assert!(!ui.click(&mut app, "authentication-submit").exit);
        assert!(app.attempt.is_some());
        assert!(!app.input_enabled());
        assert!(app.secret.is_empty());
        if mode == Mode::Login {
            assert_eq!(app.identity, "fixture-user");
            assert_eq!(app.status, "Connecting…");
        } else {
            let answer = app
                .attempt
                .as_ref()
                .unwrap()
                .receive
                .lock()
                .unwrap()
                .try_recv()
                .unwrap();
            assert_eq!(answer.expose(), "owned-response");
            assert_eq!(app.status, "Verifying…");
        }
    }
    let mut app = ready(Mode::Authorization);
    let mut ui = Ui::new(&app, 320., 240., Density::ONE);
    ui.key(&mut app, Key::Text("owned-response".into()));
    assert!(ui.click(&mut app, "authorization-cancel").exit);
    assert!(app.secret.is_empty());
    assert!(!app.verified);

    let mut app = ready(Mode::Lock);
    app.update(
        Message::Finished(Err(DomainError::Failed("Owned failure".into()))),
        &mut Effects::default(),
    );
    let mut ui = Ui::new(&app, 320., 240., Density::ONE);
    assert!(!ui.click(&mut app, "authentication-retry").exit);
    assert!(app.attempt.is_some());
    assert_eq!(app.status, "Connecting…");
    assert!(!app.verified);
}

#[test]
fn minimum_screen_keyboard_submits_each_mode_after_reading_a_long_prompt() {
    for mode in [Mode::Login, Mode::Lock, Mode::Authorization] {
        let mut app = ready(mode);
        let mut ui = Ui::new(&app, 320., 240., Density::ONE);
        if mode == Mode::Login {
            assert!(!ui.key(&mut app, Key::Enter).exit);
            assert_eq!(app.status, "Connecting…");
            assert!(app.attempt.is_some());
        }
        prompt(
            &mut app,
            PromptKind::Secret,
            format!("{}\nPROMPT-END", lines(60, "PROMPT")),
        );
        let public = public_id(&app);
        ui.focus(&mut app, &public);
        ui.key(&mut app, Key::End);
        assert_text_visible(&ui.scene(&app), "PROMPT-END");
        ui.focus(&mut app, "authentication-input");
        ui.key(&mut app, Key::Text("owned-keyboard-response".into()));
        assert!(!ui.key(&mut app, Key::Enter).exit);
        assert!(!app.input_enabled());
        assert!(app.secret.is_empty());
        assert_eq!(app.status, "Verifying…");
        let answer = app
            .attempt
            .as_ref()
            .unwrap()
            .receive
            .lock()
            .unwrap()
            .try_recv()
            .unwrap();
        assert_eq!(answer.expose(), "owned-keyboard-response");
        assert!(!app.verified);
    }
}

#[test]
fn global_escape_and_idle_lock_enter_keep_their_authentication_semantics() {
    for mode in [Mode::Login, Mode::Lock, Mode::Authorization] {
        let mut app = ready(mode);
        app.status = lines(60, "PUBLIC-STATUS");
        let mut ui = Ui::new(&app, 320., 240., Density::ONE);
        ui.key(&mut app, Key::Text("owned-response".into()));
        let public = public_id(&app);
        ui.focus(&mut app, &public);
        ui.key(&mut app, Key::End);
        assert!(app.public_scroll > 0.);
        let effects = ui.key(&mut app, Key::Escape);
        assert_eq!(effects.exit, mode == Mode::Authorization);
        assert!(app.secret.is_empty());
        assert!(!app.verified);
        if mode == Mode::Login {
            assert!(app.identity.is_empty());
        } else {
            assert_eq!(app.identity, "fixture-user");
        }
    }

    let mut app = ready(Mode::Lock);
    app.update(
        Message::Finished(Err(DomainError::Failed("Owned failure".into()))),
        &mut Effects::default(),
    );
    let mut ui = Ui::new(&app, 320., 240., Density::ONE);
    let public = public_id(&app);
    ui.focus(&mut app, &public);
    assert!(app.attempt.is_none());
    assert!(!ui.key(&mut app, Key::Enter).exit);
    assert!(app.attempt.is_some());
    assert_eq!(app.status, "Connecting…");
    assert!(!app.verified);
}

#[test]
fn public_end_reaches_each_long_copy_tail_without_one_tall_text_raster() {
    for block in 0..4 {
        let mut app = ready(Mode::Authorization);
        let content = format!("{}\nPUBLIC-END", lines(60, "OWNED"));
        match block {
            0 => app.identity = content,
            1 => app.authorization = content,
            2 => prompt(&mut app, PromptKind::Secret, content),
            _ => prompt(&mut app, PromptKind::Information, content),
        }
        let density = Density::integer(2).unwrap();
        let mut ui = Ui::new(&app, 320., 240., density);
        let initial = ui.scene(&app);
        let rows = initial
            .paint
            .iter()
            .filter_map(|paint| match paint {
                Paint::Text { text, rect, .. }
                    if text.starts_with("OWNED-") || text == "PUBLIC-END" =>
                {
                    assert!(!text.contains('\n'));
                    assert!(rect.h * density.factor() < 1024.);
                    Some(*rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 61);
        assert!((rows.last().unwrap().y - rows[0].y) * density.factor() > 1024.);
        let public = public_id(&app);
        ui.focus(&mut app, &public);
        assert!(!ui.key(&mut app, Key::End).exit);
        let end = ui.scene(&app);
        let scroll = hit(&end, &public).scroll.as_ref().unwrap();
        assert!(scroll.maximum > 0.);
        assert!((app.public_scroll - scroll.maximum).abs() < 0.02);
        assert_text_visible(&end, "PUBLIC-END");
        assert_inside(hit(&end, "authentication-input").rect, card(&end));
        assert_inside(hit(&end, "authentication-submit").rect, card(&end));
        ui.key(&mut app, Key::Home);
        assert_eq!(app.public_scroll, 0.);
        ui.key(&mut app, Key::PageDown);
        assert!(app.public_scroll > 0.);
    }
}

#[test]
fn public_copy_caps_rendered_rows_and_keeps_the_final_ellipsis_reachable() {
    let mut app = ready(Mode::Authorization);
    prompt(&mut app, PromptKind::Secret, lines(129, "BOUNDED"));
    let mut ui = Ui::new(&app, 320., 240., Density::ONE);
    let public = public_id(&app);
    ui.focus(&mut app, &public);
    ui.key(&mut app, Key::End);
    let scene = ui.scene(&app);
    let rows = scene
        .paint
        .iter()
        .filter_map(|paint| match paint {
            Paint::Text { text, .. } if text.starts_with("BOUNDED-") => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 128);
    assert_eq!(rows[0], "BOUNDED-001");
    assert_eq!(rows[127], "BOUNDED-128…");
    assert_text_visible(&scene, "BOUNDED-128…");
    assert!(scene.hits.iter().all(|hit| hit.parts.is_none()));
}

#[test]
fn identity_end_edits_the_field_without_scrolling_public_copy() {
    let mut app = ready(Mode::Login);
    let mut ui = Ui::new(&app, 320., 240., Density::ONE);
    ui.scene(&app);
    assert_eq!(
        ui.interaction.focus.as_deref(),
        Some("authentication-input")
    );
    ui.key(&mut app, Key::Home);
    ui.key(&mut app, Key::Text("prefix-".into()));
    ui.key(&mut app, Key::End);
    ui.key(&mut app, Key::Text("-suffix".into()));
    assert_eq!(app.identity, "prefix-fixture-user-suffix");
    assert_eq!(app.public_scroll, 0.);
    assert_eq!(
        ui.interaction.focus.as_deref(),
        Some("authentication-input")
    );
}

#[test]
fn disabled_authorization_fields_retire_focus_and_leave_cancel_keyboard_reachable() {
    for verifying in [false, true] {
        for activation in [Key::Enter, Key::Text(" ".into())] {
            let mut app = screen(Mode::Authorization);
            app.begin();
            let mut ui = Ui::new(&app, 320., 240., Density::ONE);
            if verifying {
                prompt(&mut app, PromptKind::Secret, "Password");
                ui.key(&mut app, Key::Text("owned-response".into()));
                assert_eq!(
                    ui.interaction.focus.as_deref(),
                    Some("authentication-input")
                );
                ui.key(&mut app, Key::Enter);
                assert_eq!(app.status, "Verifying…");
            }
            let scene = ui.scene(&app);
            assert!(!app.input_enabled());
            assert!(
                !scene
                    .hits
                    .iter()
                    .any(|hit| hit.id == "authentication-input")
            );
            assert_ne!(
                ui.interaction.focus.as_deref(),
                Some("authentication-input")
            );
            assert_text_visible(
                &scene,
                if verifying {
                    "Verifying…"
                } else {
                    "Connecting…"
                },
            );
            assert_inside(hit(&scene, "authorization-cancel").rect, card(&scene));
            assert!(
                api::Application::event(
                    &app,
                    Event::Key {
                        surface: app.surface_id(),
                        key: Key::Tab,
                    },
                )
                .is_none()
            );
            ui.key(&mut app, Key::Text("ignored-disabled-input".into()));
            assert_eq!(app.identity, "fixture-user");
            assert!(app.secret.is_empty());
            ui.focus(&mut app, "authorization-cancel");
            ui.key(&mut app, Key::BackTab);
            assert_eq!(
                ui.interaction.focus.as_deref(),
                Some(public_id(&app).as_str())
            );
            ui.key(&mut app, Key::Tab);
            assert_eq!(
                ui.interaction.focus.as_deref(),
                Some("authorization-cancel")
            );
            assert!(ui.key(&mut app, activation).exit);
            assert!(app.secret.is_empty());
            assert!(!app.verified);
        }
    }
}

#[test]
fn disabled_login_and_lock_enter_preserve_the_current_attempt() {
    for mode in [Mode::Login, Mode::Lock] {
        for verifying in [false, true] {
            let mut app = screen(mode);
            app.begin();
            let mut ui = Ui::new(&app, 320., 240., Density::ONE);
            if verifying {
                prompt(&mut app, PromptKind::Secret, "Password");
                ui.key(&mut app, Key::Text("owned-response".into()));
                ui.key(&mut app, Key::Enter);
            }
            let receive = app.attempt.as_ref().unwrap().receive.clone();
            let status = app.status.clone();
            let epoch = app.public_scroll_epoch;
            assert!(!app.input_enabled());
            let public = public_id(&app);
            ui.focus(&mut app, &public);
            assert!(
                api::Application::event(
                    &app,
                    Event::Key {
                        surface: app.surface_id(),
                        key: Key::Enter,
                    },
                )
                .is_none()
            );
            let effects = ui.key(&mut app, Key::Enter);
            assert!(!effects.exit);
            assert!(effects.tasks.is_empty());
            assert!(Arc::ptr_eq(
                &receive,
                &app.attempt.as_ref().unwrap().receive
            ));
            assert_eq!(app.status, status);
            assert_eq!(app.public_scroll_epoch, epoch);
            assert!(app.prompt.is_none());
            assert!(app.secret.is_empty());
        }
    }
}

#[test]
fn new_prompt_restores_retired_field_focus_without_stealing_valid_cancel_focus() {
    let mut app = ready(Mode::Authorization);
    let mut ui = Ui::new(&app, 320., 240., Density::ONE);
    ui.scene(&app);
    assert_eq!(
        ui.interaction.focus.as_deref(),
        Some("authentication-input")
    );
    ui.key(&mut app, Key::Enter);
    assert!(!app.input_enabled());
    assert!(ui.interaction.focus.is_none());
    prompt(&mut app, PromptKind::Secret, "Another password");
    ui.scene(&app);
    assert_eq!(
        ui.interaction.focus.as_deref(),
        Some("authentication-input")
    );
    ui.key(&mut app, Key::Text("new-response".into()));
    assert_eq!(app.secret.expose(), "new-response");

    let mut app = screen(Mode::Authorization);
    app.begin();
    let mut ui = Ui::new(&app, 320., 240., Density::ONE);
    ui.focus(&mut app, "authorization-cancel");
    prompt(&mut app, PromptKind::Secret, "Password");
    ui.scene(&app);
    assert_eq!(
        ui.interaction.focus.as_deref(),
        Some("authorization-cancel")
    );
}

#[test]
fn fresh_prompt_retires_same_geometry_scroll_capture_and_restores_field_focus() {
    let mut app = ready(Mode::Authorization);
    prompt(&mut app, PromptKind::Secret, lines(60, "FIRST"));
    let mut ui = Ui::new(&app, 320., 240., Density::ONE);
    let old_id = public_id(&app);
    ui.focus(&mut app, &old_id);
    let scene = ui.scene(&app);
    let scroll = hit(&scene, &old_id).scroll.as_ref().unwrap().clone();
    let queued = ui
        .interaction
        .scroll(
            &scene,
            scroll.viewport.x + scroll.viewport.w / 2.,
            scroll.viewport.y + scroll.viewport.h / 2.,
            1.,
        )
        .expect("wheel is consumed by the public viewport");
    assert!(!queued.is_empty());
    let x = scroll.thumb.x + scroll.thumb.w / 2.;
    let y = scroll.thumb.y + scroll.thumb.h / 2.;
    assert!(hit(&scene, &old_id).contains(x, y));
    if let Some(message) = ui.interaction.press(&scene, x, y) {
        app.update(message, &mut Effects::default());
    }
    prompt(&mut app, PromptKind::Secret, lines(60, "OTHER"));
    let current = ui.scene(&app);
    let new_id = public_id(&app);
    assert_ne!(old_id, new_id);
    let new_scroll = hit(&current, &new_id).scroll.as_ref().unwrap();
    assert_eq!(scroll.viewport, new_scroll.viewport);
    assert_eq!(scroll.maximum, new_scroll.maximum);
    assert_eq!(scroll.thumb.h, new_scroll.thumb.h);
    assert_eq!(
        ui.interaction.focus.as_deref(),
        Some("authentication-input")
    );
    assert!(ui.interaction.motion(&current, x, y + 20., 0.).is_empty());
    assert_eq!(app.public_scroll, 0.);
    assert!(ui.interaction.release(x, y + 20.).is_none());
    for message in queued {
        app.update(message, &mut Effects::default());
    }
    assert_eq!(app.public_scroll, 0.);
    ui.focus(&mut app, &new_id);
    ui.key(&mut app, Key::End);
    assert!(app.public_scroll > 0., "fresh controls still work");
}

#[test]
fn all_public_content_transitions_retire_queued_scroll_and_reset_the_offset() {
    for transition in 0..7 {
        let mut app = ready(Mode::Authorization);
        app.authorization = lines(60, "REASON");
        let mut ui = Ui::new(&app, 320., 240., Density::ONE);
        let old_id = public_id(&app);
        ui.focus(&mut app, &old_id);
        ui.key(&mut app, Key::End);
        assert!(app.public_scroll > 0.);
        let scene = ui.scene(&app);
        let scroll = hit(&scene, &old_id).scroll.as_ref().unwrap();
        let queued = (scroll.callback)(ScrollEvent { offset: 12. });
        let old_epoch = app.public_scroll_epoch;
        match transition {
            0 => app.begin(),
            1 => prompt(&mut app, PromptKind::Secret, "New password"),
            2 => prompt(&mut app, PromptKind::Visible, "Public response"),
            3 => prompt(&mut app, PromptKind::Information, "New information"),
            4 => prompt(&mut app, PromptKind::Error, "New prompt error"),
            5 => app.update(
                Message::Finished(Err(DomainError::Failed("Owned failure".into()))),
                &mut Effects::default(),
            ),
            _ => app.update(Message::Submit, &mut Effects::default()),
        }
        assert_ne!(
            app.public_scroll_epoch, old_epoch,
            "transition {transition}"
        );
        assert_eq!(app.public_scroll, 0., "transition {transition}");
        app.update(queued, &mut Effects::default());
        assert_eq!(app.public_scroll, 0., "queued transition {transition}");
    }
}

#[test]
fn typing_and_wallpaper_delivery_preserve_current_public_scroll_authority() {
    let mut app = ready(Mode::Authorization);
    app.authorization = lines(60, "REASON");
    let mut ui = Ui::new(&app, 320., 240., Density::ONE);
    let id = public_id(&app);
    ui.focus(&mut app, &id);
    ui.key(&mut app, Key::End);
    let old_offset = app.public_scroll;
    let old_epoch = app.public_scroll_epoch;
    let scene = ui.scene(&app);
    let accepted =
        (hit(&scene, &id).scroll.as_ref().unwrap().callback)(ScrollEvent { offset: 12. });
    ui.focus(&mut app, "authentication-input");
    ui.key(&mut app, Key::Text("owned-response".into()));
    app.update(Message::Wallpaper(None), &mut Effects::default());
    assert_eq!(app.public_scroll_epoch, old_epoch);
    assert_eq!(app.public_scroll, old_offset);
    app.update(accepted, &mut Effects::default());
    assert_eq!(app.public_scroll, 12.);
    for offset in [-1., f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        app.update(
            Message::PublicScroll {
                epoch: old_epoch,
                offset,
            },
            &mut Effects::default(),
        );
        assert_eq!(app.public_scroll, 12.);
    }
}

#[test]
fn masked_responses_stay_out_of_public_rich_text_paints_and_hit_values() {
    const SECRET: &str = "owned-masked-secret-fixture-ø";
    fn inspect(element: &Element<Message>) {
        match &element.kind {
            Kind::Text(text)
            | Kind::Input { value: text, .. }
            | Kind::TextInput { value: text, .. } => {
                assert!(!text.contains(SECRET));
            }
            Kind::RichText(spans) => {
                for span in spans {
                    assert!(!span.text.contains(SECRET));
                    assert!(span.link.is_none());
                }
            }
            _ => {}
        }
        if element.id == "authentication-input" {
            let Kind::MaskedInput { length, .. } = &element.kind else {
                panic!("secret responses must use the count-only field");
            };
            assert_eq!(*length, SECRET.chars().count());
        }
        for child in &element.children {
            inspect(child);
        }
    }
    for mode in [Mode::Login, Mode::Lock, Mode::Authorization] {
        let mut app = screen(mode);
        app.begin();
        prompt(&mut app, PromptKind::Secret, "Password");
        let mut ui = Ui::new(&app, 320., 240., Density::ONE);
        ui.key(&mut app, Key::Text(SECRET.into()));
        assert_eq!(app.secret.expose(), SECRET);
        inspect(&app.view(&ui.cx));
        let scene = ui.scene(&app);
        for paint in &scene.paint {
            if let Paint::Text { text, .. } = paint {
                assert!(!text.contains(SECRET));
            }
        }
        for hit in &scene.hits {
            assert!(!hit.value.contains(SECRET));
            if let Some(text) = &hit.text {
                assert!(!text.value.contains(SECRET));
            }
        }
        assert!(!api::Application::input_method_enabled(&app));
        assert!(!api::Application::clipboard_enabled(&app));
        assert!(!api::Application::keyboard_focus_requests_enabled(&app));
    }
}

#[test]
fn visible_prompts_keep_the_backend_requested_echoed_field() {
    let mut app = screen(Mode::Authorization);
    app.begin();
    prompt(&mut app, PromptKind::Visible, "Public response");
    let mut ui = Ui::new(&app, 320., 240., Density::ONE);
    ui.key(&mut app, Key::Text("owned-visible-response".into()));
    let scene = ui.scene(&app);
    let field = hit(&scene, "authentication-input");
    assert!(field.input.is_some());
    assert_eq!(field.value, "owned-visible-response");
    assert_eq!(app.secret.expose(), "owned-visible-response");
}
