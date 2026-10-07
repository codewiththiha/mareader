//! The one shape a reactive fingerprint takes: a `u64` that moves with
//! its reads.

use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

use leptos::prelude::*;

/// A `Signal` that re-reads only when `hash` yields a different `u64`.
pub fn epoch_signal(hash: impl Fn(&mut DefaultHasher) + Send + Sync + 'static) -> Signal<u64> {
    Signal::derive(move || {
        let mut hasher = DefaultHasher::new();
        hash(&mut hasher);
        hasher.finish()
    })
}
