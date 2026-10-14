//! Notifications are data and user actions; delivery/rendering belong to adapters.
use crate::{Result, StopSignal};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationAction {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    pub id: u32,
    pub revision: u64,
    pub received_unix_ms: u64,
    pub desktop_entry: Option<crate::AppId>,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<NotificationAction>,
    pub critical: bool,
    pub resident: bool,
    pub transient: bool,
    pub timeout_ms: i32,
}
/// A current received version, distinct from the sender's replaceable wire ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NotificationTarget {
    id: u32,
    revision: u64,
}
impl NotificationTarget {
    pub fn new(id: u32, revision: u64) -> Result<Self> {
        if id == 0 || revision == 0 {
            return Err(crate::DomainError::Invalid(
                "Notification target is invalid".into(),
            ));
        }
        Ok(Self { id, revision })
    }
    pub fn id(self) -> u32 {
        self.id
    }
    pub fn revision(self) -> u64 {
        self.revision
    }
}
impl Notification {
    pub fn target(&self) -> Option<NotificationTarget> {
        NotificationTarget::new(self.id, self.revision).ok()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationArchive {
    pub entries: Vec<Notification>,
    pub do_not_disturb: bool,
    pub last_revision: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotificationHistoryStatus {
    #[default]
    Memory,
    Ready,
    ReadFailed,
    WriteFailed,
}
/// Storage adapter boundary; the domain does not choose files or formats.
pub trait NotificationHistoryPort: Send + Sync {
    fn load(&self) -> Result<NotificationArchive>;
    fn save(&self, archive: &NotificationArchive) -> Result<()>;
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
    pub history_status: NotificationHistoryStatus,
}
pub trait NotificationPort: Send + Sync {
    fn watch(&self, emit: &mut dyn FnMut(Result<NotificationSnapshot>), stop: &dyn StopSignal);
    fn dismiss(&self, target: NotificationTarget) -> Result<()>;
    fn invoke(&self, target: NotificationTarget, action: &str) -> Result<()>;
    fn set_do_not_disturb(&self, enabled: bool) -> Result<()>;
    fn clear_history(&self) -> Result<()>;
    fn retry_history(&self) -> Result<()>;
}
