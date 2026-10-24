//! Opt-in compositor presentation feedback. This observes existing commits; it
//! never requests another redraw, timer or compositor frame callback.
use super::*;
use smithay_client_toolkit::presentation_time::{
    PresentTime, PresentationTimeHandler, PresentationTimeState,
};
use std::collections::{HashMap, VecDeque};
use wayland_client::{WEnum, backend::ObjectId, globals::GlobalList};
use wayland_protocols::wp::presentation_time::client::wp_presentation_feedback::{
    self, WpPresentationFeedback,
};

const PENDING_LIMIT: usize = 256;
const HISTORY_LIMIT: usize = crate::profiling::MAX_FRAMES;

struct Pending {
    // Protocol identity only. SCTK owns the benign proxy in its callback data;
    // neither identity/proxy owns our Native or its LayerSurface. SCTK destroys
    // the layer and owned wl_surface explicitly when those owners drop.
    surface: ObjectId,
    address: api::SurfaceAddress,
    native_generation: u64,
    retired: bool,
    frame: u64,
    requested_ms: f64,
    streams: Option<lucent_render::StreamTrace>,
}
#[derive(Default)]
pub(super) struct Capture {
    global: Option<PresentationTimeState>,
    enabled: bool,
    advertised: bool,
    pending: HashMap<ObjectId, Pending>,
    skipped: u64,
    retired: u64,
    accepted: u64,
    next_native_generation: u64,
    generation_exhausted: bool,
    unmatched_results: u64,
    pending_lost: u64,
    retired_history: RetiredHistory,
}
impl Capture {
    pub(super) fn bind<A: Application>(
        globals: &GlobalList,
        qh: &QueueHandle<State<A>>,
        allowed: bool,
    ) -> Self {
        let enabled =
            allowed && std::env::var_os("LUCENT_PROFILE_PRESENTATION").is_some_and(|v| v == "1");
        let advertised = enabled
            && globals
                .contents()
                .with_list(|list| list.iter().any(|g| g.interface == "wp_presentation"));
        Self {
            global: advertised.then(|| PresentationTimeState::bind(globals, qh)),
            enabled,
            advertised,
            pending: HashMap::new(),
            skipped: 0,
            retired: 0,
            accepted: 0,
            next_native_generation: 0,
            generation_exhausted: false,
            unmatched_results: 0,
            pending_lost: 0,
            retired_history: RetiredHistory::default(),
        }
    }
    pub(super) fn tracing(&self) -> bool {
        self.global.is_some()
    }
    pub(super) fn start_native(&mut self) -> Option<u64> {
        if !self.tracing() {
            return None;
        }
        self.allocate_generation()
    }
    fn allocate_generation(&mut self) -> Option<u64> {
        let Some(next) = self.next_native_generation.checked_add(1) else {
            self.generation_exhausted = true;
            return None;
        };
        self.next_native_generation = next;
        Some(next)
    }
    pub(super) fn retire_native(&mut self, generation: Option<u64>) {
        let Some(generation) = generation else { return };
        for pending in self.pending.values_mut() {
            if pending.native_generation == generation {
                pending.retired = true;
            }
        }
    }
    pub(super) fn clear_pending(&mut self) {
        self.pending_lost = self.pending_lost.saturating_add(self.pending.len() as u64);
        self.pending.clear();
    }
    /// Called only in the renderer's before-present hook, after successful
    /// acquire/paint and before WSI commits the matching buffer.
    pub(super) fn request<A: Application>(
        &mut self,
        surface: &wl_surface::WlSurface,
        qh: &QueueHandle<State<A>>,
        identity: Option<(u64, api::SurfaceAddress)>,
        frame: u64,
        requested_ms: f64,
        streams: Option<lucent_render::StreamTrace>,
    ) {
        let Some(global) = &self.global else {
            return;
        };
        let Some((native_generation, address)) = identity else {
            self.skipped = self.skipped.saturating_add(1);
            return;
        };
        if self.pending.len() >= PENDING_LIMIT {
            self.skipped = self.skipped.saturating_add(1);
            return;
        }
        match global.feedback(surface, qh) {
            Ok(feedback) => {
                self.accepted = self.accepted.saturating_add(1);
                self.pending.insert(
                    feedback.id(),
                    Pending {
                        surface: surface.id(),
                        address,
                        native_generation,
                        retired: false,
                        frame,
                        requested_ms,
                        streams,
                    },
                );
            }
            Err(_) => {
                self.skipped = self.skipped.saturating_add(1);
            }
        }
    }
    fn complete(
        &mut self,
        feedback: &WpPresentationFeedback,
        surface: &wl_surface::WlSurface,
    ) -> Option<Pending> {
        self.complete_id(feedback.id(), surface.id())
    }
    fn complete_id(&mut self, feedback: ObjectId, surface: ObjectId) -> Option<Pending> {
        let Some(pending) = self.pending.remove(&feedback) else {
            self.unmatched_results = self.unmatched_results.saturating_add(1);
            return None;
        };
        if pending.surface != surface {
            self.pending_lost = self.pending_lost.saturating_add(1);
            return None;
        }
        Some(pending)
    }
    fn present(
        &mut self,
        pending: Pending,
        active: Option<(&mut History, Option<u64>)>,
        presented: Presented,
    ) {
        if let Some((history, generation)) = active
            && !pending.retired
            && generation == Some(pending.native_generation)
        {
            history.present(pending, presented);
            return;
        }
        self.retired = self.retired.saturating_add(1);
        let sample = Sample::presented(&pending, presented);
        let outcome = if sample.is_some() {
            Completion::Presented
        } else {
            Completion::Invalid
        };
        self.retired_history.push(pending, outcome, sample);
    }
    fn discard(&mut self, pending: Pending, active: Option<(&mut History, Option<u64>)>) {
        if let Some((history, generation)) = active
            && !pending.retired
            && generation == Some(pending.native_generation)
        {
            history.discarded = history.discarded.saturating_add(1);
            return;
        }
        self.retired = self.retired.saturating_add(1);
        self.retired_history
            .push(pending, Completion::Discarded, None);
    }
    pub(super) fn json(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.enabled,"advertised":self.advertised,"accepted_requests":self.accepted,"pending":self.pending.len(),"pending_limit":PENDING_LIMIT,
            "skipped":self.skipped,"retired_surface_results":self.retired,
            "native_generations":self.next_native_generation,"generation_exhausted":self.generation_exhausted,
            "unmatched_results":self.unmatched_results,"pending_lost":self.pending_lost,
            "pending_records":self.pending.values().map(Pending::json).collect::<Vec<_>>(),
            "retired":self.retired_history.json(),
            "scope":"Compositor-reported wp_presentation timestamps and flags for existing submissions. Hardware scanout/completion depends on each sample's flags; counters and CPU submission timings remain separate."})
    }
}
#[derive(Clone, Copy)]
struct Sample {
    native_generation: u64,
    frame: u64,
    requested_ms: f64,
    clock_id: u32,
    presented_ns: u64,
    refresh_ns: u32,
    sequence: u64,
    flags: u32,
    outputs: usize,
    streams: Option<lucent_render::StreamTrace>,
}
struct Presented {
    time: PresentTime,
    refresh: u32,
    sequence: u64,
    flags: u32,
    outputs: usize,
}
impl Sample {
    fn presented(pending: &Pending, presented: Presented) -> Option<Self> {
        Some(Self {
            native_generation: pending.native_generation,
            frame: pending.frame,
            requested_ms: pending.requested_ms,
            clock_id: presented.time.clk_id,
            presented_ns: timestamp(presented.time.tv_sec, presented.time.tv_nsec)?,
            refresh_ns: presented.refresh,
            sequence: presented.sequence,
            flags: presented.flags,
            outputs: presented.outputs,
            streams: pending.streams,
        })
    }
    fn json(&self) -> serde_json::Value {
        let streams=self.streams.map(|trace|serde_json::json!({"revisions":trace.revisions().iter().map(|(id,revision)|serde_json::json!({"stream_id":id,"revision":revision})).collect::<Vec<_>>(),"truncated":trace.truncated(),"scope":"Bound resource references; opacity, occlusion and final pixel visibility are not established"}));
        serde_json::json!({"native_generation":self.native_generation,"frame":self.frame,"requested_at_ms":self.requested_ms,"clock_id":self.clock_id,
            "presented_ns":self.presented_ns,"refresh_ns":self.refresh_ns,"sequence":self.sequence,"flags":self.flags,"sync_output_count":self.outputs,"streams":streams})
    }
}
impl Pending {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({"surface_id":self.address.to_string(),"native_generation":self.native_generation,
            "frame":self.frame,"requested_at_ms":self.requested_ms,"retired":self.retired})
    }
}
enum Completion {
    Presented,
    Discarded,
    Invalid,
}
struct RetiredRecord {
    record: u64,
    address: api::SurfaceAddress,
    native_generation: u64,
    frame: u64,
    requested_ms: f64,
    outcome: Completion,
    presentation: Option<Sample>,
}
#[derive(Default)]
struct RetiredHistory {
    records: VecDeque<RetiredRecord>,
    completed: u64,
    presented: u64,
    discarded: u64,
    invalid: u64,
    evicted: u64,
    dropped: u64,
    counter_exhausted: bool,
}
impl RetiredHistory {
    fn push(&mut self, pending: Pending, outcome: Completion, presentation: Option<Sample>) {
        let Some(record) = self.completed.checked_add(1) else {
            self.counter_exhausted = true;
            self.dropped = self.dropped.saturating_add(1);
            return;
        };
        self.completed = record;
        match &outcome {
            Completion::Presented => self.presented += 1,
            Completion::Discarded => self.discarded += 1,
            Completion::Invalid => self.invalid += 1,
        }
        if self.records.len() == HISTORY_LIMIT {
            self.records.pop_front();
            self.evicted = self.evicted.saturating_add(1);
        }
        self.records.push_back(RetiredRecord {
            record,
            address: pending.address,
            native_generation: pending.native_generation,
            frame: pending.frame,
            requested_ms: pending.requested_ms,
            outcome,
            presentation,
        });
    }
    fn json(&self) -> serde_json::Value {
        serde_json::json!({"completed":self.completed,"presented":self.presented,"discarded":self.discarded,"invalid":self.invalid,
            "evicted":self.evicted,"dropped":self.dropped,"counter_exhausted":self.counter_exhausted,
            "truncated":self.evicted>0||self.dropped>0,
            "history_limit":HISTORY_LIMIT,"records":self.records.iter().map(|r| {
                let outcome=match &r.outcome {Completion::Presented=>"presented",Completion::Discarded=>"discarded",Completion::Invalid=>"invalid"};
                serde_json::json!({"record":r.record,"surface_id":r.address.to_string(),"native_generation":r.native_generation,
                    "frame":r.frame,"requested_at_ms":r.requested_ms,"outcome":outcome,"presentation":r.presentation.as_ref().map(Sample::json)})
            }).collect::<Vec<_>>(),"scope":"Bounded completed retired-native feedback only; separate from replacement histories. Eviction, pending loss and unresolved requests prevent completeness claims. No native surface or window is retained."})
    }
}
#[derive(Default)]
pub(super) struct History {
    samples: VecDeque<Sample>,
    presented: u64,
    discarded: u64,
    invalid: u64,
}
fn timestamp(seconds: u64, nanos: u32) -> Option<u64> {
    if nanos >= 1_000_000_000 {
        return None;
    }
    seconds
        .checked_mul(1_000_000_000)?
        .checked_add(u64::from(nanos))
}
impl History {
    fn present(&mut self, pending: Pending, presented: Presented) {
        let Some(sample) = Sample::presented(&pending, presented) else {
            self.invalid = self.invalid.saturating_add(1);
            return;
        };
        self.push(sample);
    }
    fn push(&mut self, sample: Sample) {
        self.presented = self.presented.saturating_add(1);
        if self.samples.len() >= HISTORY_LIMIT {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }
    pub(super) fn json(&self) -> serde_json::Value {
        serde_json::json!({"presented":self.presented,"discarded":self.discarded,"invalid":self.invalid,"history_limit":HISTORY_LIMIT,
            "samples":self.samples.iter().map(Sample::json).collect::<Vec<_>>()})
    }
}
impl<A: Application> PresentationTimeHandler for State<A> {
    fn presentation_time_state(&mut self) -> &mut PresentationTimeState {
        // Only a global bound by this Capture can dispatch this callback.
        self.presentation
            .global
            .as_mut()
            .expect("bound presentation global")
    }
    fn presented(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        feedback: &WpPresentationFeedback,
        surface: &wl_surface::WlSurface,
        outputs: Vec<wl_output::WlOutput>,
        time: PresentTime,
        refresh: u32,
        sequence: u64,
        flags: WEnum<wp_presentation_feedback::Kind>,
    ) {
        let Some(pending) = self.presentation.complete(feedback, surface) else {
            return;
        };
        // Preserve current-native routing; late results have their own bounded
        // history and can never enter a replacement's active history.
        let address = self.id_for(surface);
        let active = address
            .as_ref()
            .and_then(|address| self.surfaces.get_mut(address))
            .filter(|native| native.role.wl_surface().id() == pending.surface)
            .map(|native| (&mut native.history.presentation, native.native_generation));
        let flags = match flags {
            WEnum::Value(value) => value.bits(),
            WEnum::Unknown(raw) => raw,
        };
        self.presentation.present(
            pending,
            active,
            Presented {
                time,
                refresh,
                sequence,
                flags,
                outputs: outputs.len(),
            },
        );
    }
    fn discarded(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        feedback: &WpPresentationFeedback,
        surface: &wl_surface::WlSurface,
    ) {
        let Some(pending) = self.presentation.complete(feedback, surface) else {
            return;
        };
        let address = self.id_for(surface);
        let active = address
            .as_ref()
            .and_then(|address| self.surfaces.get_mut(address))
            .filter(|native| native.role.wl_surface().id() == pending.surface)
            .map(|native| (&mut native.history.presentation, native.native_generation));
        self.presentation.discard(pending, active);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pending(generation: u64, frame: u64) -> Pending {
        Pending {
            surface: ObjectId::null(),
            address: api::SurfaceAddress::new("dock"),
            native_generation: generation,
            retired: false,
            frame,
            requested_ms: frame as f64 * 16.,
            streams: Some([(7, frame)].into_iter().collect()),
        }
    }
    fn presented(nanos: u32) -> Presented {
        Presented {
            time: PresentTime {
                clk_id: 4,
                tv_sec: 12,
                tv_nsec: nanos,
            },
            refresh: 16_666_667,
            sequence: 123,
            flags: 5,
            outputs: 1,
        }
    }
    #[test]
    fn disabled_presentation_does_not_allocate_native_identities_or_history() {
        let mut capture = Capture::default();
        assert_eq!(capture.start_native(), None);
        assert_eq!(capture.next_native_generation, 0);
        assert_eq!(capture.pending.capacity(), 0);
        assert_eq!(capture.retired_history.records.capacity(), 0);
        let value = capture.json();
        assert_eq!(value["retired"]["completed"], 0);
        assert_eq!(value["retired"]["truncated"], false);
        assert_eq!(value["pending_lost"], 0);
    }
    #[test]
    fn native_generations_do_not_alias_or_wrap_on_recreation() {
        let mut capture = Capture::default();
        assert_eq!(capture.allocate_generation(), Some(1));
        assert_eq!(capture.allocate_generation(), Some(2));
        capture.next_native_generation = u64::MAX;
        assert_eq!(capture.allocate_generation(), None);
        assert_eq!(capture.next_native_generation, u64::MAX);
        assert!(capture.generation_exhausted);
    }
    #[test]
    fn late_feedback_stays_separate_from_continued_logical_history() {
        let mut capture = Capture::default();
        let old_generation = capture.allocate_generation().unwrap();
        let mut old = handoff::SubmissionHistory {
            frames: 10,
            ..Default::default()
        };
        old.presentation
            .present(pending(old_generation, 9), presented(10));
        let mut replacement = old.continuation();
        let next_generation = capture.allocate_generation().unwrap();
        capture.present(
            pending(old_generation, 10),
            Some((&mut replacement.presentation, Some(next_generation))),
            presented(20),
        );
        assert_eq!(
            replacement.frames, 10,
            "feedback does not submit a new frame"
        );
        assert_eq!(replacement.presentation.presented, 1);
        assert_eq!(capture.retired, 1);
        let retired = capture.retired_history.json();
        assert_eq!(retired["records"][0]["native_generation"], old_generation);
        assert_eq!(
            retired["records"][0]["presentation"]["presented_ns"],
            12_000_000_020u64
        );
        assert_eq!(retired["records"][0]["presentation"]["flags"], 5);
        assert_eq!(
            retired["records"][0]["presentation"]["streams"]["revisions"][0]["revision"],
            10
        );
        capture.present(
            pending(next_generation, 11),
            Some((&mut replacement.presentation, Some(next_generation))),
            presented(30),
        );
        assert_eq!(replacement.presentation.presented, 2);
        let current = replacement.presentation.json();
        assert_eq!(current["samples"][0]["native_generation"], old_generation);
        assert_eq!(current["samples"][1]["native_generation"], next_generation);
        assert_eq!(capture.retired_history.completed, 1);
    }
    #[test]
    fn retirement_keeps_pending_context_and_never_routes_it_back_to_active_history() {
        let mut capture = Capture::default();
        capture.pending.insert(ObjectId::null(), pending(1, 7));
        capture.retire_native(Some(1));
        assert_eq!(capture.json()["pending_records"][0]["retired"], true);
        let result = capture
            .complete_id(ObjectId::null(), ObjectId::null())
            .unwrap();
        let mut active = History::default();
        capture.present(result, Some((&mut active, Some(1))), presented(99));
        assert_eq!(active.presented, 0);
        assert_eq!(capture.retired_history.presented, 1);
        assert!(
            capture
                .complete_id(ObjectId::null(), ObjectId::null())
                .is_none()
        );
        assert_eq!(capture.unmatched_results, 1);
    }
    #[test]
    fn active_and_retired_discard_and_invalid_results_keep_their_own_scope() {
        let mut capture = Capture::default();
        let mut active = History::default();
        capture.discard(pending(2, 2), Some((&mut active, Some(2))));
        capture.discard(pending(1, 1), Some((&mut active, Some(2))));
        capture.present(
            pending(1, 3),
            Some((&mut active, Some(2))),
            presented(1_000_000_000),
        );
        capture.present(
            pending(2, 4),
            Some((&mut active, Some(2))),
            presented(1_000_000_000),
        );
        assert_eq!(active.discarded, 1);
        assert_eq!(active.invalid, 1);
        let retired = capture.retired_history.json();
        assert_eq!(retired["discarded"], 1);
        assert_eq!(retired["invalid"], 1);
        assert_eq!(retired["records"][0]["outcome"], "discarded");
        assert_eq!(retired["records"][1]["outcome"], "invalid");
        assert!(retired["records"][1]["presentation"].is_null());
    }
    #[test]
    fn retired_completion_overflow_is_bounded_and_reports_missing_records() {
        let mut capture = Capture::default();
        for frame in 0..HISTORY_LIMIT as u64 + 3 {
            capture.discard(pending(1, frame), None);
        }
        let history = capture.retired_history.json();
        assert_eq!(history["records"].as_array().unwrap().len(), HISTORY_LIMIT);
        assert_eq!(history["completed"], HISTORY_LIMIT as u64 + 3);
        assert_eq!(history["evicted"], 3);
        assert_eq!(history["truncated"], true);
        assert_eq!(history["records"][0]["frame"], 3);
        assert_eq!(history["records"][0]["record"], 4);
        capture.retired_history.completed = u64::MAX;
        capture.discard(pending(1, 999), None);
        assert!(capture.retired_history.counter_exhausted);
        assert_eq!(capture.retired_history.dropped, 1);
        assert_eq!(capture.retired_history.records.len(), HISTORY_LIMIT);
    }
    #[test]
    fn shutdown_records_outstanding_feedback_loss_instead_of_fabricating_completion() {
        let mut capture = Capture::default();
        capture.pending.insert(ObjectId::null(), pending(1, 42));
        capture.clear_pending();
        assert_eq!(capture.pending_lost, 1);
        assert!(capture.pending.is_empty());
        assert_eq!(capture.retired_history.completed, 0);
        assert!(
            capture
                .complete_id(ObjectId::null(), ObjectId::null())
                .is_none()
        );
        assert_eq!(capture.unmatched_results, 1);
    }
    #[test]
    fn a_surface_proxy_clone_does_not_prevent_explicit_protocol_destruction() {
        struct Peer;
        wayland_client::delegate_noop!(Peer: ignore wayland_client::protocol::wl_registry::WlRegistry);
        wayland_client::delegate_noop!(Peer: ignore wayland_client::protocol::wl_compositor::WlCompositor);
        wayland_client::delegate_noop!(Peer: ignore wl_surface::WlSurface);
        let (client, _server) = std::os::unix::net::UnixStream::pair().unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let queue = connection.new_event_queue::<Peer>();
        let qh = queue.handle();
        let registry = connection.display().get_registry(&qh, ());
        let compositor = registry
            .bind::<wayland_client::protocol::wl_compositor::WlCompositor, _, _>(1, 1, &qh, ());
        let surface = compositor.create_surface(&qh, ());
        let borrowed_metadata = surface.clone();
        let id = surface.id();
        surface.destroy();
        assert!(!borrowed_metadata.is_alive());
        assert_eq!(
            borrowed_metadata.id(),
            id,
            "SCTK late-feedback identity remains recognizable"
        );
    }
    #[test]
    fn presentation_timestamp_rejects_malformed_values_and_keeps_nanosecond_precision() {
        assert_eq!(timestamp(12, 345), Some(12_000_000_345));
        assert_eq!(timestamp(0, 999_999_999), Some(999_999_999));
        assert_eq!(timestamp(0, 1_000_000_000), None);
        assert_eq!(timestamp(u64::MAX, 0), None);
    }
    #[test]
    fn presented_history_is_bounded_and_preserves_association_clock_flags_and_discard_scope() {
        let mut history = History {
            discarded: 3,
            ..Default::default()
        };
        for frame in 0..HISTORY_LIMIT as u64 + 7 {
            history.push(Sample {
                native_generation: 9,
                frame,
                requested_ms: frame as f64 * 16.,
                clock_id: 4,
                presented_ns: frame * 16_666_667,
                refresh_ns: 16_666_667,
                sequence: frame + 1000,
                flags: 5,
                outputs: 1,
                streams: Some([(7, frame)].into_iter().collect()),
            });
        }
        let state = history.json();
        assert_eq!(state["presented"], HISTORY_LIMIT as u64 + 7);
        assert_eq!(state["discarded"], 3);
        let samples = state["samples"].as_array().unwrap();
        assert_eq!(samples.len(), HISTORY_LIMIT);
        assert_eq!(samples[0]["frame"], 7);
        assert_eq!(samples[0]["native_generation"], 9);
        assert_eq!(samples[0]["clock_id"], 4);
        assert_eq!(samples[0]["sequence"], 1007);
        assert_eq!(samples[0]["flags"], 5);
        assert_eq!(samples[0]["streams"]["revisions"][0]["revision"], 7);
        assert_eq!(samples[0]["streams"]["revisions"][0]["stream_id"], 7);
        assert_eq!(samples[0]["presented_ns"], 7 * 16_666_667u64);
    }
}
