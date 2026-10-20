//! Opt-in client-side callback/attempt observations; these never drive demand.
//! Request-to-handler includes compositor, transport and client dispatch time.
use std::{collections::VecDeque, time::Instant};

pub(super) const HISTORY_LIMIT: usize = 512;

pub(super) fn now(epoch: Instant) -> u64 {
    epoch.elapsed().as_nanos().try_into().unwrap_or(u64::MAX)
}

#[derive(Default)]
pub(super) struct Capture {
    enabled: bool,
    generation: u64,
    retired: u64,
    retired_pending_callbacks: u64,
    retired_ready_callbacks: u64,
    retired_unclosed_attempts: u64,
    retired_history_rows: u64,
    callbacks_for_retired_native: u64,
    generation_exhausted: bool,
}
impl Capture {
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }
    pub(super) fn start_native(&mut self) -> Option<Trace> {
        if !self.enabled {
            return None;
        }
        let Some(generation) = self.generation.checked_add(1) else {
            self.generation_exhausted = true;
            return None;
        };
        self.generation = generation;
        Some(Trace::new(generation))
    }
    pub(super) fn retire(&mut self, trace: Option<Trace>) {
        let Some(trace) = trace else {
            return;
        };
        self.retired = self.retired.saturating_add(1);
        self.retired_pending_callbacks = self
            .retired_pending_callbacks
            .saturating_add(trace.pending_callbacks);
        self.retired_ready_callbacks = self
            .retired_ready_callbacks
            .saturating_add(u64::from(trace.ready.is_some()));
        self.retired_unclosed_attempts = self
            .retired_unclosed_attempts
            .saturating_add(u64::from(trace.active.is_some()));
        self.retired_history_rows = self
            .retired_history_rows
            .saturating_add(trace.events.len() as u64);
    }
    pub(super) fn retired_callback(&mut self) {
        if self.enabled {
            self.callbacks_for_retired_native = self.callbacks_for_retired_native.saturating_add(1);
        }
    }
    pub(super) fn json(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.enabled,"history_limit":HISTORY_LIMIT,
            "native_generations":self.generation,"retired_native_surfaces":self.retired,
            "retired_pending_callbacks":self.retired_pending_callbacks,
            "retired_ready_callbacks":self.retired_ready_callbacks,
            "retired_unclosed_attempts":self.retired_unclosed_attempts,
            "retired_history_rows":self.retired_history_rows,
            "callbacks_for_retired_native":self.callbacks_for_retired_native,
            "generation_exhausted":self.generation_exhausted,
            "scope":"Client Instant-epoch nanoseconds; request-to-handler combines compositor, transport, OS scheduling and client dispatch. Core callback_data_ms has an undefined base and is not aligned with this epoch or wp_presentation. No GPU, scanout or compositor-only latency."})
    }
}

#[derive(Clone, Copy)]
struct Request {
    frame: u64,
    at: u64,
}
#[derive(Clone, Copy)]
struct Ready {
    event: u64,
    at: u64,
}
#[derive(Clone, Copy)]
struct Attempt {
    id: u64,
    next_frame: u64,
    at: u64,
    ready: Option<Ready>,
}
#[derive(Clone, Copy)]
pub(super) enum Outcome {
    Presented,
    RetryAfter(std::time::Duration),
    Error,
    PreRenderAbort,
    Unclosed,
}
impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Self::Presented => "presented",
            Self::RetryAfter(_) => "retry",
            Self::Error => "render_error",
            Self::PreRenderAbort => "pre_render_abort",
            Self::Unclosed => "unclosed",
        }
    }
}
enum Event {
    Callback {
        sequence: u64,
        request: Option<Request>,
        at: u64,
        raw: u32,
        ambiguous: bool,
    },
    Attempt {
        sequence: u64,
        attempt: Attempt,
        at: u64,
        outcome: Outcome,
    },
}
impl Event {
    fn json(&self) -> serde_json::Value {
        match self {
            Self::Callback {
                sequence,
                request,
                at,
                raw,
                ambiguous,
            } => serde_json::json!({
                "event":sequence,"kind":"callback","request_frame":request.map(|r|r.frame),
                "requested_at_ns":request.map(|r|r.at),"handler_at_ns":at,"callback_data_ms":raw,
                "request_to_handler_ns":request.and_then(|r|interval(r.at,*at)),
                "correlation":if *ambiguous {"ambiguous"} else if request.is_some() {"matched"} else {"unmatched"}
            }),
            Self::Attempt {
                sequence,
                attempt,
                at,
                outcome,
            } => serde_json::json!({
                "event":sequence,"kind":"attempt","attempt":attempt.id,"next_frame":attempt.next_frame,
                "callback_event":attempt.ready.map(|r|r.event),
                "callback_handler_at_ns":attempt.ready.map(|r|r.at),
                "started_at_ns":attempt.at,"finished_at_ns":at,"duration_ns":interval(attempt.at,*at),
                "callback_to_attempt_ns":attempt.ready.and_then(|r|interval(r.at,attempt.at)),
                "outcome":outcome.name(),"retry_after_ns":match outcome {
                    Outcome::RetryAfter(delay)=>u64::try_from(delay.as_nanos()).ok(),_=>None
                }
            }),
        }
    }
}
fn interval(start: u64, end: u64) -> Option<u64> {
    if start == u64::MAX || end == u64::MAX {
        None
    } else {
        end.checked_sub(start)
    }
}

pub(super) struct Trace {
    generation: u64,
    events: VecDeque<Event>,
    events_total: u64,
    trimmed_events: u64,
    requests: u64,
    callbacks: u64,
    attempts: u64,
    completed_attempts: u64,
    pending_callbacks: u64,
    request: Option<Request>,
    ready: Option<Ready>,
    active: Option<Attempt>,
    ambiguous: bool,
    overlapping_requests: u64,
    uncorrelated_requests: u64,
    unmatched_callbacks: u64,
    unclosed_attempts: u64,
    unmatched_completions: u64,
}
impl Trace {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            events: VecDeque::new(),
            events_total: 0,
            trimmed_events: 0,
            requests: 0,
            callbacks: 0,
            attempts: 0,
            completed_attempts: 0,
            pending_callbacks: 0,
            request: None,
            ready: None,
            active: None,
            ambiguous: false,
            overlapping_requests: 0,
            uncorrelated_requests: 0,
            unmatched_callbacks: 0,
            unclosed_attempts: 0,
            unmatched_completions: 0,
        }
    }
    fn push(&mut self, event: Event) {
        if self.events.len() == HISTORY_LIMIT {
            self.events.pop_front();
            self.trimmed_events = self.trimmed_events.saturating_add(1);
        }
        self.events_total = self.events_total.saturating_add(1);
        self.events.push_back(event);
    }
    pub(super) fn request(&mut self, frame: u64, at: u64) {
        self.requests = self.requests.saturating_add(1);
        if self.pending_callbacks != 0 && !self.ambiguous {
            // SCTK forwards only the surface, not the callback proxy. Never
            // attach an earlier outstanding callback to a newer request.
            self.ambiguous = true;
            self.overlapping_requests = self.overlapping_requests.saturating_add(1);
            self.uncorrelated_requests = self
                .uncorrelated_requests
                .saturating_add(self.pending_callbacks);
            self.request = None;
        }
        self.pending_callbacks = self.pending_callbacks.saturating_add(1);
        if self.ambiguous {
            self.uncorrelated_requests = self.uncorrelated_requests.saturating_add(1);
        } else {
            self.request = Some(Request { frame, at });
        }
    }
    pub(super) fn callback(&mut self, at: u64, raw: u32) {
        self.callbacks = self.callbacks.saturating_add(1);
        let request = if self.pending_callbacks == 0 {
            self.unmatched_callbacks = self.unmatched_callbacks.saturating_add(1);
            None
        } else {
            self.pending_callbacks -= 1;
            self.request.take().filter(|_| !self.ambiguous)
        };
        let sequence = self.events_total.saturating_add(1);
        self.push(Event::Callback {
            sequence,
            request,
            at,
            raw,
            ambiguous: self.ambiguous,
        });
        self.ready = Some(Ready {
            event: sequence,
            at,
        });
    }
    pub(super) fn begin_attempt(&mut self, next_frame: u64, at: u64) {
        if self.active.is_some() {
            self.unclosed_attempts = self.unclosed_attempts.saturating_add(1);
            self.finish_attempt(at, Outcome::Unclosed);
        }
        self.attempts = self.attempts.saturating_add(1);
        self.active = Some(Attempt {
            id: self.attempts,
            next_frame,
            at,
            ready: self.ready.take(),
        });
    }
    pub(super) fn finish_attempt(&mut self, at: u64, outcome: Outcome) {
        let Some(attempt) = self.active.take() else {
            self.unmatched_completions = self.unmatched_completions.saturating_add(1);
            return;
        };
        self.completed_attempts = self.completed_attempts.saturating_add(1);
        self.push(Event::Attempt {
            sequence: self.events_total.saturating_add(1),
            attempt,
            at,
            outcome,
        });
    }
    pub(super) fn json(&self) -> serde_json::Value {
        serde_json::json!({"native_generation":self.generation,"history_limit":HISTORY_LIMIT,
            "events_total":self.events_total,"trimmed_events":self.trimmed_events,
            "requests":self.requests,"callbacks":self.callbacks,"attempts":self.attempts,
            "completed_attempts":self.completed_attempts,"pending_callbacks":self.pending_callbacks,
            "pending_request":self.request.map(|r|serde_json::json!({"frame":r.frame,"requested_at_ns":r.at})),
            "ready_callback_event":self.ready.map(|r|r.event),
            "active_attempt":self.active.map(|a|serde_json::json!({"attempt":a.id,"next_frame":a.next_frame,"started_at_ns":a.at})),
            "correlation_ambiguous":self.ambiguous,"overlapping_requests":self.overlapping_requests,
            "uncorrelated_requests":self.uncorrelated_requests,"unmatched_callbacks":self.unmatched_callbacks,
            "unclosed_attempts":self.unclosed_attempts,"unmatched_completions":self.unmatched_completions,
            "events":self.events.iter().map(Event::json).collect::<Vec<_>>()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_capture_never_creates_native_or_per_frame_state() {
        let mut capture = Capture::new(false);
        assert!(capture.start_native().is_none());
        capture.retire(None);
        capture.retired_callback();
        assert_eq!(capture.json()["native_generations"], 0);
        assert_eq!(capture.json()["callbacks_for_retired_native"], 0);
    }

    #[test]
    fn callback_join_and_attempt_intervals_use_only_client_epoch() {
        let mut trace = Trace::new(7);
        trace.request(41, 100);
        trace.callback(140, u32::MAX - 2);
        trace.begin_attempt(42, 145);
        trace.finish_attempt(155, Outcome::Presented);
        let value = trace.json();
        assert_eq!(value["events"][0]["request_frame"], 41);
        assert_eq!(value["events"][0]["request_to_handler_ns"], 40);
        assert_eq!(value["events"][0]["callback_data_ms"], u32::MAX - 2);
        assert_eq!(value["events"][1]["callback_event"], 1);
        assert_eq!(value["events"][1]["callback_to_attempt_ns"], 5);
        assert_eq!(value["events"][1]["duration_ns"], 10);
        assert_eq!(value["events"][1]["next_frame"], 42);
    }

    #[test]
    fn overlapping_requests_never_rejoin_later_callbacks_to_newest_frame() {
        let mut trace = Trace::new(1);
        trace.request(1, 10);
        trace.request(2, 20);
        trace.callback(30, 5);
        trace.callback(40, 6);
        trace.request(3, 50);
        trace.callback(60, 7);
        let value = trace.json();
        assert_eq!(value["overlapping_requests"], 1);
        assert_eq!(value["uncorrelated_requests"], 3);
        assert_eq!(value["pending_callbacks"], 0);
        for event in value["events"].as_array().unwrap() {
            assert_eq!(event["correlation"], "ambiguous");
            assert!(event["request_frame"].is_null());
            assert!(event["requested_at_ns"].is_null());
            assert!(event["request_to_handler_ns"].is_null());
        }
    }

    #[test]
    fn first_attempt_retry_error_and_pre_render_abort_are_not_success_only() {
        let mut trace = Trace::new(1);
        trace.begin_attempt(1, 10);
        trace.finish_attempt(20, Outcome::RetryAfter(std::time::Duration::from_millis(8)));
        trace.begin_attempt(1, 30);
        trace.finish_attempt(40, Outcome::Error);
        trace.begin_attempt(1, 50);
        trace.finish_attempt(60, Outcome::PreRenderAbort);
        let value = trace.json();
        assert_eq!(value["attempts"], 3);
        assert_eq!(value["completed_attempts"], 3);
        assert_eq!(value["events"][0]["outcome"], "retry");
        assert_eq!(value["events"][0]["retry_after_ns"], 8_000_000);
        assert!(value["events"][0]["callback_event"].is_null());
        assert_eq!(value["events"][1]["outcome"], "render_error");
        assert_eq!(value["events"][2]["outcome"], "pre_render_abort");
    }

    #[test]
    fn retirement_discards_pending_correlations_and_new_native_starts_clean() {
        let mut capture = Capture::new(true);
        let mut old = capture.start_native().unwrap();
        old.callback(10, 1); // An unmatched callback still has an observed handler.
        old.begin_attempt(1, 11); // Intentionally leave an unclosed attempt.
        old.request(1, 12);
        capture.retire(Some(old));
        capture.retired_callback();
        let new = capture.start_native().unwrap().json();
        assert_eq!(new["native_generation"], 2);
        assert!(new["events"].as_array().unwrap().is_empty());
        assert_eq!(new["pending_callbacks"], 0);
        assert!(new["active_attempt"].is_null());
        assert_eq!(capture.json()["retired_pending_callbacks"], 1);
        assert_eq!(capture.json()["retired_unclosed_attempts"], 1);
        assert_eq!(capture.json()["callbacks_for_retired_native"], 1);
    }

    #[test]
    fn history_trimming_and_idle_callback_boundaries_are_explicit() {
        let mut trace = Trace::new(1);
        trace.request(1, 10);
        trace.callback(20, 2);
        assert_eq!(trace.json()["ready_callback_event"], 1);
        assert_eq!(trace.json()["attempts"], 0);
        for n in 0..HISTORY_LIMIT as u64 {
            trace.begin_attempt(n + 2, 30 + n * 2);
            trace.finish_attempt(31 + n * 2, Outcome::Presented);
        }
        let value = trace.json();
        assert_eq!(value["events"].as_array().unwrap().len(), HISTORY_LIMIT);
        assert_eq!(value["trimmed_events"], 1);
        assert_eq!(value["events_total"], HISTORY_LIMIT + 1);
        assert_eq!(value["events"][0]["event"], 2);
        assert_eq!(value["events"][0]["callback_event"], 1);
        assert!(value["events"][1]["callback_event"].is_null());
    }

    #[test]
    fn unclosed_attempt_and_invalid_intervals_never_become_success_latency() {
        let mut trace = Trace::new(1);
        trace.request(1, 20);
        trace.callback(10, 7);
        trace.begin_attempt(2, 30);
        trace.begin_attempt(2, 40);
        trace.finish_attempt(35, Outcome::Error);
        trace.finish_attempt(50, Outcome::Presented);
        let value = trace.json();
        assert!(value["events"][0]["request_to_handler_ns"].is_null());
        assert_eq!(value["events"][1]["outcome"], "unclosed");
        assert!(value["events"][2]["duration_ns"].is_null());
        assert_eq!(value["unclosed_attempts"], 1);
        assert_eq!(value["unmatched_completions"], 1);
        assert!(interval(u64::MAX, u64::MAX).is_none());
    }
}
