//! The gate that keeps a navigation from being lost to a zoom.

use std::cell::Cell;

/// Whether a run of the page→scroll effect may command the strip.
#[derive(Debug, Default)]
pub(super) struct JumpGate {
    /// The page the last `admit` saw.
    last_page: Cell<u32>,
    /// A page write that arrived during a transaction, awaiting replay.
    held: Cell<Option<u32>>,
}

impl JumpGate {
    /// The page to scroll to on this run, as `(page, reassert)`.
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

    /// A held write awaiting replay; the dominant arm defers to it.
    pub(super) fn pending(&self) -> Option<u32> {
        self.held.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The open-path regression: a held write replays on the first
    /// quiet run.
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
        // The transaction is still open: nothing moves.
        assert_eq!(gate.admit(42, true), None);
        assert_eq!(gate.pending(), Some(42));
        // The transaction closes: the held jump lands.
        assert_eq!(gate.admit(42, false), Some((42, false)));
        assert_eq!(gate.pending(), None);
    }

    /// The clobber path: the replay still names the HELD page.
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

    /// A quiet close must not scroll.
    #[test]
    fn a_transaction_closing_alone_moves_nothing() {
        let gate = JumpGate::default();
        assert_eq!(gate.admit(7, false), Some((7, false)));
        assert_eq!(gate.admit(7, true), None); // gesture opens
        assert_eq!(gate.admit(7, true), None); // frames pass
        assert_eq!(gate.admit(7, false), None); // commit lands: no write held
        assert_eq!(gate.pending(), None);
    }

    #[test]
    fn an_ordinary_page_change_jumps_immediately() {
        let gate = JumpGate::default();
        assert_eq!(gate.admit(3, false), Some((3, false)));
        assert_eq!(gate.admit(9, false), Some((9, false)));
        // Re-runs with the same page (mode flips, transaction echoes) are
        // not navigation intents.
        assert_eq!(gate.admit(9, false), None);
    }

    /// A held write survives no-op runs; a newer one replaces it.
    #[test]
    fn the_newest_held_write_wins_the_replay() {
        let gate = JumpGate::default();
        assert_eq!(gate.admit(1, false), Some((1, false)));
        assert_eq!(gate.admit(10, true), None);
        assert_eq!(gate.admit(20, true), None);
        assert_eq!(gate.admit(20, false), Some((20, false)));
    }
}
