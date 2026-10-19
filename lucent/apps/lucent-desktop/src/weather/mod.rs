//! One weather client: checked preferences return to the root persistence owner;
//! observations run on the existing weather subscription, never an effect lane.
use lucent_api::{
    Cancellation, ClipboardTarget, Effects, Motion, ScrollEvent, Subscription, TextEdit,
    TextFieldState,
};
use lucent_domain::*;
use std::{sync::Arc, time::Duration};
#[cfg(test)]
pub(crate) mod tests;
mod view;
pub use view::condition_symbol;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Latitude,
    Longitude,
}
impl Field {
    fn index(self) -> usize {
        match self {
            Self::Name => 0,
            Self::Latitude => 1,
            Self::Longitude => 2,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Latitude => "latitude",
            Self::Longitude => "longitude",
        }
    }
}
#[derive(Clone)]
pub enum Message {
    Open,
    Close,
    Refresh,
    Disable,
    Apply,
    RetrySave,
    Ignore,
    Edit(u64, Field, TextEdit),
    Unit(TemperatureUnit),
    Scroll(ScrollEvent),
    Frame(f64),
    Started(u64),
    Observed(u64, Result<WeatherSnapshot>),
    Legacy(u64, Result<Option<WeatherPreferences>>),
    WorkerFailed(u64),
}
pub enum Intent {
    Save(WeatherPreferences),
    RetrySave,
}
#[derive(Clone, Copy)]
pub struct SaveState<'a> {
    pub status: crate::persistence::Status,
    pub error: &'a str,
    pub writable: bool,
}
#[derive(Clone, Copy)]
enum Work {
    Legacy,
    Forecast(WeatherCoordinates),
}
struct Worker {
    generation: u64,
    work: Work,
}
struct Stop(Cancellation);
impl StopSignal for Stop {
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
    fn wait(&self, d: Duration) {
        self.0.sleep(d);
    }
}
struct Form {
    generation: u64,
    values: [String; 3],
    fields: [TextFieldState; 3],
    unit: TemperatureUnit,
    dirty: bool,
}
impl Form {
    fn new(generation: u64, preferences: &WeatherPreferences) -> Self {
        let mut values = [String::new(), String::new(), String::new()];
        if let Some(location) = &preferences.location {
            values = [
                location.name().into(),
                location.coordinates().latitude().to_string(),
                location.coordinates().longitude().to_string(),
            ];
        }
        let mut fields = std::array::from_fn(|_| TextFieldState::default());
        for (field, value) in fields.iter_mut().zip(&values) {
            field.reset(value);
        }
        Self {
            generation,
            values,
            fields,
            unit: preferences.unit,
            dirty: false,
        }
    }
    fn preferences(&self) -> Result<WeatherPreferences> {
        let parse = |index: usize| {
            self.values[index]
                .trim()
                .parse::<f64>()
                .map_err(|_| DomainError::Invalid("Enter numeric latitude and longitude".into()))
        };
        let coordinates = WeatherCoordinates::new(parse(1)?, parse(2)?)?;
        Ok(WeatherPreferences {
            enabled: true,
            location: Some(WeatherLocation::new(&self.values[0], coordinates)?),
            unit: self.unit,
        })
    }
    fn retire(&mut self) {
        for field in &mut self.fields {
            field.cancel();
        }
    }
}
pub struct Weather {
    adapter: Arc<dyn WeatherPort>,
    preferences: WeatherPreferences,
    initialized: bool,
    writable: bool,
    legacy_allowed: bool,
    worker: Option<Worker>,
    generation: u64,
    busy: bool,
    queued: bool,
    report: Option<WeatherSnapshot>,
    error: String,
    form_error: String,
    form: Option<Form>,
    form_generation: u64,
    pub open: bool,
    pub mounted: bool,
    pub motion: Motion,
    scroll: f32,
    now: u64,
}
impl Weather {
    pub fn new(adapter: Arc<dyn WeatherPort>) -> Self {
        Self {
            adapter,
            preferences: Default::default(),
            initialized: false,
            writable: false,
            legacy_allowed: false,
            worker: None,
            generation: 0,
            busy: false,
            queued: false,
            report: None,
            error: String::new(),
            form_error: String::new(),
            form: None,
            form_generation: 0,
            open: false,
            mounted: false,
            motion: Motion::fixed(0.),
            scroll: 0.,
            now: 0,
        }
    }
    pub fn configure(&mut self, saved: Option<&WeatherPreferences>, writable: bool) {
        self.preferences = saved.cloned().unwrap_or_default();
        self.worker = None;
        self.busy = false;
        self.queued = false;
        self.initialized = true;
        self.writable = writable;
        self.legacy_allowed = saved.is_none();
        self.report = None;
        self.error.clear();
        if !writable {
            self.worker = None;
            self.busy = false;
            self.error = "Desktop preferences are unavailable; weather is paused".into();
            return;
        }
        if self.open {
            self.reset_form();
        }
        match saved {
            Some(saved) => {
                self.preferences = saved.clone();
                self.start_desired();
            }
            None => self.start(Work::Legacy),
        }
    }
    fn reset_form(&mut self) {
        if let Some(form) = &mut self.form {
            form.retire();
        }
        let Some(next) = self.form_generation.checked_add(1) else {
            self.form = None;
            self.form_error = "Weather form limit reached; restart the desktop".into();
            return;
        };
        self.form_generation = next;
        self.form = Some(Form::new(next, &self.preferences));
    }
    fn start(&mut self, work: Work) {
        let Some(generation) = self.generation.checked_add(1) else {
            self.error = "Weather request limit reached; restart the desktop".into();
            self.worker = None;
            self.busy = false;
            return;
        };
        self.generation = generation;
        self.worker = Some(Worker { generation, work });
        self.busy = true;
        self.queued = false;
    }
    fn desired(&self) -> Option<WeatherCoordinates> {
        self.preferences
            .location
            .as_ref()
            .filter(|_| self.preferences.enabled)
            .map(WeatherLocation::coordinates)
    }
    fn start_desired(&mut self) {
        if let Some(coordinates) = self.desired() {
            self.start(Work::Forecast(coordinates));
        } else {
            self.worker = None;
            self.busy = false;
            self.queued = false;
        }
    }
    fn set_preferences(&mut self, preferences: WeatherPreferences) {
        let before = self.desired();
        self.preferences = preferences;
        self.legacy_allowed = false;
        self.form_error.clear();
        if !self.preferences.enabled {
            self.worker = None;
            self.busy = false;
            self.queued = false;
            self.report = None;
            self.error.clear();
            return;
        }
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| matches!(worker.work, Work::Legacy))
        {
            self.worker = None;
            self.busy = false;
        }
        if before != self.desired() {
            self.report = None;
            self.error.clear();
            if self.busy {
                self.queued = true;
            } else {
                self.start_desired();
            }
        }
    }
    fn refresh(&mut self) {
        if self.desired().is_some() {
            if self.busy {
                self.queued = true;
            } else {
                self.start_desired();
            }
        } else if self.initialized && self.writable && self.legacy_allowed {
            if self.busy {
                self.queued = true;
            } else {
                self.start(Work::Legacy);
            }
        }
    }
    pub fn report(&self) -> Option<&WeatherSnapshot> {
        self.report.as_ref()
    }
    pub fn unit(&self) -> TemperatureUnit {
        self.preferences.unit
    }
    pub fn open(&self) -> bool {
        self.open
    }
    pub fn visible(&self) -> bool {
        self.mounted
    }
    pub fn animating(&self, now: f64) -> bool {
        self.mounted && self.motion.active(now)
    }
    pub fn status(&self) -> &'static str {
        if !self.initialized {
            "Loading preferences"
        } else if self.busy {
            if self.report.is_some() {
                "Refreshing…"
            } else {
                "Loading weather…"
            }
        } else if !self.error.is_empty() {
            if self.report.is_some() {
                "Refresh failed · showing last update"
            } else {
                "Weather unavailable"
            }
        } else if self.desired().is_none() {
            "Weather is off · choose a location"
        } else if self.stale() {
            "Last update is stale"
        } else {
            "Current conditions"
        }
    }
    fn stale(&self) -> bool {
        self.report.as_ref().is_some_and(|report| {
            self.now.saturating_sub(report.fetched_at) > 30 * 60
                || self.now.saturating_sub(report.observed_at) > 30 * 60
        })
    }
    pub fn tick(&mut self, now: u64) -> bool {
        let before = (self.now / 60, self.stale());
        self.now = now;
        (self.now / 60, self.stale()) != before && self.report.is_some()
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"configured":self.desired().is_some(),"busy":self.busy,"has_last_good":self.report.is_some(),"stale":self.stale(),"open":self.open,"generation":self.generation,"unit":self.preferences.unit,"failed":!self.error.is_empty()})
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) -> Option<Intent> {
        let mut intent = None;
        let mut shared_presentation_changed = false;
        match message {
            Message::Open => {
                if !self.open {
                    self.form_error.clear();
                    self.reset_form();
                    self.scroll = 0.;
                    self.open = true;
                    self.mounted = true;
                    self.motion.target(
                        1.,
                        effects.now,
                        lucent_design::motion::ENTER,
                        lucent_design::motion::DECELERATE,
                    );
                }
            }
            Message::Close => {
                if !self.open {
                    return None;
                }
                self.open = false;
                self.motion.target(
                    0.,
                    effects.now,
                    lucent_design::motion::EXIT,
                    lucent_design::motion::DECELERATE,
                );
                if let Some(form) = &mut self.form {
                    form.retire();
                }
            }
            Message::Frame(now) => {
                if self.open || !self.mounted || self.motion.active(now) {
                    return None;
                }
                self.mounted = false;
                self.form = None;
            }
            Message::Edit(generation, field, edit) => {
                if self.open
                    && self.writable
                    && let Some(form) = self
                        .form
                        .as_mut()
                        .filter(|form| form.generation == generation)
                {
                    let index = field.index();
                    let limit = if field == Field::Name {
                        WeatherLocation::NAME_LIMIT
                    } else {
                        32
                    };
                    match form.fields[index].update(
                        &mut form.values[index],
                        edit,
                        limit,
                        ClipboardTarget::new(
                            "weather-controls",
                            format!("weather-{}-{generation}", field.name()),
                        ),
                        effects,
                        move |edit| Message::Edit(generation, field, edit),
                    ) {
                        Ok(change) => form.dirty |= change.content,
                        Err(error) => self.form_error = error.to_string(),
                    }
                }
            }
            Message::Unit(unit) => {
                if self.writable
                    && let Some(form) = &mut self.form
                {
                    form.dirty |= form.unit != unit;
                    form.unit = unit;
                }
            }
            Message::Apply => {
                if self.open
                    && self.writable
                    && let Some(form) = &mut self.form
                {
                    match form.preferences() {
                        Ok(preferences) => {
                            form.retire();
                            shared_presentation_changed = self.preferences != preferences;
                            self.set_preferences(preferences.clone());
                            intent = Some(Intent::Save(preferences));
                        }
                        Err(error) => self.form_error = error.to_string(),
                    }
                }
            }
            Message::Disable => {
                if self.writable {
                    let preferences = WeatherPreferences {
                        enabled: false,
                        ..self.preferences.clone()
                    };
                    shared_presentation_changed = self.preferences.enabled
                        || self.report.is_some()
                        || self.busy
                        || !self.error.is_empty();
                    self.set_preferences(preferences.clone());
                    if let Some(form) = &mut self.form {
                        form.retire();
                    }
                    intent = Some(Intent::Save(preferences));
                }
            }
            Message::Refresh => {
                let before = (self.generation, self.busy);
                self.refresh();
                shared_presentation_changed = before != (self.generation, self.busy);
            }
            Message::Started(generation) => {
                if self
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.generation == generation)
                {
                    shared_presentation_changed = !self.busy;
                    self.busy = true;
                } else {
                    return None;
                }
            }
            Message::Observed(generation, result) => {
                let worker = self
                    .worker
                    .as_ref()
                    .filter(|worker| worker.generation == generation)?;
                let Work::Forecast(coordinates) = worker.work else {
                    return None;
                };
                self.busy = false;
                shared_presentation_changed = true;
                if Some(coordinates) == self.desired() {
                    match result {
                        Ok(report) => {
                            self.now = self.now.max(report.fetched_at);
                            self.report = Some(report);
                            self.error.clear();
                        }
                        Err(error) => self.error = error.to_string(),
                    }
                }
                if self.queued {
                    self.start_desired();
                }
            }
            Message::Legacy(generation, result) => {
                if !self.worker.as_ref().is_some_and(|worker| {
                    worker.generation == generation && matches!(worker.work, Work::Legacy)
                }) {
                    return None;
                }
                self.worker = None;
                self.busy = false;
                shared_presentation_changed = true;
                match result {
                    Ok(Some(preferences)) => {
                        let clean_form = self.form.as_ref().is_some_and(|form| !form.dirty);
                        self.set_preferences(preferences.clone());
                        if clean_form {
                            self.reset_form();
                        }
                        intent = Some(Intent::Save(preferences));
                    }
                    Ok(None) => (),
                    Err(error) => self.error = error.to_string(),
                }
                if self.queued && self.legacy_allowed {
                    self.start(Work::Legacy);
                }
            }
            Message::WorkerFailed(generation) => {
                if !self
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.generation == generation)
                {
                    return None;
                }
                self.worker = None;
                self.busy = false;
                shared_presentation_changed = true;
                self.error = "Weather worker stopped; retry the refresh".into();
                if self.queued {
                    self.start_desired();
                }
            }
            Message::Scroll(event) => {
                if event.offset.is_finite() && event.offset >= 0. {
                    self.scroll = event.offset;
                }
            }
            Message::RetrySave => intent = Some(Intent::RetrySave),
            Message::Ignore => return None,
        }
        effects.redraw("weather-controls");
        if shared_presentation_changed {
            // Widgets can share a native group whose source role is another
            // catalog member. Preserve the existing all-host redraw group.
            effects.redraw("bar");
            effects.redraw("widgets");
        }
        intent
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        let Some(worker) = &self.worker else {
            return vec![];
        };
        let generation = worker.generation;
        let work = worker.work;
        let adapter = self.adapter.clone();
        vec![Subscription::stream(
            format!("weather:{generation}"),
            move |out, cancel| match work {
                Work::Legacy => {
                    if !cancel.cancelled() {
                        out.send(Message::Legacy(generation, adapter.legacy_preferences()));
                    }
                }
                Work::Forecast(coordinates) => {
                    while !cancel.cancelled() {
                        out.send(Message::Started(generation));
                        if cancel.cancelled() {
                            break;
                        }
                        let result = adapter.forecast(coordinates, &Stop(cancel.clone()));
                        if cancel.cancelled() {
                            break;
                        }
                        out.send(Message::Observed(generation, result));
                        cancel.sleep(Duration::from_secs(15 * 60));
                    }
                }
            },
        )]
    }
}
