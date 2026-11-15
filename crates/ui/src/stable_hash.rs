//! One stable hash for cached, content-derived keys.

use std::hash::{DefaultHasher, Hash, Hasher};

pub fn hash(v: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}
