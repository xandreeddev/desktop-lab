//! Pure bounded inbox and timeout policy; time is provided by the adapter.
use lucent_domain::*;
pub mod body;
use std::collections::{BTreeMap, VecDeque};
pub const ACTIVE_LIMIT: usize = 64;
pub const HISTORY_LIMIT: usize = 100;
pub const BODY_LIMIT: usize = 4096;
#[derive(Default)]
pub struct NotificationInbox {
    next_id: u32,
    revision: u64,
    active: BTreeMap<u32, (Notification, Option<u64>)>,
    history: VecDeque<Notification>,
    dnd: bool,
}
impl NotificationInbox {
    /// Returns the assigned ID and any old notification closed to enforce the bound.
    pub fn receive(&mut self, mut note: Notification, now: u64) -> Result<(u32, Option<u32>)> {
        let revision = self.revision.checked_add(1).ok_or_else(|| {
            DomainError::Unavailable("Notification revision limit reached".into())
        })?;
        validate_notification(&note, false)?;
        self.revision = revision;
        note.revision = revision;
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
            let first = *self
                .active
                .iter()
                .min_by_key(|(_, (note, _))| note.revision)
                .unwrap()
                .0;
            self.close(first);
            Some(first)
        } else {
            None
        };
        self.active.insert(id, (note, deadline));
        Ok((id, removed))
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
    pub fn matches(&self, target: NotificationTarget) -> bool {
        self.get(target.id())
            .is_some_and(|note| note.revision == target.revision())
    }
    pub fn archive(&self) -> NotificationArchive {
        let mut entries: Vec<_> = self
            .active
            .values()
            .map(|(note, _)| note)
            .chain(self.history.iter())
            .filter(|n| !n.transient)
            .cloned()
            .collect();
        entries.sort_by_key(|note| std::cmp::Reverse(note.revision));
        entries.truncate(HISTORY_LIMIT);
        NotificationArchive {
            entries,
            do_not_disturb: self.dnd,
            last_revision: self.revision,
        }
    }
    pub fn restore(&mut self, archive: NotificationArchive) -> Result<()> {
        validate_archive(&archive)?;
        if !self.active.is_empty() {
            return Err(DomainError::Invalid(
                "Cannot replace a live notification inbox".into(),
            ));
        }
        self.revision = archive.last_revision;
        self.dnd = archive.do_not_disturb;
        self.history = archive.entries.into();
        Ok(())
    }
    /// Explicit recovery preserves live versions and newer local preferences.
    pub fn merge_archive(
        &mut self,
        archive: NotificationArchive,
        preserve_dnd: bool,
    ) -> Result<()> {
        validate_archive(&archive)?;
        let mut revision = self.revision.max(archive.last_revision);
        let mut entries = self.history.clone();
        let mut used: std::collections::BTreeSet<_> = self
            .active
            .values()
            .map(|(n, _)| n.revision)
            .chain(entries.iter().map(|n| n.revision))
            .collect();
        for mut note in archive.entries {
            if entries.iter().any(|n| n == &note) || self.active.values().any(|(n, _)| n == &note) {
                continue;
            }
            if !used.insert(note.revision) {
                revision = revision.checked_add(1).ok_or_else(|| {
                    DomainError::Unavailable("Notification revision limit reached".into())
                })?;
                note.revision = revision;
                used.insert(revision);
            }
            entries.push_back(note);
        }
        let mut entries: Vec<_> = entries.into_iter().collect();
        entries.sort_by_key(|n| std::cmp::Reverse((n.received_unix_ms, n.revision)));
        entries.truncate(HISTORY_LIMIT);
        self.history = entries.into();
        self.revision = revision;
        if !preserve_dnd {
            self.dnd = archive.do_not_disturb;
        }
        Ok(())
    }
    /// Retire transport authority without replaying close/action signals on a new bus.
    pub fn retire_active(&mut self) {
        let ids: Vec<_> = self.active.keys().copied().collect();
        for id in ids {
            self.close(id);
        }
    }
    pub fn snapshot(&self) -> NotificationSnapshot {
        let mut active: Vec<_> = self.active.values().map(|(n, _)| n.clone()).collect();
        active.sort_by_key(|n| std::cmp::Reverse(n.revision));
        NotificationSnapshot {
            active,
            history: self.history.iter().cloned().collect(),
            do_not_disturb: self.dnd,
            history_status: NotificationHistoryStatus::Memory,
        }
    }
}
pub fn validate_notification(note: &Notification, assigned: bool) -> Result<()> {
    let invalid = (assigned && (note.id == 0 || note.revision == 0))
        || note.app.len() > 128
        || note.summary.len() > 256
        || note.body.len() > BODY_LIMIT
        || note.actions.len() > 8
        || note
            .actions
            .iter()
            .any(|a| a.id.is_empty() || a.id.len() > 128 || a.label.len() > 64);
    if invalid {
        return Err(DomainError::Invalid(
            "Notification exceeds supported limits".into(),
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    if note.actions.iter().any(|a| !ids.insert(&a.id)) {
        return Err(DomainError::Invalid(
            "Notification action identity is duplicated".into(),
        ));
    }
    Ok(())
}
pub fn validate_archive(archive: &NotificationArchive) -> Result<()> {
    if archive.entries.len() > HISTORY_LIMIT {
        return Err(DomainError::Invalid(
            "Notification archive exceeds supported limits".into(),
        ));
    }
    let mut revisions = std::collections::BTreeSet::new();
    for note in &archive.entries {
        validate_notification(note, true)?;
        if note.transient
            || note.revision > archive.last_revision
            || !revisions.insert(note.revision)
        {
            return Err(DomainError::Invalid(
                "Notification archive identity is invalid".into(),
            ));
        }
    }
    Ok(())
}
/// Pure grouping policy. The desktop chooses labels, layout and collapse state.
pub fn notification_group(note: &Notification) -> String {
    note.desktop_entry.as_ref().map_or_else(
        || format!("name:{}", note.app),
        |id| format!("desktop:{}", id.as_str()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn note(id: u32, timeout_ms: i32) -> Notification {
        Notification {
            id,
            revision: 0,
            received_unix_ms: 0,
            desktop_entry: None,
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
        let (id, _) = inbox.receive(note(0, 100), 0).unwrap();
        let (replaced, _) = inbox.receive(note(id, 200), 50).unwrap();
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
        let (id, _) = inbox.receive(n, 0).unwrap();
        assert!(inbox.due(u64::MAX).is_empty());
        inbox.close(id);
        for _ in 0..150 {
            let (id, _) = inbox.receive(note(0, 0), 0).unwrap();
            inbox.close(id);
        }
        assert_eq!(inbox.snapshot().history.len(), 100);
        for _ in 0..100 {
            inbox.receive(note(0, 0), 0).unwrap();
        }
        assert_eq!(inbox.snapshot().active.len(), 64);
    }
    #[test]
    fn transient_notifications_leave_no_history() {
        let mut inbox = NotificationInbox::default();
        let mut n = note(0, 0);
        n.transient = true;
        let (id, _) = inbox.receive(n, 0).unwrap();
        assert!(inbox.close(id));
        assert!(!inbox.close(id));
        assert!(inbox.snapshot().history.is_empty());
    }
}

#[cfg(test)]
mod archive_tests {
    use super::*;
    fn note(id: u32) -> Notification {
        Notification {
            id,
            revision: 0,
            received_unix_ms: 10,
            desktop_entry: None,
            app: "Fixture".into(),
            summary: "Fixture event".into(),
            body: String::new(),
            actions: vec![NotificationAction {
                id: "open".into(),
                label: "Open".into(),
            }],
            critical: false,
            resident: false,
            transient: false,
            timeout_ms: 0,
        }
    }
    #[test]
    fn replacements_retire_old_targets_and_become_the_newest_item() {
        let mut inbox = NotificationInbox::default();
        let (old, _) = inbox.receive(note(0), 0).unwrap();
        let captured = inbox.get(old).unwrap().target().unwrap();
        let (other, _) = inbox.receive(note(0), 0).unwrap();
        inbox.receive(note(old), 0).unwrap();
        assert!(!inbox.matches(captured));
        assert_eq!(inbox.snapshot().active[0].id, old);
        assert!(inbox.get(other).is_some());
        assert!(inbox.matches(inbox.get(old).unwrap().target().unwrap()));
        assert!(NotificationTarget::new(0, 1).is_err());
        assert!(NotificationTarget::new(1, 0).is_err());
    }
    #[test]
    fn restored_entries_are_history_without_live_action_authority() {
        let mut inbox = NotificationInbox::default();
        let (id, _) = inbox.receive(note(0), 0).unwrap();
        let target = inbox.get(id).unwrap().target().unwrap();
        inbox.dnd(true);
        let archive = inbox.archive();
        let mut restored = NotificationInbox::default();
        restored.restore(archive.clone()).unwrap();
        assert!(restored.snapshot().active.is_empty());
        assert_eq!(restored.snapshot().history, archive.entries);
        assert!(restored.snapshot().do_not_disturb);
        assert!(!restored.matches(target));
        let (new, _) = restored.receive(note(0), 0).unwrap();
        assert_ne!(restored.get(new).unwrap().target(), Some(target));
        assert!(restored.get(new).unwrap().revision > archive.last_revision);
    }
    #[test]
    fn archives_exclude_transient_bound_payload_and_reject_duplicate_versions() {
        let mut inbox = NotificationInbox::default();
        for _ in 0..150 {
            let (id, _) = inbox.receive(note(0), 0).unwrap();
            inbox.close(id);
        }
        let mut temporary = note(0);
        temporary.transient = true;
        inbox.receive(temporary, 0).unwrap();
        let archive = inbox.archive();
        assert_eq!(archive.entries.len(), HISTORY_LIMIT);
        assert!(archive.entries.iter().all(|n| !n.transient));
        let mut bad = archive.clone();
        bad.entries[0].body = "x".repeat(BODY_LIMIT + 1);
        assert!(validate_archive(&bad).is_err());
        bad = archive.clone();
        bad.entries[1].revision = bad.entries[0].revision;
        assert!(validate_archive(&bad).is_err());
        let mut invalid = note(0);
        invalid.actions.push(invalid.actions[0].clone());
        let before = inbox.snapshot();
        assert!(inbox.receive(invalid, 0).is_err());
        assert_eq!(inbox.snapshot(), before);
    }
    #[test]
    fn archive_merge_keeps_live_targets_and_newer_local_dnd() {
        let mut old = NotificationInbox::default();
        let (id, _) = old.receive(note(0), 0).unwrap();
        old.close(id);
        old.dnd(true);
        let archive = old.archive();
        let mut current = NotificationInbox::default();
        let mut newer = note(0);
        newer.received_unix_ms = 20;
        let (id, _) = current.receive(newer, 0).unwrap();
        let captured = current.get(id).unwrap().target().unwrap();
        current.merge_archive(archive, true).unwrap();
        assert!(current.matches(captured));
        assert!(!current.snapshot().do_not_disturb);
        assert_eq!(current.snapshot().history.len(), 1);
        assert_ne!(current.snapshot().history[0].revision, captured.revision());
        let (next, _) = current.receive(note(0), 0).unwrap();
        assert!(current.get(next).unwrap().revision > current.snapshot().history[0].revision);
    }
    #[test]
    fn group_identity_prefers_desktop_entry_and_keeps_names_separate() {
        let mut a = note(1);
        let mut b = a.clone();
        b.app = "Another display label".into();
        assert_ne!(notification_group(&a), notification_group(&b));
        a.desktop_entry = Some(AppId::new("org.example.Fixture.desktop").unwrap());
        b.desktop_entry = a.desktop_entry.clone();
        assert_eq!(notification_group(&a), notification_group(&b));
    }
}

#[cfg(test)]
mod recovery_identity_tests {
    use super::*;
    #[test]
    fn recovering_an_already_live_saved_entry_does_not_duplicate_or_rekey_it() {
        let mut inbox = NotificationInbox::default();
        let note = Notification {
            id: 0,
            revision: 0,
            received_unix_ms: 1,
            desktop_entry: None,
            app: "Fixture".into(),
            summary: "Current receipt".into(),
            body: String::new(),
            actions: vec![],
            critical: false,
            resident: false,
            transient: false,
            timeout_ms: 0,
        };
        let (id, _) = inbox.receive(note, 0).unwrap();
        let captured = inbox.get(id).unwrap().target().unwrap();
        let archive = inbox.archive();
        inbox.merge_archive(archive, true).unwrap();
        assert!(inbox.matches(captured));
        assert!(inbox.snapshot().history.is_empty());
        assert_eq!(inbox.archive().entries.len(), 1);
    }
}
