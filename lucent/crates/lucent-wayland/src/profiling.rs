//! Bounded, opt-in history for successful frame submissions. Durations are CPU-side
//! wall time, including any blocking driver work, not GPU timestamps or scanout.
use lucent_render::RenderProfile;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub const MAX_FRAMES: usize = 120;
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameProfile {
    pub frame: u64,
    pub submitted_ms: f64,
    pub view_ms: f64,
    pub layout_ms: f64,
    pub input_ms: f64,
    pub bookkeeping_ms: f64,
    pub total_ms: f64,
    pub render: RenderProfile,
}
impl FrameProfile {
    pub fn json(&self) -> serde_json::Value {
        let r = self.render;
        serde_json::json!({
            "frame":self.frame, "submitted_ms":self.submitted_ms, "total_ms":self.total_ms, "renderer_ms":millis(r.total),
            "stages_ms": {"view":self.view_ms, "layout":self.layout_ms, "input":self.input_ms,
                "configure":millis(r.configure), "acquire":millis(r.acquire),
                "prepare":millis(r.paint.prepare), "encode":millis(r.paint.encode),
                "submit":millis(r.paint.submit), "present":millis(r.present),
                "retire":millis(r.paint.retire), "bookkeeping":self.bookkeeping_ms},
            "prepare_breakdown_ms":{"text_raster":millis(r.paint.resources.text_raster),
                "texture_upload":millis(r.paint.resources.texture_upload)},
            "resources":{"text_misses":r.paint.resources.text_misses,
                "image_misses":r.paint.resources.image_misses,
                "texture_upload_bytes":r.paint.resources.texture_upload_bytes}
        })
    }
}
#[derive(Default)]
pub struct History(VecDeque<FrameProfile>);
impl History {
    pub fn push(&mut self, value: FrameProfile) {
        if self.0.len() == MAX_FRAMES {
            self.0.pop_front();
        }
        self.0.push_back(value);
    }
    pub fn json(&self) -> Vec<serde_json::Value> {
        self.0.iter().map(FrameProfile::json).collect()
    }
}
fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.
}
pub fn elapsed(start: Option<Instant>) -> f64 {
    start.map_or(0., |time| millis(time.elapsed()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_is_bounded_and_nested_work_is_distinct_from_stage_totals() {
        let mut history = History::default();
        assert!(history.json().is_empty());
        for frame in 0..MAX_FRAMES as u64 + 10 {
            let mut sample = FrameProfile {
                frame,
                ..Default::default()
            };
            sample.render.paint.prepare = Duration::from_millis(8);
            sample.render.paint.resources.text_raster = Duration::from_millis(3);
            history.push(sample);
        }
        let values = history.json();
        assert_eq!(values.len(), MAX_FRAMES);
        assert_eq!(values[0]["frame"], 10);
        assert_eq!(values.last().unwrap()["frame"], MAX_FRAMES + 9);
        assert_eq!(values[0]["stages_ms"]["prepare"], 8.);
        assert_eq!(values[0]["prepare_breakdown_ms"]["text_raster"], 3.);
        assert_eq!(elapsed(None), 0.);
    }
}
