use super::*;
struct Adapter;
impl WeatherPort for Adapter {
    fn legacy_preferences(&self) -> Result<Option<WeatherPreferences>> {
        panic!("updates/views must not perform I/O")
    }
    fn forecast(&self, _: WeatherCoordinates, _: &dyn StopSignal) -> Result<WeatherSnapshot> {
        panic!("updates/views must not perform I/O")
    }
}
fn preferences(longitude: f64) -> WeatherPreferences {
    WeatherPreferences {
        enabled: true,
        location: Some(
            WeatherLocation::new(
                "Example coast",
                WeatherCoordinates::new(0., longitude).unwrap(),
            )
            .unwrap(),
        ),
        unit: TemperatureUnit::Celsius,
    }
}
fn report() -> WeatherSnapshot {
    let low = WeatherTemperature::new(3.).unwrap();
    let high = WeatherTemperature::new(8.).unwrap();
    WeatherSnapshot::new(
        high,
        WeatherCondition::Clear,
        true,
        1_768_472_640,
        1_768_472_640,
        (15..18)
            .map(|day| {
                WeatherDay::new(
                    Date::new(2026, 1, day).unwrap(),
                    WeatherCondition::Rain,
                    low,
                    high,
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()
}
fn child() -> Weather {
    let mut child = Weather::new(Arc::new(Adapter));
    child.configure(Some(&preferences(0.)), true);
    child
}
fn send(child: &mut Weather, message: Message) -> Option<Intent> {
    let mut effects = Effects::default();
    let result = child.update(message, &mut effects);
    assert!(
        effects.tasks.is_empty(),
        "weather uses its existing subscription, not a blocking effect lane"
    );
    result
}
pub(crate) fn visual_fixture(state: &str) -> Weather {
    let mut child = child();
    let generation = child.generation;
    if state == "unconfigured" {
        child.configure(Some(&WeatherPreferences::default()), true);
    } else if state == "error" {
        send(
            &mut child,
            Message::Observed(
                generation,
                Err(DomainError::Unavailable(
                    "Connection unavailable; retry when online".into(),
                )),
            ),
        );
    } else if state != "loading" {
        let mut value = report();
        if state == "night" {
            value.is_day = false;
        }
        if state == "rain" {
            value.condition = WeatherCondition::Rain;
        }
        if state == "widget-long" {
            value.condition = WeatherCondition::FreezingDrizzle;
            value.temperature = WeatherTemperature::new(-23.).unwrap();
        }
        send(&mut child, Message::Observed(generation, Ok(value)));
        if state == "stale" {
            child.tick(1_768_472_640 + 7200);
        }
        if state == "offline" {
            send(
                &mut child,
                Message::Observed(
                    generation,
                    Err(DomainError::Unavailable(
                        "Connection unavailable; last update retained".into(),
                    )),
                ),
            );
        }
    }
    send(&mut child, Message::Open);
    child.motion = Motion::fixed(1.);
    if state == "invalid" {
        let generation = child.form.as_ref().unwrap().generation;
        send(
            &mut child,
            Message::Edit(generation, Field::Latitude, TextEdit::Replace("181".into())),
        );
        send(&mut child, Message::Apply);
    }
    child
}
#[test]
fn failed_refresh_preserves_last_good_and_explicit_retry_recovers() {
    let mut child = child();
    let generation = child.generation;
    send(&mut child, Message::Observed(generation, Ok(report())));
    send(
        &mut child,
        Message::Observed(
            generation,
            Err(DomainError::Unavailable("offline fixture".into())),
        ),
    );
    assert_eq!(child.report(), Some(&report()));
    assert!(!child.error.is_empty());
    assert!(!child.busy);
    send(&mut child, Message::Refresh);
    assert!(child.busy);
    assert_eq!(child.report(), Some(&report()));
    let generation = child.generation;
    send(&mut child, Message::Observed(generation, Ok(report())));
    assert!(child.error.is_empty());
    assert!(!child.busy);
}
#[test]
fn location_changes_coalesce_and_never_present_or_save_stale_results() {
    let mut child = child();
    let first = child.generation;
    for longitude in [1., 2., 3.] {
        child.set_preferences(preferences(longitude));
    }
    assert_eq!(child.generation, first);
    assert!(child.queued);
    assert_eq!(child.subscriptions().len(), 1);
    send(&mut child, Message::Observed(first, Ok(report())));
    assert!(child.report().is_none());
    assert_eq!(child.generation, first + 1);
    let current = child.generation;
    send(&mut child, Message::Observed(first, Ok(report())));
    assert!(child.report().is_none());
    send(&mut child, Message::Observed(current, Ok(report())));
    assert!(child.report().is_some());
    assert_eq!(
        child.preferences.location.as_ref().unwrap().coordinates(),
        WeatherCoordinates::new(0., 3.).unwrap()
    );
}
#[test]
fn refresh_burst_has_one_latest_demand_and_opaque_subscription_identity() {
    let mut child = child();
    let first = child.generation;
    for _ in 0..100 {
        send(&mut child, Message::Refresh);
    }
    assert_eq!(child.generation, first);
    assert!(child.queued);
    send(&mut child, Message::Observed(first, Ok(report())));
    assert_eq!(child.generation, first + 1);
    assert!(!child.queued);
    assert_eq!(
        child.subscriptions()[0].id,
        format!("weather:{}", first + 1)
    );
    assert!(!child.inspect().to_string().contains("Example coast"));
}
#[test]
fn explicit_off_outranks_legacy_and_late_inflight_import() {
    let mut child = Weather::new(Arc::new(Adapter));
    child.configure(None, true);
    let legacy = child.generation;
    assert!(
        matches!(send(&mut child,Message::Disable),Some(Intent::Save(value)) if !value.enabled)
    );
    assert!(child.subscriptions().is_empty());
    assert!(
        send(
            &mut child,
            Message::Legacy(legacy, Ok(Some(preferences(0.))))
        )
        .is_none()
    );
    send(&mut child, Message::Refresh);
    assert!(child.subscriptions().is_empty());
    child.configure(Some(&WeatherPreferences::default()), true);
    send(&mut child, Message::Refresh);
    assert!(child.subscriptions().is_empty());
}
#[test]
fn valid_legacy_returns_root_save_intent_invalid_legacy_preserves_configuration() {
    let mut child = Weather::new(Arc::new(Adapter));
    child.configure(None, true);
    let generation = child.generation;
    assert!(
        matches!(send(&mut child,Message::Legacy(generation,Ok(Some(preferences(0.))))),Some(Intent::Save(value)) if value.enabled)
    );
    assert!(child.busy);
    assert!(!child.legacy_allowed);
    child.configure(None, true);
    let generation = child.generation;
    assert!(
        send(
            &mut child,
            Message::Legacy(
                generation,
                Err(DomainError::Invalid("invalid configuration".into()))
            )
        )
        .is_none()
    );
    assert!(!child.error.is_empty());
    assert!(child.report().is_none());
}
#[test]
fn units_and_selection_are_pure_and_invalid_form_has_no_save_intent() {
    let mut child = child();
    let generation = child.generation;
    send(&mut child, Message::Observed(generation, Ok(report())));
    send(&mut child, Message::Open);
    send(&mut child, Message::Unit(TemperatureUnit::Fahrenheit));
    assert!(
        matches!(send(&mut child,Message::Apply),Some(Intent::Save(value)) if value.unit==TemperatureUnit::Fahrenheit)
    );
    assert_eq!(child.generation, generation);
    assert!(child.summary().ends_with("°F"));
    let form = child.form.as_ref().unwrap().generation;
    assert!(
        send(
            &mut child,
            Message::Edit(
                form,
                Field::Name,
                TextEdit::Select {
                    anchor: 0,
                    caret: 1
                }
            )
        )
        .is_none()
    );
    send(
        &mut child,
        Message::Edit(form, Field::Latitude, TextEdit::Replace("NaN".into())),
    );
    assert!(send(&mut child, Message::Apply).is_none());
    assert!(!child.form_error.is_empty());
}
#[test]
fn closing_retires_clipboard_authority_and_reopened_form_identity() {
    let mut child = child();
    send(&mut child, Message::Open);
    let generation = child.form.as_ref().unwrap().generation;
    let mut effects = Effects::default();
    child.update(
        Message::Edit(generation, Field::Name, TextEdit::Paste),
        &mut effects,
    );
    let request = effects.clipboard.pop().unwrap();
    assert_eq!(request.target.surface, "weather-controls");
    send(&mut child, Message::Close);
    send(&mut child, Message::Open);
    let before = child.form.as_ref().unwrap().values.clone();
    send(
        &mut child,
        request.finish(lucent_api::ClipboardOutcome::Text(
            "stale private text".into(),
        )),
    );
    assert_eq!(child.form.as_ref().unwrap().values, before);
    assert_ne!(child.form.as_ref().unwrap().generation, generation);
}
#[test]
fn missing_preferences_and_readonly_failure_never_start_a_request() {
    let mut child = Weather::new(Arc::new(Adapter));
    assert!(child.subscriptions().is_empty());
    child.configure(None, false);
    send(&mut child, Message::Open);
    assert!(send(&mut child, Message::Apply).is_none());
    send(&mut child, Message::Refresh);
    assert!(child.subscriptions().is_empty());
}

#[test]
fn transient_form_input_and_close_frames_do_not_invalidate_shared_consumers() {
    let mut child = child();
    let request = child.generation;
    send(&mut child, Message::Observed(request, Ok(report())));
    send(&mut child, Message::Open);
    let form = child.form.as_ref().unwrap().generation;
    for message in [
        Message::Edit(
            form,
            Field::Name,
            TextEdit::Select {
                anchor: 0,
                caret: 2,
            },
        ),
        Message::Edit(form, Field::Name, TextEdit::Replace("New label".into())),
        Message::Unit(TemperatureUnit::Fahrenheit),
        Message::Scroll(ScrollEvent { offset: 32. }),
    ] {
        let mut effects = Effects::default();
        assert!(child.update(message, &mut effects).is_none());
        assert!(effects.redraw.contains("weather-controls"));
        assert!(!effects.redraw.contains("widgets"));
        assert!(!effects.redraw.contains("bar"));
    }
    let mut effects = Effects::default();
    assert!(matches!(
        child.update(Message::Apply, &mut effects),
        Some(Intent::Save(_))
    ));
    assert!(
        effects.redraw.contains("widgets"),
        "shared group covers every monitor and grouped weather member"
    );
    assert!(effects.redraw.contains("bar"));
    let mut effects = Effects::default();
    child.update(Message::Ignore, &mut effects);
    assert!(effects.redraw.is_empty());
    child.update(Message::Close, &mut effects);
    child.update(Message::Frame(100.), &mut effects);
    assert!(!child.visible());
    assert!(effects.redraw.contains("weather-controls"));
    assert!(!effects.redraw.contains("widgets"));
    assert!(!effects.redraw.contains("bar"));
}
