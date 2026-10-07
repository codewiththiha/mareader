//! The realm's document-generation mint: one generation per open attempt.

use std::sync::atomic::{AtomicU64, Ordering};

static MINT: AtomicU64 = AtomicU64::new(0);

/// A fresh document generation, never handed out before.
pub(crate) fn next_generation() -> u64 {
    MINT.fetch_add(1, Ordering::Relaxed) + 1
}

/// The latest generation minted, a diagnostics epoch — not an ownership check.
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
