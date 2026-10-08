//! The one level cache: answered words, so a revisited page re-derives
//! from memory.

use std::collections::{HashMap, HashSet};

/// The bound, in entries; past it the cache clears whole, like the
/// spot memo.
pub const LEVEL_CACHE_CAP: usize = 8_192;

/// The band a dataset answer stands for: misses answer 0, and 1..=6 is clamped.
pub fn band_of(level: Option<f64>) -> u8 {
    level
        .map(|level| level.round().clamp(1.0, 6.0) as u8)
        .unwrap_or(0)
}

/// Words answered by the dataset: key -> band; misses remember 0.
#[derive(Clone, Default)]
pub struct LevelCache(HashMap<Box<str>, u8>);

impl LevelCache {
    /// The memoized band for `key`, or `None` when it was never asked.
    pub fn get(&self, key: &str) -> Option<u8> {
        self.0.get(key).copied()
    }

    /// Remember `band` for `key`, clearing whole at the cap.
    pub fn insert(&mut self, key: &str, band: u8) {
        if self.0.len() >= LEVEL_CACHE_CAP && !self.0.contains_key(key) {
            self.0.clear();
            self.0.shrink_to_fit();
        }
        self.0.insert(key.into(), band.min(6));
    }

    /// The keys among `candidates` the cache cannot answer yet, first
    /// seen first and never twice.
    pub fn misses<'a, I: IntoIterator<Item = &'a str>>(&self, candidates: I) -> Vec<String> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut out: Vec<String> = Vec::new();
        for key in candidates {
            if !self.0.contains_key(key) && seen.insert(key) {
                out.push(key.to_string());
            }
        }
        out
    }

    /// Whether any cached band answers above `threshold` — the decision
    /// every painter asks per token.
    pub fn any_above<S: AsRef<str>>(&self, candidates: &[S], threshold: u8) -> bool {
        candidates
            .iter()
            .filter_map(|key| self.0.get(key.as_ref()))
            .any(|&band| band > threshold)
    }

    /// Everything answered so far, dropped (a document closed, a dataset
    /// replaced).
    pub fn clear(&mut self) {
        self.0.clear();
        self.0.shrink_to_fit();
    }

    /// How many words are remembered.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Nothing remembered.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys<'a>(words: &[&'a str]) -> Vec<&'a str> {
        words.to_vec()
    }

    #[test]
    fn a_miss_is_remembered_as_zero_and_never_asked_twice() {
        let mut cache = LevelCache::default();
        cache.insert("palimpsest", 0);
        assert_eq!(cache.get("palimpsest"), Some(0));
        assert_eq!(
            cache.misses(keys(&["palimpsest", "ephemeral"])),
            keys(&["ephemeral"])
        );
        // A repeated candidate is asked once.
        assert_eq!(
            cache.misses(keys(&["ephemeral", "ephemeral"])),
            keys(&["ephemeral"])
        );
    }

    #[test]
    fn above_threshold_means_strictly_above() {
        let mut cache = LevelCache::default();
        cache.insert("run", 2);
        cache.insert("ephemeral", 6);
        assert!(cache.any_above(&keys(&["run", "ephemeral"]), 4));
        assert!(!cache.any_above(&keys(&["run"]), 4));
        // A band AT the threshold is the reader's own level: not marked.
        assert!(!cache.any_above(&keys(&["run"]), 2));
        // An unknown word never marks.
        assert!(!cache.any_above(&keys(&["zzz"]), 2));
    }

    #[test]
    fn the_cap_clears_whole_instead_of_growing() {
        let mut cache = LevelCache::default();
        for i in 0..LEVEL_CACHE_CAP {
            cache.insert(&format!("w{i}"), 3);
        }
        assert_eq!(cache.len(), LEVEL_CACHE_CAP);
        cache.insert("fresh", 4);
        assert!(cache.len() <= LEVEL_CACHE_CAP);
        // The eviction is wholesale: nothing from before survives with the
        // new entry guaranteed present.
        assert_eq!(cache.get("fresh"), Some(4));
    }

    #[test]
    fn clearing_drops_the_capacity_too() {
        let mut cache = LevelCache::default();
        for i in 0..100 {
            cache.insert(&format!("w{i}"), 3);
        }
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn a_miss_is_band_zero_and_a_six_point_something_is_six() {
        assert_eq!(band_of(None), 0);
        assert_eq!(band_of(Some(1.0)), 1);
        assert_eq!(band_of(Some(4.4)), 4);
        assert_eq!(band_of(Some(4.6)), 5);
        assert_eq!(band_of(Some(6.7)), 6);
        assert_eq!(band_of(Some(-3.0)), 1);
    }

    #[test]
    fn bands_clamp_into_one_through_six() {
        let mut cache = LevelCache::default();
        cache.insert("run", 99);
        assert_eq!(cache.get("run"), Some(6));
    }
}
