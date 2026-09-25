//! The one shape a reactive fingerprint takes.
//!
//! Five places in the app turn a set of reactive reads into a `u64` that only
//! changes when the reads do: the two virtualizers' geometry, the two overlay
//! fingerprints in `reader_runtime::components::viewer::refresh`, the reflow row's
//! relayout trigger and the continuous stream's block epoch. Each spelled out
//! the hasher, the `finish` and the `Signal::derive` around it, and two of the
//! five imported `Hash` and `Hasher` inside the closure rather than at the top
//! of the file. What the hash is *of* stays with each caller; this owns the
//! rest, so "what an epoch is" has one home.

use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

use leptos::prelude::*;

/// A `Signal` that re-reads only when `hash` produces a different `u64`.
///
/// Hashing rather than comparing the values keeps the signal's type small and
/// fixed, and lets a caller fold in something that has no cheap equality —
/// a pointer identity, a `Vec` of sizes — without the signal's type naming it.
/// `Send + Sync` is what `Signal::derive` asks of its closure; each call site's
/// captured state already satisfies it, but a generic has to say so.
pub fn epoch_signal(hash: impl Fn(&mut DefaultHasher) + Send + Sync + 'static) -> Signal<u64> {
    Signal::derive(move || {
        let mut hasher = DefaultHasher::new();
        hash(&mut hasher);
        hasher.finish()
    })
}
