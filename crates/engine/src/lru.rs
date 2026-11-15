//! A least-recently-used store bounded by bytes and by entries.

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone)]
pub struct ByteLruCache<K, V> {
    pub entries: HashMap<K, (V, usize, u64)>,
    pub bytes: usize,
    max_bytes: usize,
    max_entries: usize,
    stamp: u64,
}

impl<K: Copy + Eq + Hash, V> ByteLruCache<K, V> {
    pub fn new(max_bytes: usize, max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            max_bytes,
            max_entries,
            stamp: 0,
        }
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.stamp = self.stamp.wrapping_add(1);
        let entry = self.entries.get_mut(key)?;
        entry.2 = self.stamp;
        Some(&entry.0)
    }

    pub fn insert(&mut self, key: K, value: V, cost: usize) -> bool {
        if cost > self.max_bytes {
            return false;
        }
        if let Some((_, old_cost, _)) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(old_cost);
        }
        while self.bytes.saturating_add(cost) > self.max_bytes
            || self.entries.len() >= self.max_entries
        {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, _, used))| *used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some((_, old_cost, _)) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(old_cost);
            }
        }
        self.stamp = self.stamp.wrapping_add(1);
        self.entries.insert(key, (value, cost, self.stamp));
        self.bytes = self.bytes.saturating_add(cost);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_cache_reuses_a_hit_and_evicts_the_oldest() {
        let mut cache = ByteLruCache::new(10, 512);
        assert!(cache.insert(1, "one", 4));
        assert!(cache.insert(2, "two", 4));
        assert_eq!(cache.get(&1), Some(&"one"));
        assert!(cache.insert(3, "three", 4));
        assert!(cache.get(&2).is_none());
        assert_eq!(cache.get(&1), Some(&"one"));
        assert_eq!(cache.get(&3), Some(&"three"));
        assert_eq!(cache.bytes, 8);
    }

    #[test]
    fn byte_cache_refuses_one_entry_larger_than_its_bound() {
        let mut cache = ByteLruCache::new(7, 512);
        assert!(!cache.insert(1, "too large", 8));
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
    }
}
