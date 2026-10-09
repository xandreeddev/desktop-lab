//! Search and selection policy. No compositor, process execution, or rendering.
use lucent_domain::MenuEntry;

pub struct Selection {
    pub entries: Vec<MenuEntry>,
    pub matches: Vec<usize>,
    pub selected: usize,
    searchable: Vec<String>,
}
impl Selection {
    pub fn new(entries: Vec<MenuEntry>) -> Self {
        let searchable = entries
            .iter()
            .map(|e| format!("{} {}", e.label, e.detail).to_lowercase())
            .collect();
        Self {
            matches: (0..entries.len()).collect(),
            entries,
            selected: 0,
            searchable,
        }
    }
    /// All query words must match; retain Omarchy's order and original values.
    pub fn search(&mut self, query: &str) {
        let query = query.to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        self.matches = self
            .searchable
            .iter()
            .enumerate()
            .filter(|(_, text)| words.iter().all(|word| text.contains(word)))
            .map(|(i, _)| i)
            .collect();
        self.selected = 0;
    }
    pub fn navigate(&mut self, delta: i32) {
        self.selected = (self.selected as i64 + i64::from(delta))
            .clamp(0, self.matches.len().saturating_sub(1) as i64) as usize;
    }
    pub fn choose(&self) -> Option<String> {
        self.matches
            .get(self.selected)
            .map(|i| self.entries[*i].value.clone())
    }
    /// Keep the selected row visible without moving earlier rows unnecessarily.
    pub fn start(&self, previous: usize, visible: usize) -> usize {
        previous
            .min(self.selected)
            .max((self.selected + 1).saturating_sub(visible.max(1)))
            .min(self.matches.len().saturating_sub(visible))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_keeps_duplicate_labels_distinct_and_matches_shortcuts() {
        let mut s = Selection::new(vec![
            MenuEntry {
                label: "Terminal".into(),
                detail: "Super Return".into(),
                value: "first".into(),
            },
            MenuEntry {
                label: "Terminal".into(),
                detail: "Super Alt Return".into(),
                value: "second".into(),
            },
        ]);
        s.search("TERMINAL alt");
        assert_eq!(s.choose().as_deref(), Some("second"));
        s.search("missing");
        s.navigate(1);
        assert_eq!(s.choose(), None);
        s.search("");
        s.navigate(500);
        assert_eq!(s.choose().as_deref(), Some("second"));
        assert_eq!(s.start(0, 1), 1);
        s.navigate(-500);
        assert_eq!(s.start(1, 1), 0);
    }
}
