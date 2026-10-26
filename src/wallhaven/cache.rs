//! A small least-recently-used cache with a hard entry cap (bounds thumbnail memory).

use std::collections::HashMap;
use std::hash::Hash;

pub struct Lru<K, V> {
    cap: usize,
    tick: u64,
    map: HashMap<K, (V, u64)>,
}

impl<K: Hash + Eq + Clone, V> Lru<K, V> {
    pub fn new(cap: usize) -> Self {
        Self { cap: cap.max(1), tick: 0, map: HashMap::new() }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn contains(&self, k: &K) -> bool {
        self.map.contains_key(k)
    }

    /// Looks up `k` and marks it most recently used.
    pub fn get(&mut self, k: &K) -> Option<&V> {
        self.tick += 1;
        let t = self.tick;
        self.map.get_mut(k).map(|e| {
            e.1 = t;
            &e.0
        })
    }

    /// Inserts `v`, evicting the least recently used entry when over capacity.
    pub fn insert(&mut self, k: K, v: V) {
        self.tick += 1;
        self.map.insert(k, (v, self.tick));
        while self.map.len() > self.cap {
            let oldest = self.map.iter().min_by_key(|(_, (_, t))| *t).map(|(k, _)| k.clone());
            match oldest {
                Some(k) => {
                    self.map.remove(&k);
                }
                None => break,
            }
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used() {
        let mut c = Lru::new(2);
        c.insert("a", 1);
        c.insert("b", 2);
        assert_eq!(c.get(&"a"), Some(&1)); // a is now newer than b
        c.insert("c", 3);
        assert!(!c.contains(&"b"));
        assert!(c.contains(&"a") && c.contains(&"c"));
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn reinsert_refreshes_without_growing() {
        let mut c = Lru::new(2);
        c.insert(1, "x");
        c.insert(1, "y");
        assert_eq!(c.len(), 1);
        assert_eq!(c.get(&1), Some(&"y"));
    }

    #[test]
    fn cap_is_at_least_one() {
        let mut c = Lru::new(0);
        c.insert(1, 1);
        c.insert(2, 2);
        assert_eq!(c.len(), 1);
        assert!(c.contains(&2));
    }

    #[test]
    fn clear_empties() {
        let mut c = Lru::new(3);
        c.insert(1, 1);
        c.clear();
        assert!(c.is_empty());
    }
}
