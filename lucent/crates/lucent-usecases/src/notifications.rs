//! Pure bounded inbox and timeout policy; time is provided by the adapter.
use lucent_domain::*;
use std::collections::{BTreeMap, VecDeque};
const ACTIVE_LIMIT: usize = 64;
const HISTORY_LIMIT: usize = 100;
#[derive(Default)]
pub struct NotificationInbox {
    next_id: u32,
    active: BTreeMap<u32, (Notification, Option<u64>)>,
    history: VecDeque<Notification>,
    dnd: bool,
}
impl NotificationInbox {
    /// Returns the assigned ID and any old notification closed to enforce the bound.
    pub fn receive(&mut self, mut note: Notification, now: u64) -> (u32, Option<u32>) {
        if note.id == 0 || !self.active.contains_key(&note.id) {
            loop {
                self.next_id = self.next_id.wrapping_add(1).max(1);
                if !self.active.contains_key(&self.next_id) {
                    break;
                }
            }
            note.id = self.next_id;
        }
        let id = note.id;
        let deadline = match note.timeout_ms {
            0 => None,
            n if n > 0 => Some(now.saturating_add(n as u64)),
            _ if note.critical => None,
            _ => Some(now.saturating_add(5000)),
        };
        let removed = if self.active.len() >= ACTIVE_LIMIT && !self.active.contains_key(&id) {
            let first = *self.active.keys().next().unwrap();
            self.close(first);
            Some(first)
        } else {
            None
        };
        self.active.insert(id, (note, deadline));
        (id, removed)
    }
    pub fn close(&mut self, id: u32) -> bool {
        if let Some((note, _)) = self.active.remove(&id) {
            if !note.transient {
                self.history.push_front(note);
                self.history.truncate(HISTORY_LIMIT);
            }
            true
        } else {
            false
        }
    }
    pub fn due(&self, now: u64) -> Vec<u32> {
        self.active
            .iter()
            .filter(|(_, (_, deadline))| deadline.is_some_and(|d| d <= now))
            .map(|(id, _)| *id)
            .collect()
    }
    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.active.get(&id).map(|(n, _)| n)
    }
    pub fn dnd(&mut self, enabled: bool) {
        self.dnd = enabled;
    }
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
    pub fn snapshot(&self) -> NotificationSnapshot {
        NotificationSnapshot {
            active: self.active.values().rev().map(|(n, _)| n.clone()).collect(),
            history: self.history.iter().cloned().collect(),
            do_not_disturb: self.dnd,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn note(id: u32, timeout_ms: i32) -> Notification {
        Notification {
            id,
            app: "Fixture".into(),
            summary: "Hello".into(),
            body: String::new(),
            actions: vec![],
            critical: false,
            resident: false,
            transient: false,
            timeout_ms,
        }
    }
    #[test]
    fn replacement_preserves_identity_and_refreshes_deadline() {
        let mut inbox = NotificationInbox::default();
        let (id, _) = inbox.receive(note(0, 100), 0);
        let (replaced, _) = inbox.receive(note(id, 200), 50);
        assert_eq!(id, replaced);
        assert!(inbox.due(100).is_empty());
        assert_eq!(inbox.due(250), vec![id]);
        assert_eq!(inbox.snapshot().active.len(), 1);
    }
    #[test]
    fn critical_and_persistent_do_not_expire_and_history_is_bounded() {
        let mut inbox = NotificationInbox::default();
        let mut n = note(0, -1);
        n.critical = true;
        let (id, _) = inbox.receive(n, 0);
        assert!(inbox.due(u64::MAX).is_empty());
        inbox.close(id);
        for _ in 0..150 {
            let (id, _) = inbox.receive(note(0, 0), 0);
            inbox.close(id);
        }
        assert_eq!(inbox.snapshot().history.len(), 100);
        for _ in 0..100 {
            inbox.receive(note(0, 0), 0);
        }
        assert_eq!(inbox.snapshot().active.len(), 64);
    }
    #[test]
    fn transient_notifications_leave_no_history() {
        let mut inbox = NotificationInbox::default();
        let mut n = note(0, 0);
        n.transient = true;
        let (id, _) = inbox.receive(n, 0);
        assert!(inbox.close(id));
        assert!(!inbox.close(id));
        assert!(inbox.snapshot().history.is_empty());
    }
}
