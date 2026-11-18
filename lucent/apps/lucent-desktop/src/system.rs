//! System observation status is presentation state, separate from the last sample.
use lucent_domain::{Result, SystemSnapshot};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Loading,
    Ready,
    Unavailable,
}
#[derive(Default)]
pub struct System {
    snapshot: Option<SystemSnapshot>,
    status: Status,
    error: String,
}
impl System {
    pub fn snapshot(&self) -> Option<&SystemSnapshot> {
        self.snapshot.as_ref()
    }
    pub fn status(&self) -> Status {
        self.status
    }
    /// Return whether the widget's presentation changed. Repeated errors retain
    /// diagnostics without causing redraws for text the widget does not display.
    pub fn observe(&mut self, result: Result<SystemSnapshot>) -> bool {
        match result.and_then(|sample| sample.validate().map(|()| sample)) {
            Ok(sample) => {
                let changed =
                    self.status != Status::Ready || self.snapshot.as_ref() != Some(&sample);
                self.status = Status::Ready;
                self.snapshot = Some(sample);
                self.error.clear();
                changed
            }
            Err(error) => {
                let changed = self.status != Status::Unavailable;
                self.status = Status::Unavailable;
                self.error = error.to_string();
                changed
            }
        }
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({
            "status": match self.status {
                Status::Loading => "loading",
                Status::Ready => "ready",
                Status::Unavailable => "unavailable",
            },
            "error": self.error,
            "last_sample": self.snapshot.as_ref().map(|sample| serde_json::json!({
                "cpu_percent": sample.cpu_percent,
                "memory_used_mib": sample.memory_used_mib,
                "memory_total_mib": sample.memory_total_mib,
            })),
        })
    }
}
