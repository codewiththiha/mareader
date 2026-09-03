//! The gate that keeps a navigation from being lost to a zoom.
//!
//! A page write that lands while a zoom transaction holds the geometry cannot
//! be acted on — the transaction's anchor decides where the reader ends up,
//! and a scroll issued into it fights that. Dropping the write is worse: an
//! outline click or a search hit that lands inside a fit slide would simply
//! never happen.
//!
//! So the write is HELD, with its value, and replayed on the first run after
//! the transaction closes. Pure and `Cell`-based, so the whole contract is
//! testable on the host without a browser.

use std::cell::Cell;

/// Decides whether a run of the page→scroll effect may command the strip, in
/// a form pure enough to unit-test on the host.
///
/// The two inputs it distinguishes are "the page signal changed" and "the
/// zoom transaction flag changed" — the effect re-runs for both, and only the
/// first is a navigation intent. A write that lands while a transaction holds
/// the geometry is HELD, with its value; the first run after the transaction
/// closes replays it. A transaction that closes with no held write moves
/// nothing, which is what keeps a zoom commit from scrolling the top of the
/// current page back under the reader's eyes.
///
/// Holding the VALUE (not just a flag) is what lets the replay survive the
/// dominant arm: both effects re-run in the same flush when a transaction
/// closes, and if the dominant arm runs first it reads the not-yet-jumped
/// strip and "corrects" the page back to the stale dominant item — a replay
/// that only remembered "something was held" would then scroll to the
/// clobbered page. So the dominant arm DEFERS to [`JumpGate::pending`] for
/// exactly that flush, and a replay whose page signal was clobbered anyway
/// RE-ASSERTS the held page alongside the scroll.
#[derive(Debug, Default)]
pub(super) struct JumpGate {
    /// The page the last `admit` saw, so a real write can be told apart from
    /// a re-run caused by the transaction flag flipping.
    last_page: Cell<u32>,
    /// A page write that arrived while a transaction was open, with its
    /// value, awaiting its replay.
    held: Cell<Option<u32>>,
}

impl JumpGate {
    /// The page the strip should be scrolled to on this run, if any — as
    /// `(page, reassert)`, where `reassert` says the page SIGNAL no longer
    /// names that page (it was clobbered after the hold) and must be written
    /// back before the scroll, or the next scroll event would correct the
    /// strip right back off the jumped-to page.
    pub(super) fn admit(&self, page: u32, zooming: bool) -> Option<(u32, bool)> {
        let changed = page != self.last_page.get();
        self.last_page.set(page);
        if zooming {
            if changed {
                self.held.set(Some(page));
            }
            return None;
        }
        if let Some(held) = self.held.take() {
            return Some((held, held != page));
        }
        changed.then_some((page, false))
    }

    /// A held write waiting for its replay — the dominant arm defers to it
    /// for the flush that closes the transaction, instead of correcting the
    /// page from a strip the replay has not moved yet.
    pub(super) fn pending(&self) -> Option<u32> {
        self.held.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The open-path regression: a page write that lands while a transaction
    /// is open is held, and the first quiet run replays it.
    #[test]
    fn a_page_write_during_a_transaction_replays_when_it_lands() {
        let gate = JumpGate::default();
        // Mount-time first run: page 1, no transaction.
        assert_eq!(gate.admit(1, false), Some((1, false)));
        // The mount fit opens a transaction; the resume jump arrives inside it.
        assert_eq!(gate.admit(1, true), None);
        assert_eq!(gate.pending(), None);
        assert_eq!(gate.admit(42, true), None);
        assert_eq!(gate.pending(), Some(42));
        // Frames pass with the transaction still open: nothing moves, and the
        // dominant arm can see the hold and stand aside.
        assert_eq!(gate.admit(42, true), None);
        assert_eq!(gate.pending(), Some(42));
        // The transaction closes: the held jump lands.
        assert_eq!(gate.admit(42, false), Some((42, false)));
        assert_eq!(gate.pending(), None);
    }

    /// The clobber path: the dominant arm corrects the page to the stale
    /// dominant before the replay runs — the replay must still name the HELD
    /// page and say the counter needs re-asserting.
    #[test]
    fn a_replayed_jump_survives_a_clobbered_page_signal() {
        let gate = JumpGate::default();
        assert_eq!(gate.admit(1, false), Some((1, false)));
        assert_eq!(gate.admit(42, true), None); // held: resume jump
        // The dominant arm ran first and wrote page back to 1.
        assert_eq!(gate.admit(1, false), Some((42, true)));
        // The re-assert write re-runs the arm: an ordinary page change now.
        assert_eq!(gate.admit(42, false), Some((42, false)));
    }

    /// A transaction that closes with no held write must not scroll — that is
    /// the invariant a zoom commit depends on.
    #[test]
    fn a_transaction_closing_alone_moves_nothing() {
        let gate = JumpGate::default();
        assert_eq!(gate.admit(7, false), Some((7, false)));
        assert_eq!(gate.admit(7, true), None); // gesture opens
        assert_eq!(gate.admit(7, true), None); // frames pass
        assert_eq!(gate.admit(7, false), None); // commit lands: no write held
        assert_eq!(gate.pending(), None);
    }

    /// An ordinary page change with no transaction in flight jumps at once.
    #[test]
    fn an_ordinary_page_change_jumps_immediately() {
        let gate = JumpGate::default();
        assert_eq!(gate.admit(3, false), Some((3, false)));
        assert_eq!(gate.admit(9, false), Some((9, false)));
        // Re-runs with the same page (mode flips, transaction echoes) are
        // not navigation intents.
        assert_eq!(gate.admit(9, false), None);
    }

    /// A write held by one transaction survives intermediate no-op runs and
    /// later writes replace it — the newest page is the one that lands.
    #[test]
    fn the_newest_held_write_wins_the_replay() {
        let gate = JumpGate::default();
        assert_eq!(gate.admit(1, false), Some((1, false)));
        assert_eq!(gate.admit(10, true), None);
        assert_eq!(gate.admit(20, true), None);
        assert_eq!(gate.admit(20, false), Some((20, false)));
    }
}
