//! Byte-bounded in-RAM caches (issue #106).
//!
//! The app keeps rendered bodies and fetched attachments in memory so reopening
//! a message is instant. Both used to live in plain `HashMap`s that only ever
//! grew: the background body prefetch alone feeds in 50 bodies per folder per
//! sync, and a body with inline images can run to tens of megabytes — after a
//! day of use the main process held hundreds of megabytes of mail it would
//! never show again. Everything cached here also lives in the SQLite cache, so
//! eviction costs at most a disk read.
//!
//! Eviction is oldest-inserted-first under a byte budget. That is deliberately
//! not a strict LRU: a touched-on-get order would need `&mut` at every read
//! site for little gain, since the pressure comes from the prefetch conveyor
//! writing new entries, not from re-reads of old ones.

use std::collections::{HashMap, VecDeque};

/// What an entry costs the budget.
pub(crate) trait Weigh {
    fn weight(&self) -> usize;
}

impl Weigh for String {
    fn weight(&self) -> usize {
        self.len()
    }
}

impl Weigh for Vec<crate::models::Attachment> {
    fn weight(&self) -> usize {
        self.iter().map(|a| a.data.len() + a.name.len()).sum()
    }
}

/// A map that stays under a byte budget by dropping its oldest entries. The
/// key says which message an entry belongs to; see its users for the shape.
pub(crate) struct RamCache<K, V: Weigh> {
    map: HashMap<K, V>,
    /// Insertion order, oldest first — the eviction queue.
    order: VecDeque<K>,
    bytes: usize,
    budget: usize,
}

impl<K: std::hash::Hash + Eq + Clone, V: Weigh> RamCache<K, V> {
    pub(crate) fn new(budget: usize) -> Self {
        RamCache {
            map: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            budget,
        }
    }

    /// How many entries are held.
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }

    /// What the entries weigh together, in bytes.
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// The byte budget eviction keeps the cache under.
    pub(crate) fn budget(&self) -> usize {
        self.budget
    }

    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub(crate) fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        self.bytes += value.weight();
        if let Some(old) = self.map.insert(key.clone(), value) {
            self.bytes -= old.weight();
            self.order.retain(|k| k != &key);
        }
        self.order.push_back(key);
        // Keep at least the newest entry even when it alone busts the budget:
        // it is almost certainly the message on screen, and evicting it would
        // only force a disk re-read on the next render.
        while self.bytes > self.budget && self.order.len() > 1 {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(v) = self.map.remove(&oldest) {
                self.bytes -= v.weight();
            }
        }
    }

    pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
        let v = self.map.remove(key)?;
        self.bytes -= v.weight();
        self.order.retain(|k| k != key);
        Some(v)
    }

    pub(crate) fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(n: usize) -> String {
        "x".repeat(n)
    }

    #[test]
    fn evicts_oldest_first_under_the_budget() {
        let mut c = RamCache::new(100);
        c.insert((1, 1), body(40));
        c.insert((1, 2), body(40));
        c.insert((1, 3), body(40)); // 120 bytes: (1,1) must go
        assert!(!c.contains_key(&(1, 1)));
        assert!(c.contains_key(&(1, 2)));
        assert!(c.contains_key(&(1, 3)));
    }

    #[test]
    fn replacing_an_entry_reclaims_its_old_weight() {
        let mut c = RamCache::new(100);
        c.insert((1, 1), body(80));
        c.insert((1, 1), body(10)); // shrunk in place, not double-counted
        c.insert((1, 2), body(80));
        assert!(c.contains_key(&(1, 1)), "10 + 80 fits the budget");
    }

    #[test]
    fn an_oversized_newest_entry_survives_alone() {
        let mut c = RamCache::new(100);
        c.insert((1, 1), body(40));
        c.insert((1, 2), body(500));
        assert!(!c.contains_key(&(1, 1)));
        assert!(c.contains_key(&(1, 2)), "the newest entry is never evicted");
    }

    #[test]
    fn clear_resets_the_budget_accounting() {
        let mut c = RamCache::new(100);
        c.insert((1, 1), body(90));
        c.clear();
        c.insert((1, 2), body(90));
        assert!(c.contains_key(&(1, 2)));
        assert_eq!(c.get(&(1, 2)).map(String::len), Some(90));
    }
}
