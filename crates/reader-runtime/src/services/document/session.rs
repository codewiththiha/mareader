//! The realm's document-generation mint, and the diagnostics epoch it
//! doubles as.
//!
//! Opening is asynchronous in several hops — the engine's `open`, the outline
//! resolve, the cover render — and nothing stops a reader from picking a
//! second book mid-flight. Without an owner, the loser of that race still runs
//! its tail: it writes `num_pages`, `page1_size` and the size stores for a
//! book no longer open, seeds the zoom for the wrong page size, and flips
//! `status` to `Ready` after the winner did.
//!
//! So every attempt claims a generation before it starts and re-checks it
//! after each await. The generation is PER PANE
//! ([`crate::pane::handle::PaneHandle::claim_generation`] /
//! [`owns_generation`](crate::pane::handle::PaneHandle::owns_generation)):
//! a later open or the dispose of THE SAME pane stales an attempt, another
//! pane's open never does. This module only mints the numbers — monotonic
//! for the realm's life, so a generation is never reused by any pane — and
//! reports the latest one as the diagnostics epoch.
//!
//! Relaxed ordering throughout: the webview is single-threaded, so the counter
//! only needs to be monotonic, never synchronising.

use std::sync::atomic::{AtomicU64, Ordering};

static MINT: AtomicU64 = AtomicU64::new(0);

/// A fresh document generation, never handed out before.
pub(crate) fn next_generation() -> u64 {
    MINT.fetch_add(1, Ordering::Relaxed) + 1
}

/// The latest generation minted, as a diagnostics epoch: it moves on every
/// open and dispose in any pane, so a snapshot can be attributed to a
/// moment in the lifecycle. NOT an ownership check — that is the pane's.
pub(crate) fn current_epoch() -> u64 {
    MINT.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generations_never_repeat() {
        let a = next_generation();
        let b = next_generation();
        let c = next_generation();
        assert!(a < b && b < c);
        assert!(current_epoch() >= c);
    }
}
