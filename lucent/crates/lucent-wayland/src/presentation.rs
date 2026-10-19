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
    surface: wl_surface::WlSurface,
    frame: u64,
    requested_ms: f64,
}
pub(super) struct Capture {
    global: Option<PresentationTimeState>,
    enabled: bool,
    advertised: bool,
    pending: HashMap<ObjectId, Pending>,
    skipped: u64,
    retired: u64,
    accepted: u64,
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
        }
    }
    /// Called only in the renderer's before-present hook, after successful
    /// acquire/paint and before WSI commits the matching buffer.
    pub(super) fn request<A: Application>(
        &mut self,
        surface: &wl_surface::WlSurface,
        qh: &QueueHandle<State<A>>,
        frame: u64,
        requested_ms: f64,
    ) {
        let Some(global) = &self.global else {
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
                        surface: surface.clone(),
                        frame,
                        requested_ms,
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
        self.pending
            .remove(&feedback.id())
            .filter(|pending| &pending.surface == surface)
    }
    pub(super) fn json(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.enabled,"advertised":self.advertised,"accepted_requests":self.accepted,"pending":self.pending.len(),"pending_limit":PENDING_LIMIT,
            "skipped":self.skipped,"retired_surface_results":self.retired,
            "scope":"Compositor-reported wp_presentation timestamps and flags for existing submissions. Hardware scanout/completion depends on each sample's flags; counters and CPU submission timings remain separate."})
    }
}
#[derive(Clone, Copy)]
struct Sample {
    frame: u64,
    requested_ms: f64,
    clock_id: u32,
    presented_ns: u64,
    refresh_ns: u32,
    sequence: u64,
    flags: u32,
    outputs: usize,
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
    fn present(
        &mut self,
        pending: Pending,
        time: PresentTime,
        refresh: u32,
        sequence: u64,
        flags: u32,
        outputs: usize,
    ) {
        let Some(presented_ns) = timestamp(time.tv_sec, time.tv_nsec) else {
            self.invalid = self.invalid.saturating_add(1);
            return;
        };
        self.push(Sample {
            frame: pending.frame,
            requested_ms: pending.requested_ms,
            clock_id: time.clk_id,
            presented_ns,
            refresh_ns: refresh,
            sequence,
            flags,
            outputs,
        });
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
            "samples":self.samples.iter().map(|s|serde_json::json!({"frame":s.frame,"requested_at_ms":s.requested_ms,"clock_id":s.clock_id,
                "presented_ns":s.presented_ns,"refresh_ns":s.refresh_ns,"sequence":s.sequence,"flags":s.flags,"sync_output_count":s.outputs})).collect::<Vec<_>>()})
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
        // A logical address may already name a recreated native surface. Keep
        // old feedback out of the replacement's history and frame identities.
        let Some(address) = self.id_for(surface) else {
            self.presentation.retired = self.presentation.retired.saturating_add(1);
            return;
        };
        let Some(native) = self
            .surfaces
            .get_mut(&address)
            .filter(|n| n.role.wl_surface() == &pending.surface)
        else {
            return;
        };
        let flags = match flags {
            WEnum::Value(value) => value.bits(),
            WEnum::Unknown(raw) => raw,
        };
        native
            .history
            .presentation
            .present(pending, time, refresh, sequence, flags, outputs.len());
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
        let Some(address) = self.id_for(surface) else {
            self.presentation.retired = self.presentation.retired.saturating_add(1);
            return;
        };
        if let Some(native) = self
            .surfaces
            .get_mut(&address)
            .filter(|n| n.role.wl_surface() == &pending.surface)
        {
            native.history.presentation.discarded =
                native.history.presentation.discarded.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                frame,
                requested_ms: frame as f64 * 16.,
                clock_id: 4,
                presented_ns: frame * 16_666_667,
                refresh_ns: 16_666_667,
                sequence: frame + 1000,
                flags: 5,
                outputs: 1,
            });
        }
        let state = history.json();
        assert_eq!(state["presented"], HISTORY_LIMIT as u64 + 7);
        assert_eq!(state["discarded"], 3);
        let samples = state["samples"].as_array().unwrap();
        assert_eq!(samples.len(), HISTORY_LIMIT);
        assert_eq!(samples[0]["frame"], 7);
        assert_eq!(samples[0]["clock_id"], 4);
        assert_eq!(samples[0]["sequence"], 1007);
        assert_eq!(samples[0]["flags"], 5);
        assert_eq!(samples[0]["presented_ns"], 7 * 16_666_667u64);
    }
}
