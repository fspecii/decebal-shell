//! A small bounded LRU. Bounded by entry count and by a caller-defined weight (bytes for pixmaps), so a launcher
//! with hundreds of apps or a magnifying dock cannot grow without limit.

use std::collections::HashMap;
use std::hash::Hash;

pub struct Lru<K, V> {
    map: HashMap<K, (V, u64, usize)>,
    tick: u64,
    weight: usize,
    max_entries: usize,
    max_weight: usize,
}

impl<K: Hash + Eq + Clone, V> Lru<K, V> {
    pub fn new(max_entries: usize, max_weight: usize) -> Self {
        Lru { map: HashMap::new(), tick: 0, weight: 0, max_entries: max_entries.max(1), max_weight: max_weight.max(1) }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Total weight of the entries held.
    pub fn weight(&self) -> usize {
        self.weight
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.weight = 0;
    }

    pub fn contains(&self, k: &K) -> bool {
        self.map.contains_key(k)
    }

    /// The value for `k`, marking it as just used.
    pub fn get(&mut self, k: &K) -> Option<&V> {
        self.tick += 1;
        let t = self.tick;
        self.map.get_mut(k).map(|e| {
            e.1 = t;
            &e.0
        })
    }

    /// Stores `v` (weighing `weight`), then evicts the least recently used entries until both bounds hold.
    /// The entry just stored is never the one evicted.
    pub fn put(&mut self, k: K, v: V, weight: usize) -> &V {
        self.tick += 1;
        if let Some(old) = self.map.insert(k.clone(), (v, self.tick, weight)) {
            self.weight -= old.2;
        }
        self.weight += weight;
        while (self.map.len() > self.max_entries || self.weight > self.max_weight) && self.map.len() > 1 {
            let oldest = self.map.iter().filter(|(key, _)| **key != k).min_by_key(|(_, e)| e.1).map(|(key, _)| key.clone());
            let Some(key) = oldest else { break };
            if let Some(e) = self.map.remove(&key) {
                self.weight -= e.2;
            }
        }
        &self.map[&k].0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_bound_evicts_the_least_recently_used() {
        let mut c: Lru<u32, u32> = Lru::new(3, usize::MAX);
        for i in 0..3 {
            c.put(i, i * 10, 1);
        }
        assert_eq!(c.get(&0), Some(&0), "touch 0: now 1 is the oldest");
        c.put(3, 30, 1);
        assert_eq!(c.len(), 3);
        assert!(!c.contains(&1) && c.contains(&0) && c.contains(&2) && c.contains(&3));
    }

    #[test]
    fn weight_bound_holds_and_the_new_entry_survives() {
        let mut c: Lru<&str, Vec<u8>> = Lru::new(100, 100);
        c.put("a", vec![0; 40], 40);
        c.put("b", vec![0; 40], 40);
        c.put("c", vec![0; 40], 40);
        assert!(c.weight() <= 100 && !c.contains(&"a"), "{}", c.weight());
        c.put("huge", vec![0; 500], 500);
        assert!(c.contains(&"huge"), "an oversized entry is kept alone rather than dropped");
        assert_eq!(c.len(), 1);
        assert_eq!(c.weight(), 500);
    }

    #[test]
    fn replacing_a_key_adjusts_the_weight_and_a_million_puts_stay_bounded() {
        let mut c: Lru<u32, u32> = Lru::new(8, usize::MAX);
        c.put(1, 1, 5);
        c.put(1, 2, 7);
        assert_eq!((c.len(), c.weight()), (1, 7));
        for i in 0..20_000u32 {
            c.put(i, i, 1);
            assert!(c.len() <= 8);
        }
        assert_eq!(c.weight(), 8);
        c.clear();
        assert!(c.is_empty() && c.weight() == 0);
    }
}
