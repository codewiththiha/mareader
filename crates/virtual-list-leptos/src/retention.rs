//! Zombie retention: the pure bookkeeping that lets freshly evicted items
//! stay rendered for a short grace period.
//!
//! A virtualizer's window moves for two reasons that benefit from a bridge:
//! fast scrolls (an item blinks out and back when the window jitters around
//! a fling) and a zoom's geometry commit (the reader has been looking at a
//! continuously scaled surface; the commit reinstalls geometry at the new
//! scale and the window jumps, evicting pages that are still on screen).
//! Retaining those items briefly — as [`RetainedItem`]s with an expiry —
//! keeps their DOM alive across the change so nothing visibly pops.
//!
//! This module is pure (host-testable): the reactive adapter in
//! [`crate::virtualizer`] owns the signals and the expiry timer, and only
//! the merge/diff arithmetic lives here. The set is always BOUNDED —
//! `max_retained` prunes the oldest first — so retention can never turn
//! windowing into "mount everything".

use virtual_list::Window;

/// One evicted item, kept rendered until `expires_at` (monotonic-ish
/// milliseconds, e.g. `Date::now()`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetainedItem {
    /// The evicted item's index.
    pub index: usize,
    /// When the item unmounts, in milliseconds on the caller's clock.
    pub expires_at: f64,
}

/// Diff two windows and schedule the evicted indices for retention.
///
/// `None` windows (no layout yet / empty list) retain nothing. Indices that
/// simply moved out of a `None`→`Some` transition are new mounts, not
/// evictions, so only items that were IN the old window and are NOT in the
/// new one are retained. Re-entering the window clears an item's retention:
/// it is active again, and its DOM never left.
pub fn retain_evicted(
    old: Option<Window>,
    new: Option<Window>,
    now_ms: f64,
    grace_ms: u32,
    max_retained: usize,
) -> Vec<RetainedItem> {
    if grace_ms == 0 || max_retained == 0 {
        return Vec::new();
    }
    let (Some(old), Some(new)) = (old, new) else {
        return Vec::new();
    };
    let mut evicted: Vec<RetainedItem> = (old.first..=old.last)
        .filter(|index| index < &new.first || index > &new.last)
        .map(|index| RetainedItem {
            index,
            expires_at: now_ms + grace_ms as f64,
        })
        .collect();
    if evicted.len() > max_retained {
        // Bound the set by keeping the upper end of the (ascending) eviction
        // list — the items just below the new window. For a one-sided scroll
        // these are the ones closest to the viewport; on a two-sided shrink
        // the lower stragglers are dropped first.
        let drop = evicted.len() - max_retained;
        evicted.drain(0..drop);
    }
    evicted
}

/// Drop retained items whose time is up, and drop any that are back inside
/// the active window (an active item needs no bridge).
pub fn prune_retained(
    mut retained: Vec<RetainedItem>,
    active: Option<Window>,
    now_ms: f64,
) -> Vec<RetainedItem> {
    retained.retain(|item| item.expires_at > now_ms);
    if let Some(window) = active {
        retained.retain(|item| item.index < window.first || item.index > window.last);
    }
    retained
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(first: usize, last: usize) -> Option<Window> {
        Some(Window { first, last })
    }

    #[test]
    fn a_window_move_retains_only_the_evicted_side() {
        // Scrolling down: 0..=4 -> 2..=6 evicts 0 and 1.
        let retained = retain_evicted(window(0, 4), window(2, 6), 1_000.0, 300, 12);
        let indices: Vec<usize> = retained.iter().map(|r| r.index).collect();
        assert_eq!(indices, vec![0, 1]);
        assert!((retained[0].expires_at - 1_300.0).abs() < 1e-9);
    }

    #[test]
    fn a_zooms_worth_of_eviction_on_both_sides_is_retained() {
        // A commit can shrink the window from both ends at once.
        let retained = retain_evicted(window(4, 20), window(8, 14), 0.0, 300, 12);
        let indices: Vec<usize> = retained.iter().map(|r| r.index).collect();
        assert_eq!(indices, (4..=7).chain(15..=20).collect::<Vec<_>>());
    }

    #[test]
    fn zero_grace_or_zero_budget_disables_retention_entirely() {
        assert!(retain_evicted(window(0, 4), window(2, 6), 0.0, 0, 12).is_empty());
        assert!(retain_evicted(window(0, 4), window(2, 6), 0.0, 300, 0).is_empty());
    }

    #[test]
    fn an_empty_or_appearing_window_retains_nothing() {
        assert!(retain_evicted(None, window(0, 4), 0.0, 300, 12).is_empty());
        // A window disappearing unmounts everything; retaining the whole
        // document would defeat virtualization, so nothing is kept.
        assert!(retain_evicted(window(0, 4), None, 0.0, 300, 12).is_empty());
    }

    #[test]
    fn the_retained_set_is_bounded_and_keeps_the_closest_items() {
        // 20 evicted, room for 6: keep the six nearest the new window.
        let retained = retain_evicted(window(0, 19), window(18, 19), 0.0, 300, 6);
        let indices: Vec<usize> = retained.iter().map(|r| r.index).collect();
        assert_eq!(indices, vec![12, 13, 14, 15, 16, 17]);
        assert_eq!(retained.len(), 6);
    }

    #[test]
    fn expiry_and_reactivation_prune() {
        let retained = vec![
            RetainedItem { index: 0, expires_at: 500.0 },
            RetainedItem { index: 9, expires_at: 9_000.0 },
        ];
        // At t=1000 the first has expired; 9 is alive but back in the window.
        let pruned = prune_retained(retained, window(8, 12), 1_000.0);
        assert!(pruned.is_empty());
    }

    #[test]
    fn unexpired_items_outside_the_window_survive_pruning() {
        let retained = vec![RetainedItem { index: 3, expires_at: 9_000.0 }];
        let pruned = prune_retained(retained, window(8, 12), 1_000.0);
        assert_eq!(pruned.len(), 1);
    }
}
