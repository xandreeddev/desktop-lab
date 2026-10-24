use super::*;
use lucent_domain::{NotificationAction, NotificationHistoryStatus, Result, StopSignal};
use std::sync::Mutex;
#[derive(Default)]
struct Adapter {
    calls: Mutex<Vec<NotificationTarget>>,
    links: Mutex<Vec<String>>,
    link_contexts: Mutex<Vec<Option<String>>>,
    fail_link: Mutex<bool>,
}
impl lucent_domain::UriPort for Adapter {
    fn open(
        &self,
        target: &lucent_domain::ExternalUri,
        context: lucent_domain::ApplicationLaunchContext,
    ) -> Result<()> {
        self.links.lock().unwrap().push(target.value().into());
        self.link_contexts
            .lock()
            .unwrap()
            .push(context.activation_token().map(str::to_owned));
        if *self.fail_link.lock().unwrap() {
            return Err(lucent_domain::DomainError::Failed(
                "Controlled link failure".into(),
            ));
        }
        Ok(())
    }
}
impl NotificationPort for Adapter {
    fn watch(&self, _: &mut dyn FnMut(Result<NotificationSnapshot>), _: &dyn StopSignal) {}
    fn dismiss(&self, target: NotificationTarget) -> Result<()> {
        self.calls.lock().unwrap().push(target);
        Ok(())
    }
    fn invoke(&self, target: NotificationTarget, _: &str) -> Result<()> {
        self.calls.lock().unwrap().push(target);
        Ok(())
    }
    fn set_do_not_disturb(&self, _: bool) -> Result<()> {
        Ok(())
    }
    fn clear_history(&self) -> Result<()> {
        Ok(())
    }
    fn retry_history(&self) -> Result<()> {
        Ok(())
    }
}
fn note(id: u32, revision: u64, app: &str) -> Notification {
    Notification {
        id,
        revision,
        received_unix_ms: revision,
        desktop_entry: None,
        app: app.into(),
        summary: "Fixture event".into(),
        body: "Fixture body".into(),
        image: None,
        actions: vec![NotificationAction {
            id: "open".into(),
            label: "Open".into(),
        }],
        critical: false,
        resident: false,
        transient: false,
        timeout_ms: 0,
    }
}
fn center() -> (Center, Arc<Adapter>) {
    let adapter = Arc::new(Adapter::default());
    (
        Center::new(
            adapter.clone(),
            adapter.clone(),
            super::artwork::tests::adapter(),
        ),
        adapter,
    )
}
#[test]
fn history_groups_active_and_saved_entries_with_stable_selection_and_bounded_pages() {
    let (mut center, _) = center();
    let mut effects = Effects::default();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![note(1, 10, "Editor")],
            history: vec![note(2, 9, "Browser"), note(3, 8, "Editor")],
            ..Default::default()
        })),
        &mut effects,
    );
    let groups = center.groups();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].label, "Editor");
    assert_eq!(groups[0].entries.len(), 2);
    let key = groups[0].key.clone();
    center.update(Message::Group(key.clone()), &mut effects);
    center.update(Message::Page(i32::MAX), &mut effects);
    assert_eq!(center.page, 1);
    center.update(Message::Page(-1), &mut effects);
    assert_eq!(center.page, 0);
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![note(1, 11, "Editor")],
            ..Default::default()
        })),
        &mut effects,
    );
    assert_eq!(center.selected_group, Some(key));
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot::default())),
        &mut effects,
    );
    assert_eq!(center.selected_group, None);
}
#[test]
fn history_reveal_reverses_continuously_and_retires_only_after_settled_submission() {
    let (mut center, _) = center();
    let mut effects = Effects::default();
    center.update(Message::Show, &mut effects);
    assert!(center.visible());
    assert!(center.animating(0.));
    assert!(!center.animating(1.));
    effects.now = 0.1;
    let before = center.reveal.value(effects.now);
    center.update(Message::Hide, &mut effects);
    assert_eq!(center.reveal.value(effects.now), before);
    assert!(center.visible());
    effects.now = 0.12;
    let before = center.reveal.value(effects.now);
    center.update(Message::Show, &mut effects);
    assert_eq!(center.reveal.value(effects.now), before);
    center.update(Message::Settled, &mut effects);
    assert!(center.history_open());
    effects.now = 1.;
    center.update(Message::Hide, &mut effects);
    assert!(center.visible());
    assert!(center.settled(3.));
    effects.now = 3.;
    center.update(Message::Settled, &mut effects);
    assert!(!center.visible());
}
#[test]
fn action_pages_retire_on_replacement_and_history_has_no_action_controls() {
    let (mut center, _) = center();
    let mut effects = Effects::default();
    let mut live = note(1, 1, "Fixture");
    live.actions.push(NotificationAction {
        id: "other".into(),
        label: "Other".into(),
    });
    let old = live.target().unwrap();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![live.clone()],
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(Message::ActionPage(old, -1), &mut effects);
    assert_eq!(center.action_pages[&old], 1);
    live.revision = 2;
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![live],
            ..Default::default()
        })),
        &mut effects,
    );
    assert!(center.action_pages.is_empty());
    center.update(Message::ActionPage(old, 1), &mut effects);
    assert!(center.action_pages.is_empty());
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            history: vec![note(1, 1, "Fixture")],
            history_status: NotificationHistoryStatus::Ready,
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(Message::Show, &mut effects);
    center.update(Message::Group("name:Fixture".into()), &mut effects);
    let tree = center.view(&ViewContext {
        surface: "notifications",
        instance: None,
        width: 390.,
        height: 580.,
        now: 1.,
        density: lucent_api::Density::ONE,
    });
    fn actions<M>(node: &Element<M>) -> bool {
        node.id.starts_with("notification-action-") || node.children.iter().any(actions)
    }
    assert!(!actions(&tree));
}
#[test]
fn current_targets_reach_only_injected_adapter_and_mapped_completion_has_no_ui_io() {
    let (mut center, adapter) = center();
    let mut effects = Effects::default();
    let live = note(1, 5, "Fixture");
    let target = live.target().unwrap();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![live],
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(Message::Invoke(target, "open".into()), &mut effects);
    assert!(adapter.calls.lock().unwrap().is_empty());
    let task = effects.tasks.pop().unwrap();
    let result = task.run();
    center.update(result, &mut effects);
    assert_eq!(*adapter.calls.lock().unwrap(), vec![target]);
}

#[test]
fn retired_or_unavailable_actions_are_not_queued_from_old_scenes() {
    let (mut center, adapter) = center();
    let mut effects = Effects::default();
    let old = note(1, 1, "Fixture").target().unwrap();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![note(1, 2, "Fixture")],
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(Message::Invoke(old, "open".into()), &mut effects);
    center.update(Message::Dismiss(old), &mut effects);
    assert!(effects.tasks.is_empty());
    assert!(adapter.calls.lock().unwrap().is_empty());
    let current = center.snapshot.active[0].target().unwrap();
    center.update(
        Message::Snapshot(Err(lucent_domain::DomainError::Unavailable(
            "fixture".into(),
        ))),
        &mut effects,
    );
    center.update(Message::Invoke(current, "open".into()), &mut effects);
    assert!(effects.tasks.is_empty());
}

#[test]
fn rich_links_use_current_content_and_history_never_invokes_the_sender() {
    let (mut center, adapter) = center();
    let mut effects = Effects::default();
    let mut live = note(1, 5, "Fixture");
    live.body = "A <b>bold</b> and <a href='https://example.test/details'>linked</a> body".into();
    let target = live.target().unwrap();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![live.clone()],
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(
        Message::OpenLink(target, "javascript:bad".into()),
        &mut effects,
    );
    assert!(effects.tasks.is_empty());
    assert!(effects.activation.is_empty());
    center.update(
        Message::OpenLink(target, "https://example.test/details".into()),
        &mut effects,
    );
    assert!(adapter.links.lock().unwrap().is_empty());
    assert!(effects.tasks.is_empty());
    let request = effects.activation.pop().unwrap();
    assert!(
        request.app_id.is_none(),
        "the default handler is not known yet"
    );
    center.update(
        request.finish(ActivationOutcome::Token(
            ActivationToken::new("fresh-link-intent".into()).unwrap(),
        )),
        &mut effects,
    );
    release_link_input(&mut center, &mut effects);
    let result = effects.tasks.pop().unwrap().run();
    center.update(result, &mut effects);
    assert_eq!(
        *adapter.links.lock().unwrap(),
        ["https://example.test/details"]
    );
    assert_eq!(
        *adapter.link_contexts.lock().unwrap(),
        [Some("fresh-link-intent".into())]
    );
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            history: vec![live],
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(
        Message::OpenLink(target, "https://example.test/details".into()),
        &mut effects,
    );
    let completion = effects
        .activation
        .pop()
        .unwrap()
        .finish(ActivationOutcome::NoUserInput);
    center.update(completion, &mut effects);
    release_link_input(&mut center, &mut effects);
    let result = effects.tasks.pop().unwrap().run();
    center.update(result, &mut effects);
    assert!(adapter.calls.lock().unwrap().is_empty());
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot::default())),
        &mut effects,
    );
    center.update(
        Message::OpenLink(target, "https://example.test/details".into()),
        &mut effects,
    );
    assert!(effects.tasks.is_empty());
    assert!(effects.activation.is_empty());
    assert_eq!(
        *adapter.link_contexts.lock().unwrap(),
        [Some("fresh-link-intent".into()), None]
    );
}

fn link_note() -> Notification {
    let mut value = note(1, 5, "Fixture");
    value.body = "<a href='https://example.test/first'>First</a> <a href='https://example.test/second'>Second</a>".into();
    value
}
fn token() -> ActivationOutcome {
    ActivationOutcome::Token(ActivationToken::new("fresh-controlled-intent".into()).unwrap())
}

fn show_link_history(center: &mut Center, effects: &mut Effects<Message>) -> NotificationTarget {
    let value = link_note();
    let target = value.target().unwrap();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            history: vec![value],
            ..Default::default()
        })),
        effects,
    );
    center.update(Message::Show, effects);
    center.update(Message::Group("name:Fixture".into()), effects);
    target
}

fn release_link_input(center: &mut Center, effects: &mut Effects<Message>) {
    let request = effects.input_release.pop().unwrap();
    center.update(
        request.finish(InputReleaseOutcome::Released).unwrap(),
        effects,
    );
}

fn prepared_link_task(
    center: &mut Center,
    effects: &mut Effects<Message>,
    target: NotificationTarget,
) -> lucent_api::ScheduledTask<Message> {
    center.update(
        Message::OpenLink(target, "https://example.test/first".into()),
        effects,
    );
    let prepared = effects.activation.pop().unwrap().finish(token());
    center.update(prepared, effects);
    release_link_input(center, effects);
    let task = effects.tasks.pop().unwrap();
    assert_eq!(task.queue, crate::work::CONTROLS);
    task
}

#[test]
fn uri_dispatch_waits_for_both_context_and_actual_release_ack_in_either_order() {
    for release_first in [false, true] {
        let (mut center, adapter) = center();
        let mut effects = Effects::default();
        let target = show_link_history(&mut center, &mut effects);
        let session = center.history_session;
        center.update(
            Message::OpenLink(target, "https://example.test/first".into()),
            &mut effects,
        );
        assert!(center.history_open() && center.yielding_input());
        let token = effects.activation.pop().unwrap().finish(token());
        let released = effects
            .input_release
            .pop()
            .unwrap()
            .finish(InputReleaseOutcome::Released)
            .unwrap();
        let (first, second) = if release_first {
            (released, token)
        } else {
            (token, released)
        };
        center.update(first.clone(), &mut effects);
        assert!(
            effects.tasks.is_empty(),
            "one completion cannot open the handler"
        );
        assert!(adapter.links.lock().unwrap().is_empty());
        center.update(second.clone(), &mut effects);
        center.update(first, &mut effects);
        center.update(second, &mut effects);
        assert_eq!(effects.tasks.len(), 1);
        assert_eq!(center.history_session, session);
        assert!(center.history_open(), "release is not dismissal");
        assert!(
            center.yielding_input(),
            "keep released until handler outcome"
        );
        center.update(effects.tasks.pop().unwrap().run(), &mut effects);
        assert!(!center.history_open());
        assert!(!center.yielding_input());
        assert_eq!(
            *adapter.links.lock().unwrap(),
            ["https://example.test/first"]
        );
    }
}

#[test]
fn release_failure_retains_current_history_and_never_invokes_uri_adapter() {
    for context_first in [false, true] {
        for outcome in [
            InputReleaseOutcome::Unavailable,
            InputReleaseOutcome::Retired,
            InputReleaseOutcome::Superseded,
            InputReleaseOutcome::Timeout,
            InputReleaseOutcome::Busy,
        ] {
            let (mut center, adapter) = center();
            let mut effects = Effects::default();
            let target = show_link_history(&mut center, &mut effects);
            let session = center.history_session;
            center.update(
                Message::OpenLink(target, "https://example.test/first".into()),
                &mut effects,
            );
            let token = effects.activation.pop().unwrap().finish(token());
            if context_first {
                center.update(token.clone(), &mut effects);
            }
            let failure = effects
                .input_release
                .pop()
                .unwrap()
                .finish(outcome)
                .unwrap();
            center.update(failure.clone(), &mut effects);
            center.update(token, &mut effects);
            center.update(failure, &mut effects);
            assert!(center.history_open());
            assert_eq!(center.history_session, session);
            assert!(center.error);
            assert!(!center.yielding_input());
            assert!(effects.tasks.is_empty());
            assert!(adapter.links.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn hide_reopen_and_new_presentation_cancel_unadmitted_handoff_without_closing_new_ui() {
    for context_first in [false, true] {
        let (mut center, adapter) = center();
        let mut effects = Effects::default();
        let target = show_link_history(&mut center, &mut effects);
        center.update(
            Message::OpenLink(target, "https://example.test/first".into()),
            &mut effects,
        );
        let activation = effects.activation.pop().unwrap().finish(token());
        let release = effects.input_release.pop().unwrap();
        if context_first {
            center.update(activation.clone(), &mut effects);
        }
        center.update(Message::Hide, &mut effects);
        center.update(Message::Show, &mut effects);
        let session = center.history_session;
        assert!(release.cancellation.cancelled());
        assert!(release.finish(InputReleaseOutcome::Released).is_none());
        center.update(activation, &mut effects);
        assert!(center.history_open());
        assert_eq!(center.history_session, session);
        assert!(!center.yielding_input());
        assert!(!center.error);
        assert!(effects.tasks.is_empty());
        assert!(adapter.links.lock().unwrap().is_empty());
    }
}

#[test]
fn changed_note_and_invalid_new_link_retire_waiting_release_and_checked_context() {
    for replace in [false, true] {
        let (mut center, adapter) = center();
        let mut effects = Effects::default();
        let target = show_link_history(&mut center, &mut effects);
        center.update(
            Message::OpenLink(target, "https://example.test/first".into()),
            &mut effects,
        );
        let activation = effects.activation.pop().unwrap().finish(token());
        center.update(activation, &mut effects);
        let release = effects.input_release.pop().unwrap();
        if replace {
            let mut changed = link_note();
            changed.revision += 1;
            center.update(
                Message::Snapshot(Ok(NotificationSnapshot {
                    history: vec![changed],
                    ..Default::default()
                })),
                &mut effects,
            );
        } else {
            center.update(
                Message::OpenLink(target, "javascript:invalid".into()),
                &mut effects,
            );
        }
        assert!(release.cancellation.cancelled());
        assert!(release.finish(InputReleaseOutcome::Released).is_none());
        assert!(effects.tasks.is_empty());
        assert!(adapter.links.lock().unwrap().is_empty());
        assert!(center.history_open());
        assert!(!center.yielding_input());
    }
}

#[test]
fn current_history_link_success_uses_normal_exit_paint_and_replay_cannot_close_new_history() {
    let (mut center, adapter) = center();
    let mut effects = Effects::default();
    let target = show_link_history(&mut center, &mut effects);
    let session = center.history_session.unwrap();
    let task = prepared_link_task(&mut center, &mut effects, target);
    assert_eq!(
        center.link_opening.as_ref().unwrap().history_session,
        Some(session)
    );
    assert!(adapter.links.lock().unwrap().is_empty());
    let done = task.run();
    effects.now = 1.;
    center.update(done.clone(), &mut effects);
    assert!(!center.history_open());
    assert!(center.history_closing());
    assert!(center.visible());
    assert_eq!(center.history_session, None);
    assert!(!center.error);
    assert_eq!(
        *adapter.links.lock().unwrap(),
        ["https://example.test/first"]
    );
    effects.now = 3.;
    center.update(Message::Settled, &mut effects);
    assert!(!center.visible());
    center.update(Message::Show, &mut effects);
    let newer = center.history_session.unwrap();
    assert_ne!(newer, session);
    center.update(done, &mut effects);
    assert!(center.history_open());
    assert_eq!(center.history_session, Some(newer));
}

#[test]
fn current_history_link_failure_retains_its_presentation_and_reports_error() {
    let (mut center, adapter) = center();
    *adapter.fail_link.lock().unwrap() = true;
    let mut effects = Effects::default();
    let target = show_link_history(&mut center, &mut effects);
    let session = center.history_session;
    let task = prepared_link_task(&mut center, &mut effects, target);
    effects.now = 1.;
    center.update(task.run(), &mut effects);
    assert!(center.history_open());
    assert!(!center.history_closing());
    assert_eq!(center.history_session, session);
    assert!(center.error);
    assert!(center.link_opening.is_none());
}

#[test]
fn explicit_show_or_hide_reopen_retires_old_history_completion_ownership() {
    for hide_first in [false, true] {
        let (mut center, _) = center();
        let mut effects = Effects::default();
        let target = show_link_history(&mut center, &mut effects);
        let old = center.history_session.unwrap();
        let done = prepared_link_task(&mut center, &mut effects, target).run();
        if hide_first {
            center.update(Message::Hide, &mut effects);
        }
        center.update(Message::Show, &mut effects);
        let current = center.history_session.unwrap();
        assert_ne!(current, old);
        center.update(done, &mut effects);
        assert!(center.history_open());
        assert_eq!(center.history_session, Some(current));
        assert!(!center.history_closing());
    }
}

#[test]
fn toast_link_completion_has_no_ownership_of_history_opened_later() {
    let (mut center, _) = center();
    let mut effects = Effects::default();
    let value = link_note();
    let target = value.target().unwrap();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![value],
            ..Default::default()
        })),
        &mut effects,
    );
    let task = prepared_link_task(&mut center, &mut effects, target);
    assert_eq!(center.link_opening.as_ref().unwrap().history_session, None);
    center.update(Message::Show, &mut effects);
    let current = center.history_session;
    center.update(task.run(), &mut effects);
    assert!(center.history_open());
    assert_eq!(center.history_session, current);
}

#[test]
fn retired_or_superseded_success_cannot_close_current_history() {
    for retirement in 0..4 {
        let (mut center, _) = center();
        let mut effects = Effects::default();
        let target = show_link_history(&mut center, &mut effects);
        let done = prepared_link_task(&mut center, &mut effects, target).run();
        let session = center.history_session;
        match retirement {
            0 => center.update(Message::Clear, &mut effects),
            1 | 2 => {
                let mut replacement = link_note();
                if retirement == 1 {
                    replacement.revision += 1;
                } else {
                    replacement.body = "The link was replaced".into();
                }
                center.update(
                    Message::Snapshot(Ok(NotificationSnapshot {
                        history: vec![replacement],
                        ..Default::default()
                    })),
                    &mut effects,
                );
            }
            _ => center.update(
                Message::OpenLink(target, "https://example.test/second".into()),
                &mut effects,
            ),
        }
        let newer_request = center.link_opening.as_ref().map(|opening| opening.request);
        center.update(done, &mut effects);
        assert!(center.history_open());
        assert_eq!(center.history_session, session);
        assert_eq!(
            center.link_opening.as_ref().map(|opening| opening.request),
            newer_request
        );
    }
}

#[test]
fn history_session_exhaustion_never_reuses_last_or_none_as_close_authority() {
    for reopen_before_completion in [false, true] {
        let (mut center, _) = center();
        center.next_history_session = u64::MAX - 1;
        let mut effects = Effects::default();
        let target = show_link_history(&mut center, &mut effects);
        assert_eq!(center.history_session, Some(u64::MAX));
        let done = prepared_link_task(&mut center, &mut effects, target).run();
        if reopen_before_completion {
            center.update(Message::Hide, &mut effects);
            center.update(Message::Show, &mut effects);
            assert_eq!(center.history_session, None);
        }
        center.update(done, &mut effects);
        assert_eq!(center.history_open(), reopen_before_completion);
        center.update(Message::Show, &mut effects);
        assert_eq!(center.next_history_session, u64::MAX);
        assert_eq!(center.history_session, None);
        let task = prepared_link_task(&mut center, &mut effects, target);
        assert_eq!(center.link_opening.as_ref().unwrap().history_session, None);
        center.update(task.run(), &mut effects);
        assert!(center.history_open());
        assert_eq!(center.history_session, None);
    }
}

#[test]
fn unprepared_or_exhausted_link_request_cannot_close_history() {
    let (mut center, _) = center();
    let mut effects = Effects::default();
    let target = show_link_history(&mut center, &mut effects);
    center.update(
        Message::OpenLink(target, "https://example.test/first".into()),
        &mut effects,
    );
    let request = center.link_opening.as_ref().unwrap().request;
    center.update(Message::LinkDone(request, Ok(())), &mut effects);
    assert!(center.history_open());
    assert!(center.link_opening.as_ref().unwrap().waiting_for_token);
    center.next_link_request = u64::MAX;
    center.update(
        Message::OpenLink(target, "https://example.test/second".into()),
        &mut effects,
    );
    assert!(center.history_open());
    assert!(center.link_opening.is_none());
    assert_eq!(center.next_link_request, u64::MAX);
    center.update(Message::LinkDone(request, Ok(())), &mut effects);
    assert!(center.history_open());
}

#[test]
fn pending_link_tokens_are_retired_by_replacement_and_explicit_clear() {
    for retirement in 0..3 {
        let (mut center, adapter) = center();
        let mut effects = Effects::default();
        let value = link_note();
        let target = value.target().unwrap();
        center.update(
            Message::Snapshot(Ok(NotificationSnapshot {
                active: vec![value.clone()],
                ..Default::default()
            })),
            &mut effects,
        );
        center.update(
            Message::OpenLink(target, "https://example.test/first".into()),
            &mut effects,
        );
        let pending = effects.activation.pop().unwrap();
        if retirement == 0 {
            center.update(Message::Clear, &mut effects);
        } else {
            let mut replacement = value;
            if retirement == 1 {
                replacement.revision += 1;
            } else {
                // Even an adapter reusing its revision cannot grant authority
                // through a stale prepared body cache.
                replacement.body = "The link was removed".into();
            }
            center.update(
                Message::Snapshot(Ok(NotificationSnapshot {
                    active: vec![replacement],
                    ..Default::default()
                })),
                &mut effects,
            );
        }
        let task_count = effects.tasks.len();
        center.update(pending.finish(token()), &mut effects);
        assert_eq!(effects.tasks.len(), task_count);
        assert!(center.link_opening.is_none());
        assert!(adapter.links.lock().unwrap().is_empty());
    }
}

#[test]
fn newer_link_intent_rejects_old_tokens_and_completion_replay() {
    let (mut center, adapter) = center();
    let mut effects = Effects::default();
    let value = link_note();
    let target = value.target().unwrap();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            history: vec![value],
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(
        Message::OpenLink(target, "https://example.test/first".into()),
        &mut effects,
    );
    let old_id = center.link_opening.as_ref().unwrap().request;
    let old = effects.activation.pop().unwrap();
    center.update(
        Message::OpenLink(target, "https://example.test/second".into()),
        &mut effects,
    );
    let current_id = center.link_opening.as_ref().unwrap().request;
    assert_ne!(old_id, current_id);
    center.update(old.finish(token()), &mut effects);
    assert!(effects.tasks.is_empty());
    let prepared = effects.activation.pop().unwrap().finish(token());
    center.update(prepared.clone(), &mut effects);
    center.update(prepared.clone(), &mut effects);
    release_link_input(&mut center, &mut effects);
    assert_eq!(
        effects.tasks.len(),
        1,
        "one result cannot schedule a second opening"
    );
    center.error = false;
    center.update(
        Message::LinkDone(
            old_id,
            Err(lucent_domain::DomainError::Failed("old".into())),
        ),
        &mut effects,
    );
    assert!(!center.error);
    assert_eq!(center.link_opening.as_ref().unwrap().request, current_id);
    let task = effects.tasks.pop().unwrap();
    assert_eq!(task.queue, crate::work::CONTROLS);
    center.update(task.run(), &mut effects);
    center.update(prepared, &mut effects);
    assert!(effects.tasks.is_empty());
    assert!(center.link_opening.is_none());
    assert_eq!(
        *adapter.links.lock().unwrap(),
        ["https://example.test/second"]
    );
    assert_eq!(
        *adapter.link_contexts.lock().unwrap(),
        [Some("fresh-controlled-intent".into())]
    );
    assert!(adapter.calls.lock().unwrap().is_empty());
}

#[test]
fn retiring_a_link_cancels_an_opening_still_waiting_on_the_controls_queue() {
    for retirement in 0..6 {
        let (mut center, adapter) = center();
        let mut effects = Effects::default();
        let value = link_note();
        let target = value.target().unwrap();
        center.update(
            Message::Snapshot(Ok(NotificationSnapshot {
                history: vec![value],
                ..Default::default()
            })),
            &mut effects,
        );
        center.update(
            Message::OpenLink(target, "https://example.test/first".into()),
            &mut effects,
        );
        let prepared = effects.activation.pop().unwrap().finish(token());
        center.update(prepared, &mut effects);
        release_link_input(&mut center, &mut effects);
        let task = effects.tasks.pop().unwrap();
        assert!(!task.cancellation.cancelled());
        if retirement == 0 {
            center.update(Message::Clear, &mut effects);
        } else if retirement == 1 {
            center.update(
                Message::Snapshot(Ok(NotificationSnapshot::default())),
                &mut effects,
            );
        } else if retirement == 2 {
            center.update(
                Message::OpenLink(target, "https://example.test/second".into()),
                &mut effects,
            );
        } else {
            center.update(
                match retirement {
                    3 => Message::Hide,
                    4 => Message::Show,
                    _ => Message::Toggle,
                },
                &mut effects,
            );
        }
        assert!(
            task.cancellation.cancelled(),
            "the executor must skip retired queued work"
        );
        assert!(adapter.links.lock().unwrap().is_empty());
        if retirement == 2 {
            let opening = center.link_opening.as_ref().unwrap();
            assert!(opening.waiting_for_token);
            assert_eq!(opening.selected.value(), "https://example.test/second");
            assert_eq!(effects.activation.len(), 1);
        } else {
            assert!(center.link_opening.is_none());
        }
    }
}

#[test]
fn link_request_exhaustion_retires_the_last_queued_task_and_rejects_phase_replay() {
    let (mut center, adapter) = center();
    let mut effects = Effects::default();
    let target = show_link_history(&mut center, &mut effects);
    let session = center.history_session;
    center.next_link_request = u64::MAX - 1;
    let task = prepared_link_task(&mut center, &mut effects, target);
    assert_eq!(center.next_link_request, u64::MAX);
    assert!(!task.cancellation.cancelled());
    center.update(
        Message::OpenLink(target, "https://example.test/second".into()),
        &mut effects,
    );
    assert!(task.cancellation.cancelled());
    assert!(center.error);
    assert!(!center.yielding_input());
    assert!(center.history_open());
    assert_eq!(center.history_session, session);
    assert!(effects.activation.is_empty());
    assert!(effects.input_release.is_empty());
    center.error = false;
    for message in [
        Message::LinkPrepared(u64::MAX, token()),
        Message::LinkInputReleased(u64::MAX, InputReleaseOutcome::Released),
        Message::LinkInputReleased(u64::MAX, InputReleaseOutcome::Timeout),
        Message::LinkDone(u64::MAX, Ok(())),
        Message::LinkDone(
            u64::MAX,
            Err(lucent_domain::DomainError::Failed("stale".into())),
        ),
    ] {
        center.update(message, &mut effects);
    }
    assert_eq!(center.next_link_request, u64::MAX);
    assert!(center.history_open());
    assert_eq!(center.history_session, session);
    assert!(!center.error);
    assert!(!center.yielding_input());
    assert!(effects.tasks.is_empty());
    assert!(adapter.links.lock().unwrap().is_empty());
}

#[test]
fn grouped_buttons_scroll_and_close_use_actual_sdk_input_routing() {
    let (mut center, _) = center();
    let mut effects = Effects::default();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            history: (1..8).map(|id| note(id, id as u64, "Fixture")).collect(),
            ..Default::default()
        })),
        &mut effects,
    );
    center.update(Message::Show, &mut effects);
    let fonts = vec![
        lucent_ui::text::Font::from_bytes(
            include_bytes!("../../assets/LucentSans.ttf").as_slice(),
            Default::default(),
        )
        .unwrap(),
    ];
    let layout = lucent_ui::Layout::new(Arc::new(fonts));
    let cx = ViewContext {
        surface: "notifications",
        instance: None,
        width: 390.,
        height: 580.,
        now: 1.,
        density: lucent_api::Density::ONE,
    };
    let mut interaction = lucent_ui::Interaction::default();
    let scene = layout.build(&center.view(&cx), cx.width, cx.height, &interaction, cx.now);
    interaction.synchronize(&scene);
    interaction.focus = Some("notification-group-name:Fixture".into());
    for message in interaction.key(&scene, &Key::Enter) {
        center.update(message, &mut effects);
    }
    assert_eq!(center.selected_group.as_deref(), Some("name:Fixture"));
    let scene = layout.build(&center.view(&cx), cx.width, cx.height, &interaction, cx.now);
    interaction.synchronize(&scene);
    interaction.focus = Some("notification-next".into());
    for message in interaction.key(&scene, &Key::Enter) {
        center.update(message, &mut effects);
    }
    assert_eq!(center.page, 1);
    effects.now = 1.;
    interaction.focus = Some("notification-history-close".into());
    for message in interaction.key(&scene, &Key::Enter) {
        center.update(message, &mut effects);
    }
    assert!(!center.history_open());
    assert!(center.visible());
}

#[test]
fn image_toast_scrolling_keeps_the_final_native_action_reachable_on_small_outputs() {
    let (mut center, _) = center();
    let mut effects = Effects::default();
    let mut live = note(1, 7, "Fixture");
    live.image = Some(lucent_domain::NotificationImage::Pixels(
        lucent_domain::NotificationPixels::new(400, 100, vec![255; 400 * 100 * 4]).unwrap(),
    ));
    live.body =
        "Long image content remains readable through a bounded native viewport. ".repeat(12);
    let target = live.target().unwrap();
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot {
            active: vec![live],
            ..Default::default()
        })),
        &mut effects,
    );
    assert_eq!(effects.tasks.len(), 1);
    let artwork = effects.tasks.remove(0).run();
    center.update(artwork, &mut effects);
    let font = lucent_ui::text::Font::from_bytes(
        include_bytes!("../../assets/LucentSans.ttf").as_slice(),
        Default::default(),
    )
    .unwrap();
    let layout = lucent_ui::Layout::new(Arc::new(vec![font]));
    let cx = ViewContext {
        surface: "notifications",
        instance: None,
        width: 390.,
        height: 320.,
        now: 1.,
        density: lucent_api::Density::ONE,
    };
    let mut input = lucent_ui::Interaction::default();
    let scene = layout.build(&center.view(&cx), cx.width, cx.height, &input, cx.now);
    input.synchronize(&scene);
    input.focus = Some("notification-toast-scroll-1-7".into());
    for message in input.key(&scene, &Key::End) {
        center.update(message, &mut effects);
    }
    assert!(
        center
            .toast_scroll
            .get(&target)
            .is_some_and(|offset| *offset > 0.)
    );
    let scene = layout.build(&center.view(&cx), cx.width, cx.height, &input, cx.now);
    let action = scene
        .hits
        .iter()
        .find(|hit| hit.id == "notification-action-1-7-0")
        .unwrap();
    assert!(action.contains(
        action.rect.x + action.rect.w / 2.,
        action.rect.y + action.rect.h / 2.
    ));
    center.update(
        Message::Snapshot(Ok(NotificationSnapshot::default())),
        &mut effects,
    );
    center.update(
        Message::ToastScroll(target, ScrollEvent { offset: 10. }),
        &mut effects,
    );
    assert!(center.toast_scroll.is_empty());
}
