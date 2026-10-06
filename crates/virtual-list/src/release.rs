//! Release accounting: what a window move frees.
//!
//! The virtualizer owns one question a content cache cannot answer for itself:
//! is this item still near the reader? Answering it with a clock — "hold it for
//! `n` frames, or `m` milliseconds" — keeps whatever happens to have been on
//! screen when the scroll stopped and drops whatever the reader is about to
//! return to, which is the opposite of a reason. This module answers with
//! distance instead: content stays while it is within the grace margin of the
//! mount window and goes the frame it leaves it, in the same tick the row
//! unmounts.
//!
//! The distinction that makes this safe is *what* is released. An item's
//! measurement (its size) is what keeps the scrollbar honest and the anchors
//! stable, and it costs 16 bytes; the layout keeps that for as long as it keeps
//! the item. What [`release_sides`] says to drop is the expensive half a
//! consumer attached to the index: the raster, the canvas, the snapshot. That is
//! the whole memory argument — a cache that holds the first and not the second
//! is both cheaper and smoother than one that holds neither.
//!
//! Pure and host-testable: the adapter in the Leptos crate owns the reactive
//! signal and the listeners; only the diff and the bounded queue live here.

use alloc::vec::Vec;

use crate::Window;

/// The release queue's reason codes and the bounded set they travel in.
///
/// See [`release_sides`] for the spatial rule that fills it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseReason {
    /// Its index left the window's grace margin: the reader is away from it.
    Evicted,
    /// Its geometry changed underneath it, so what it holds is wrong: a zoom
    /// rescale or a re-measure replaces the content rather than dropping it.
    Superseded,
}

/// The sides of the old window whose content may be dropped after a window
/// move, given `grace` items of reversal allowance on each side of the new one.
///
/// This is the memory half of the model, and it is deliberately *spatial* rather
/// than temporal: an item's content stays while it is near the window — scrolling
/// back one page finds it ready — and goes the moment the window has left it
/// behind past the margin, in the same tick the row unmounts. A timer-based hold
/// keeps content because it *was* recent, which is not a reason to keep
/// anything; that is how a cache ends up sized by the whole session while
/// paying for nothing but RAM.
///
/// A side with nothing beyond its margin is `None`, and neither side can name an
/// index the new window mounts — dropping content from an item that just came
/// into the window is the worst thing a release rule can do, because it blanks
/// the page the reader is looking at.
pub fn release_sides(old: Window, new: Window, grace: usize) -> (Option<Window>, Option<Window>) {
    let below_last = new.first.saturating_sub(grace + 1);
    let below = (old.first <= old.last && old.first <= below_last).then_some(Window {
        first: old.first,
        last: below_last.min(old.last),
    });
    let above_first = new.last.saturating_add(grace + 1);
    let above = (old.first <= old.last && above_first <= old.last).then_some(Window {
        first: above_first,
        last: old.last,
    });
    (below, above)
}

/// The pending releases, bounded.
///
/// The adapter pushes into it whenever it publishes a window, and drains it to
/// hand the indices to whoever owns content for them. The ceiling exists because
/// a listener that never drains must not turn a scroll into an unbounded queue:
/// when the set is full the oldest entry goes, which is the entry the caller has
/// had the longest to act on.
#[derive(Debug, Clone)]
pub struct ReleaseLedger {
    entries: Vec<(usize, ReleaseReason)>,
    capacity: usize,
}

impl ReleaseLedger {
    /// A ledger holding at most `capacity` releases. `0` behaves as `1`: a
    /// caller that asks for no room still gets the most recent release, which
    /// is the only answer that can be acted on.
    pub const fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity: if capacity == 0 { 1 } else { capacity },
        }
    }

    /// Queue one release, unless that index is already pending. Returns whether
    /// it was added.
    pub fn push(&mut self, index: usize, reason: ReleaseReason) -> bool {
        if self.entries.iter().any(|(held, _)| *held == index) {
            return false;
        }
        if self.entries.len() >= self.capacity {
            self.entries.remove(0);
        }
        self.entries.push((index, reason));
        true
    }

    /// Take every pending release, emptying the ledger.
    pub fn drain(&mut self) -> Vec<(usize, ReleaseReason)> {
        core::mem::take(&mut self.entries)
    }

    /// How many releases are waiting.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is waiting. Required alongside [`Self::len`].
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(first: usize, last: usize) -> Window {
        Window { first, last }
    }

    #[test]
    fn grace_absorbs_a_reversal_and_a_jump_releases_the_far_side() {
        // A step of four with three items of allowance: only index 0 is out of
        // it, and the upper end of the old window is now mounted.
        assert_eq!(
            release_sides(win(0, 10), win(4, 14), 3),
            (Some(win(0, 0)), None)
        );
        assert_eq!(
            release_sides(win(0, 10), win(8, 16), 3),
            (Some(win(0, 4)), None)
        );
        // Backward, the escaped side is the one above the new window.
        assert_eq!(
            release_sides(win(20, 30), win(4, 12), 2),
            (None, Some(win(15, 30)))
        );
        // No margin: everything the window left behind goes.
        assert_eq!(
            release_sides(win(0, 20), win(10, 12), 0),
            (Some(win(0, 9)), Some(win(13, 20)))
        );
    }

    #[test]
    fn a_release_never_names_an_index_the_new_window_mounts() {
        // The trap this rule guards: a forward move would otherwise compute an
        // `above` side inside the new window, and a backward move a `below`
        // side inside it. Dropping content from an item that just arrived
        // blanks the page the reader is looking at.
        assert_eq!(release_sides(win(0, 2), win(100, 110), 1).1, None);
        assert_eq!(release_sides(win(100, 102), win(0, 10), 1).0, None);
        // A window that only grew releases nothing, on either side.
        assert_eq!(release_sides(win(5, 9), win(2, 14), 0), (None, None));
        // And an index cannot be released while it is inside the new window.
        let (below, above) = release_sides(win(0, 30), win(10, 20), 0);
        assert_eq!(below, Some(win(0, 9)));
        assert_eq!(above, Some(win(21, 30)));
    }

    #[test]
    fn the_ledger_dedups_bounds_and_clears() {
        let mut ledger = ReleaseLedger::new(3);
        assert!(ledger.push(5, ReleaseReason::Evicted));
        assert!(
            !ledger.push(5, ReleaseReason::Superseded),
            "already pending"
        );
        assert!(ledger.push(6, ReleaseReason::Evicted));
        assert_eq!(ledger.len(), 2);
        // Overflow drops the oldest entry, the one the caller has had longest
        // to act on.
        for index in 7..=10 {
            assert!(ledger.push(index, ReleaseReason::Evicted));
        }
        assert_eq!(ledger.len(), 3, "bounded by the ceiling");
        let taken = ledger.drain();
        assert_eq!(
            taken,
            vec![
                (8, ReleaseReason::Evicted),
                (9, ReleaseReason::Evicted),
                (10, ReleaseReason::Evicted)
            ]
        );
        assert!(ledger.is_empty(), "draining empties it");
        // After a drain the index is pending no more, so it can be queued again.
        assert!(ledger.push(8, ReleaseReason::Superseded));
    }

    #[test]
    fn a_zero_ceiling_still_holds_the_most_recent_release() {
        let mut ledger = ReleaseLedger::new(0);
        assert!(ledger.push(1, ReleaseReason::Evicted));
        assert!(ledger.push(2, ReleaseReason::Evicted));
        assert_eq!(ledger.drain(), vec![(2, ReleaseReason::Evicted)]);
    }
}

// only the changed file was rewritten
