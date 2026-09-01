//! Which open attempt currently owns the app's document state.
//!
//! Opening is asynchronous in several hops — the engine's `open`, the outline
//! resolve, the cover render — and nothing stops a reader from picking a
//! second book while the first is still in the middle of them. Without an
//! owner, the loser of that race still runs its tail: it writes `num_pages`,
//! `page1_size` and `metrics` for a book that is no longer open, seeds the
//! zoom for the wrong page size, and flips `status` to `Ready` after the
//! winner already did — resuming the new book at the old one's page.
//!
//! So every attempt takes a stamp before it starts and re-checks it after
//! each await. Taking a stamp is also what invalidates whoever held it
//! before, which is why closing takes one too: a close that lands mid-open
//! must not be undone by the open's own tail two frames later.
//!
//! Relaxed ordering throughout: the webview is single-threaded, so the
//! counter only ever needs to be monotonic, never synchronising.

use std::sync::atomic::{AtomicU64, Ordering};

static SESSION: AtomicU64 = AtomicU64::new(0);

/// Claim the document state for a new attempt (an open or a close) and return
/// its stamp. Every earlier stamp is stale from here on.
pub(crate) fn claim() -> u64 {
    SESSION.fetch_add(1, Ordering::Relaxed) + 1
}

/// Whether `stamp` still owns the document state — false once a later open or
/// close has claimed it.
pub(crate) fn owns(stamp: u64) -> bool {
    SESSION.load(Ordering::Relaxed) == stamp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_claim_supersedes_every_earlier_one() {
        let first = claim();
        assert!(owns(first));
        let second = claim();
        assert!(owns(second));
        assert!(!owns(first), "the superseded attempt must stand down");
    }

    #[test]
    fn stamps_never_repeat() {
        let a = claim();
        let b = claim();
        let c = claim();
        assert!(a < b && b < c);
    }
}
