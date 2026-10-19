//! Client-owned media PTS associations for generic prepared image revisions.
//! Diagnostic metadata only; no image pixels, asset paths or playback policy.
use std::{collections::VecDeque, time::Duration};
const LIMIT: usize = 256;
#[derive(Clone, Copy)]
struct Sample {
    generation: u64,
    stream_id: u64,
    revision: u64,
    source_ns: u64,
    cycle: u64,
}
#[derive(Default)]
pub(super) struct SourceFrames {
    samples: VecDeque<Sample>,
    prepared: u64,
    evicted: u64,
    previous_ns: Option<u64>,
    cycle: u64,
}
impl SourceFrames {
    pub fn record(&mut self, generation: u64, frame: (u64, u64), source: Duration) {
        let Ok(source_ns) = u64::try_from(source.as_nanos()) else {
            return;
        };
        if self
            .previous_ns
            .is_some_and(|previous| source_ns <= previous)
        {
            self.cycle = self.cycle.saturating_add(1);
        }
        self.previous_ns = Some(source_ns);
        self.prepared = self.prepared.saturating_add(1);
        if self.samples.len() == LIMIT {
            self.samples.pop_front();
            self.evicted = self.evicted.saturating_add(1);
        }
        self.samples.push_back(Sample {
            generation,
            stream_id: frame.0,
            revision: frame.1,
            source_ns,
            cycle: self.cycle,
        });
    }
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({"prepared":self.prepared,"evicted":self.evicted,"limit":LIMIT,
            "scope":"Prepared media PTS associations; an observed non-increasing PTS starts a new cycle. No decoded-frame completeness or pixel visibility claim.",
            "samples":self.samples.iter().map(|sample|serde_json::json!({"generation":sample.generation,
                "stream_id":sample.stream_id,"revision":sample.revision,"source_ns":sample.source_ns,
                "cycle":sample.cycle})).collect::<Vec<_>>()})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_associations_keep_prepared_revisions_and_pts_cycles_without_pixels() {
        let mut trace = SourceFrames::default();
        trace.record(7, (11, 1), Duration::from_millis(100));
        trace.record(7, (11, 2), Duration::from_millis(125));
        trace.record(7, (11, 3), Duration::ZERO);
        let state = trace.json();
        assert_eq!(state["samples"][1]["source_ns"], 125_000_000);
        assert_eq!(state["samples"][1]["cycle"], 0);
        assert_eq!(state["samples"][2]["cycle"], 1);
        assert_eq!(state["samples"][2]["generation"], 7);
    }
    #[test]
    fn a_slow_inspector_cannot_retain_an_unbounded_preparation_history() {
        let mut trace = SourceFrames::default();
        for index in 0..LIMIT + 9 {
            trace.record(
                3,
                (4, index as u64 + 1),
                Duration::from_millis(index as u64),
            );
        }
        let state = trace.json();
        assert_eq!(state["samples"].as_array().unwrap().len(), LIMIT);
        assert_eq!(state["evicted"], 9);
        assert_eq!(state["samples"][0]["revision"], 10);
    }
}
