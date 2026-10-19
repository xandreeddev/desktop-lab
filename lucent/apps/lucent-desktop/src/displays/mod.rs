//! Native display client: draft editing, explicit transaction and rollback UI.
mod view;
use lucent_api::*;
use lucent_design::motion;
use lucent_domain::*;
use std::{sync::Arc, time::Duration};
#[derive(Clone)]
pub enum Message {
    Observed(Result<DisplaySnapshot>),
    Open,
    Toggle,
    Close,
    Closed(u64),
    Refresh,
    Select(MonitorId),
    Mode(MonitorId, DisplayMode),
    Enable(MonitorId),
    Rotate(MonitorId),
    Edit(u64, MonitorId, usize, TextEdit),
    Apply,
    Keep,
    Revert,
    Admitted(u64, Result<DisplayTransactionId>),
    Completed(u64, Result<()>),
    Scroll(ScrollEvent),
    Ignore,
}
struct Row {
    configuration: DisplayConfiguration,
    values: [String; 3],
    fields: [TextFieldState; 3],
}
impl Row {
    fn new(c: DisplayConfiguration) -> Self {
        Self {
            values: [
                c.position.x().to_string(),
                c.position.y().to_string(),
                format!("{:.3}", c.scale.value()),
            ],
            fields: std::array::from_fn(|_| TextFieldState::default()),
            configuration: c,
        }
    }
}
pub struct Displays {
    adapter: Arc<dyn DisplayPort>,
    snapshot: Option<DisplaySnapshot>,
    rows: Vec<Row>,
    baseline: Vec<DisplayConfiguration>,
    selected: Option<MonitorId>,
    open: bool,
    mounted: bool,
    motion: Motion,
    close_generation: u64,
    revision: u64,
    active: Option<u64>,
    transaction: Option<DisplayTransactionId>,
    scroll: f32,
    error: String,
    observation_error: String,
    dirty: bool,
}
impl Displays {
    pub fn new(adapter: Arc<dyn DisplayPort>) -> Self {
        Self {
            adapter,
            snapshot: None,
            rows: vec![],
            baseline: vec![],
            selected: None,
            open: false,
            mounted: false,
            motion: Motion::fixed(0.),
            close_generation: 0,
            revision: 0,
            active: None,
            transaction: None,
            scroll: 0.,
            error: String::new(),
            observation_error: String::new(),
            dirty: false,
        }
    }
    pub fn open(&self) -> bool {
        self.open
    }
    pub fn visible(&self) -> bool {
        self.mounted
    }
    pub fn animating(&self, now: f64) -> bool {
        self.motion.active(now)
    }
    fn pending(&self) -> bool {
        self.active.is_some()
            || self.transaction.is_some()
            || self
                .snapshot
                .as_ref()
                .and_then(|s| s.transaction.as_ref())
                .is_some_and(|t| t.phase.pending())
    }
    fn reset(&mut self) {
        if let Some(snapshot) = &self.snapshot {
            self.baseline = snapshot.configuration();
            self.rows = self.baseline.iter().cloned().map(Row::new).collect();
            if self
                .selected
                .as_ref()
                .is_none_or(|id| !self.rows.iter().any(|r| &r.configuration.output == id))
            {
                self.selected = self.rows.first().map(|r| r.configuration.output.clone());
            }
            self.dirty = false;
            self.revision = self.revision.wrapping_add(1);
            self.error.clear();
        }
    }
    fn set_open(&mut self, open: bool, now: f64) {
        if open == self.open {
            return;
        }
        self.open = open;
        self.mounted = true;
        if !open {
            self.close_generation = self.close_generation.wrapping_add(1);
            for row in &mut self.rows {
                for field in &mut row.fields {
                    field.cancel();
                }
            }
        }
        self.motion.target(
            if open { 1. } else { 0. },
            now,
            if open { motion::ENTER } else { motion::EXIT },
            motion::DECELERATE,
        );
    }
    fn draft(&self) -> Result<Vec<DisplayConfiguration>> {
        self.rows
            .iter()
            .map(|row| {
                let mut c = row.configuration.clone();
                let x = row.values[0].trim().parse::<i32>().map_err(|_| {
                    DomainError::Invalid("Enter a whole horizontal position".into())
                })?;
                let y = row.values[1]
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| DomainError::Invalid("Enter a whole vertical position".into()))?;
                c.position = DisplayPosition::new(x, y)?;
                let scale = row.values[2]
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .ok_or_else(|| DomainError::Invalid("Enter a finite display scale".into()))?;
                c.scale = DisplayScale::new((scale * 1000.).round() as u32)?;
                Ok(c)
            })
            .collect()
    }
    fn command(&mut self, keep: bool, effects: &mut Effects<Message>) {
        let Some(id) = self
            .snapshot
            .as_ref()
            .and_then(|s| s.transaction.as_ref())
            .filter(|t| t.phase.pending())
            .map(|t| t.id)
            .or(self.transaction)
        else {
            return;
        };
        self.revision = self.revision.wrapping_add(1);
        let revision = self.revision;
        self.active = Some(revision);
        let adapter = self.adapter.clone();
        effects.task_on_with_failure(
            crate::work::CONTROLS,
            move || {
                Message::Completed(
                    revision,
                    if keep {
                        adapter.keep(id)
                    } else {
                        adapter.revert(id)
                    },
                )
            },
            move |f| Message::Completed(revision, Err(DomainError::Failed(f.to_string()))),
        );
    }
    pub fn update(&mut self, message: Message, effects: &mut Effects<Message>) {
        match message {
            Message::Observed(result) => match result {
                Ok(snapshot) => {
                    if self.snapshot.as_ref() == Some(&snapshot) {
                        return;
                    }
                    let terminal = snapshot
                        .transaction
                        .as_ref()
                        .is_some_and(|t| !t.phase.pending())
                        && self.transaction.is_some_and(|id| {
                            snapshot.transaction.as_ref().is_some_and(|t| t.id == id)
                        });
                    self.snapshot = Some(snapshot);
                    self.observation_error.clear();
                    if terminal {
                        self.transaction = None;
                        self.active = None;
                        self.reset();
                    } else if !self.dirty && !self.pending() {
                        self.reset();
                    }
                }
                Err(error) => {
                    self.observation_error = error.to_string();
                }
            },
            Message::Open => self.set_open(true, effects.now),
            Message::Toggle => {
                if self.open {
                    if self.pending() {
                        self.command(false, effects);
                    }
                    self.set_open(false, effects.now);
                } else {
                    self.set_open(true, effects.now);
                }
            }
            Message::Close => {
                if self.pending() {
                    self.command(false, effects);
                }
                self.set_open(false, effects.now);
            }
            Message::Closed(generation) => {
                if !self.open && self.close_generation == generation {
                    self.mounted = false;
                }
            }
            Message::Refresh => {
                if !self.pending() {
                    self.reset();
                }
            }
            Message::Select(id) => {
                if self.rows.iter().any(|r| r.configuration.output == id) {
                    self.selected = Some(id);
                    self.scroll = 0.;
                }
            }
            Message::Mode(id, mode) => {
                if self.open
                    && !self.pending()
                    && self.snapshot.as_ref().is_some_and(|s| {
                        s.outputs
                            .iter()
                            .any(|o| o.current.output == id && o.modes.contains(&mode))
                    })
                    && let Some(row) = self.rows.iter_mut().find(|r| r.configuration.output == id)
                {
                    row.configuration.mode = mode;
                    self.dirty = true;
                }
            }
            Message::Enable(id) => {
                if self.open
                    && !self.pending()
                    && let Some(row) = self.rows.iter_mut().find(|r| r.configuration.output == id)
                {
                    row.configuration.enabled = !row.configuration.enabled;
                    self.dirty = true;
                }
            }
            Message::Rotate(id) => {
                if self.open
                    && !self.pending()
                    && let Some(row) = self.rows.iter_mut().find(|r| r.configuration.output == id)
                {
                    row.configuration.transform = (row.configuration.transform + 1) % 4;
                    self.dirty = true;
                }
            }
            Message::Edit(revision, id, index, edit) => {
                if self.open
                    && !self.pending()
                    && revision == self.revision
                    && index < 3
                    && let Some(row) = self.rows.iter_mut().find(|r| r.configuration.output == id)
                {
                    let callback_id = id.clone();
                    match row.fields[index].update(
                        &mut row.values[index],
                        edit,
                        24,
                        ClipboardTarget::new(
                            "displays",
                            format!("display-field-{index}-{revision}"),
                        ),
                        effects,
                        move |edit| Message::Edit(revision, callback_id.clone(), index, edit),
                    ) {
                        Ok(change) => self.dirty |= change.content,
                        Err(e) => self.error = e.to_string(),
                    }
                }
            }
            Message::Apply => {
                if self.open && !self.pending() {
                    let draft = self.draft().and_then(|draft| {
                        let s = self.snapshot.as_ref().ok_or_else(|| {
                            DomainError::Unavailable("Displays are not available".into())
                        })?;
                        lucent_usecases::displays::validate_display_draft(
                            &s.outputs,
                            &self.baseline,
                            &draft,
                        )?;
                        Ok(draft)
                    });
                    match draft {
                        Err(e) => self.error = e.to_string(),
                        Ok(draft) => {
                            self.revision = self.revision.wrapping_add(1);
                            let revision = self.revision;
                            self.active = Some(revision);
                            let adapter = self.adapter.clone();
                            let baseline = self.baseline.clone();
                            self.error.clear();
                            effects.task_on_with_failure(
                                crate::work::CONTROLS,
                                move || Message::Admitted(revision, adapter.apply(baseline, draft)),
                                move |f| {
                                    Message::Admitted(
                                        revision,
                                        Err(DomainError::Failed(f.to_string())),
                                    )
                                },
                            );
                        }
                    }
                }
            }
            Message::Keep => {
                if self.open && self.active.is_none() {
                    self.command(true, effects);
                }
            }
            Message::Revert => {
                if self.active.is_none() {
                    self.command(false, effects);
                }
            }
            Message::Admitted(revision, result) => {
                if self.active != Some(revision) {
                    if let Ok(id) = result {
                        let adapter = self.adapter.clone();
                        effects.task_on(crate::work::CONTROLS, move || {
                            Message::Completed(revision, adapter.revert(id))
                        });
                    }
                    return;
                }
                self.active = None;
                match result {
                    Ok(id) => {
                        let completed = self
                            .snapshot
                            .as_ref()
                            .and_then(|s| s.transaction.as_ref())
                            .is_some_and(|t| t.id == id && !t.phase.pending());
                        if completed {
                            self.transaction = None;
                            self.reset();
                        } else {
                            self.transaction = Some(id);
                            if !self.open {
                                self.command(false, effects);
                            }
                        }
                    }
                    Err(e) => self.error = e.to_string(),
                }
            }
            Message::Completed(revision, result) => {
                if self.active == Some(revision) {
                    self.active = None;
                    if let Err(e) = result {
                        self.error = e.to_string();
                    }
                }
            }
            Message::Scroll(e) => {
                if e.offset.is_finite() {
                    self.scroll = e.offset.max(0.);
                }
            }
            Message::Ignore => return,
        }
        effects.redraw("displays");
    }
    pub fn escape(&self) -> Message {
        Message::Close
    }
    pub fn subscriptions(&self) -> Vec<Subscription<Message>> {
        if self.mounted && !self.open {
            let generation = self.close_generation;
            vec![Subscription::stream(
                format!("displays-close-{generation}"),
                move |out, cancel| {
                    cancel.sleep(Duration::from_secs_f64(motion::EXIT));
                    if !cancel.cancelled() {
                        out.send(Message::Closed(generation));
                    }
                },
            )]
        } else {
            vec![]
        }
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"open":self.open,"visible":self.mounted,"busy":self.pending(),"dirty":self.dirty,"output_count":self.rows.len(),"selected":self.selected.as_ref().map(MonitorId::as_str),"error":self.error,"observation_error":self.observation_error,"transaction":self.snapshot.as_ref().and_then(|s|s.transaction.as_ref()).map(|t|serde_json::json!({"id":t.id.value(),"phase":match t.phase{DisplayTransactionPhase::Preparing=>"preparing",DisplayTransactionPhase::AwaitingConfirmation{..}=>"confirmation",DisplayTransactionPhase::Kept=>"kept",DisplayTransactionPhase::Reverted{..}=>"reverted",DisplayTransactionPhase::Failed{..}=>"failed"}}))})
    }
}

#[cfg(test)]
pub(super) mod tests;
