//! Byte-budgeted LRU cache. Strong references pin resources used by live canvases.
use std::{collections::HashMap, rc::Rc};

struct Entry<T> {
    resource: Rc<T>,
    bytes: u64,
    touched: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct CacheStats {
    pub entries: usize,
    pub bytes: u64,
    pub active_bytes: u64,
    pub budget_bytes: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}
pub(super) struct ResourceCache<T> {
    entries: HashMap<String, Entry<T>>,
    clock: u64,
    budget_bytes: u64,
    entry_limit: usize,
    hits: u64,
    misses: u64,
    evictions: u64,
}
impl<T> ResourceCache<T> {
    pub fn new(budget_bytes: u64, entry_limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            budget_bytes,
            entry_limit,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }
    pub fn get(&mut self, key: &str) -> Option<Rc<T>> {
        self.clock = self.clock.saturating_add(1);
        match self.entries.get_mut(key) {
            Some(entry) => {
                entry.touched = self.clock;
                self.hits += 1;
                Some(entry.resource.clone())
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }
    pub fn insert(&mut self, key: String, resource: Rc<T>, bytes: u64) {
        self.clock = self.clock.saturating_add(1);
        self.entries.insert(
            key,
            Entry {
                resource,
                bytes,
                touched: self.clock,
            },
        );
    }
    /// Retire unused resources whose source no longer exists, even below budget.
    /// Active scenes stay pinned regardless of source lifetime or budget;
    /// evicting those resources would cause cross-surface upload thrashing.
    pub fn trim(&mut self, reusable: impl Fn(&T) -> bool) {
        let before = self.entries.len();
        self.entries
            .retain(|_, entry| Rc::strong_count(&entry.resource) > 1 || reusable(&entry.resource));
        self.evictions += (before - self.entries.len()) as u64;
        let mut bytes = self.entries.values().map(|entry| entry.bytes).sum::<u64>();
        while bytes > self.budget_bytes || self.entries.len() > self.entry_limit {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, entry)| Rc::strong_count(&entry.resource) == 1)
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(key, _)| key.clone());
            let Some(key) = oldest else {
                break;
            };
            bytes -= self.entries.remove(&key).unwrap().bytes;
            self.evictions += 1;
        }
    }
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.entries.len(),
            bytes: self.entries.values().map(|entry| entry.bytes).sum(),
            active_bytes: self
                .entries
                .values()
                .filter(|entry| Rc::strong_count(&entry.resource) > 1)
                .map(|entry| entry.bytes)
                .sum(),
            budget_bytes: self.budget_bytes,
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn over_budget_wallpaper_survives_another_surface_and_releases_when_unused() {
        let mut cache = ResourceCache::new(64, 10);
        let wallpaper = Rc::new("wallpaper");
        cache.insert("wallpaper".into(), wallpaper.clone(), 85);
        let bar = Rc::new("bar");
        cache.insert("bar".into(), bar.clone(), 2);
        cache.trim(|_| true);
        assert!(Rc::ptr_eq(&cache.get("wallpaper").unwrap(), &wallpaper));
        assert_eq!(cache.stats().active_bytes, 87);
        assert_eq!(cache.stats().evictions, 0);
        drop(wallpaper);
        cache.trim(|_| true);
        assert!(cache.get("wallpaper").is_none());
        assert!(Rc::ptr_eq(&cache.get("bar").unwrap(), &bar));
        assert_eq!(cache.stats().bytes, 2);
    }
    #[test]
    fn eviction_preserves_recent_unused_resources_within_the_budget() {
        let mut cache = ResourceCache::new(20, 2);
        for key in ["a", "b"] {
            cache.insert(key.into(), Rc::new(key), 10);
        }
        drop(cache.get("a"));
        cache.insert("c".into(), Rc::new("c"), 10);
        cache.trim(|_| true);
        assert!(cache.get("b").is_none());
        assert!(cache.get("a").is_some());
        assert!(cache.get("c").is_some());
        assert_eq!(cache.stats().evictions, 1);
    }
    #[test]
    fn expired_sources_retire_below_budget_only_after_all_draws_release_them() {
        use std::cell::Cell;
        let mut cache = ResourceCache::new(100, 10);
        let image = Rc::new(Cell::new(true));
        let second_surface = image.clone();
        cache.insert("image".into(), image.clone(), 20);
        cache.insert("reusable".into(), Rc::new(Cell::new(true)), 10);
        image.set(false);
        drop(image);
        cache.trim(Cell::get);
        assert_eq!(cache.stats().active_bytes, 20);
        assert_eq!(cache.stats().evictions, 0);
        drop(second_surface);
        cache.trim(Cell::get);
        assert_eq!(cache.stats().bytes, 10);
        assert_eq!(cache.stats().evictions, 1);
        assert!(cache.get("image").is_none());
        assert!(cache.get("reusable").is_some());
    }
}
