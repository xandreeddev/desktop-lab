//! Borrowed grouped projection. No mutation, persistence or renderer operations.
use super::Center;
use lucent_domain::Notification;

pub(super) struct Group<'a> {
    pub(super) key: String,
    pub(super) label: &'a str,
    pub(super) entries: Vec<(&'a Notification, bool)>,
}
impl Center {
    pub(super) fn groups(&self) -> Vec<Group<'_>> {
        let mut groups = std::collections::BTreeMap::<String, Group<'_>>::new();
        for (note, history) in self
            .snapshot
            .active
            .iter()
            .map(|n| (n, false))
            .chain(self.snapshot.history.iter().map(|n| (n, true)))
        {
            let key = lucent_usecases::notifications::notification_group(note);
            groups
                .entry(key.clone())
                .or_insert_with(|| Group {
                    key,
                    label: &note.app,
                    entries: vec![],
                })
                .entries
                .push((note, history));
        }
        let mut groups: Vec<_> = groups.into_values().collect();
        for group in &mut groups {
            group
                .entries
                .sort_by_key(|(n, _)| std::cmp::Reverse((n.received_unix_ms, n.revision)));
        }
        groups.sort_by_key(|group| {
            std::cmp::Reverse(
                group
                    .entries
                    .first()
                    .map(|(n, _)| (n.received_unix_ms, n.revision)),
            )
        });
        groups
    }
}
