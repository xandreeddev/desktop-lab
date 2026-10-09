//! Notifications are data and user actions; delivery/rendering belong to adapters.
use crate::{Result, StopSignal};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationAction {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<NotificationAction>,
    pub critical: bool,
    pub resident: bool,
    pub transient: bool,
    pub timeout_ms: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    Requested = 3,
    Undefined = 4,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationSnapshot {
    pub active: Vec<Notification>,
    pub history: Vec<Notification>,
    pub do_not_disturb: bool,
}
pub trait NotificationPort: Send + Sync {
    fn watch(&self, emit: &mut dyn FnMut(Result<NotificationSnapshot>), stop: &dyn StopSignal);
    fn dismiss(&self, id: u32) -> Result<()>;
    fn invoke(&self, id: u32, action: &str) -> Result<()>;
    fn set_do_not_disturb(&self, enabled: bool) -> Result<()>;
    fn clear_history(&self) -> Result<()>;
}
