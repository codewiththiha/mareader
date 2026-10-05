//! Zombie retention: the pure bookkeeping that lets freshly evicted items
//! stay rendered for a short bridge across the change that evicted them.
//!
//! A window moves for two reasons that benefit from a bridge: fast scrolls
//! (an item blinks out and back when the window jitters around a fling) and a
//! zoom's geometry commit (the commit reinstalls geometry at the new scale and
//! the window jumps, evicting pages still on screen). Retaining those items
//! briefly — as [`RetainedItem`]s with a deadline — keeps their DOM alive
//! across the change so nothing visibly pops.
//!
//! The bridge is measured in whichever unit fits the caller
//! ([`RetentionPolicy`]): milliseconds for a caller pacing against wall-clock
//! work, animation frames for a caller whose whole problem is frames. Both
//! clocks are deadlines, so one [`RetainedItem::alive`] test serves them and an
//! expired bridge always ends on a wake the adapter owns (a timer, or the
//! frame chain) — never on an event that may not come.
//!
//! Pure and host-testable: the reactive adapter in [`crate::virtualizer`]
//! owns the signals, the clocks and their timers; only the merge/diff
//! arithmetic lives here. The set is always BOUNDED — `max` prunes oldest
//! first — so retention can never turn windowing into "mount everything".

use virtual_list::Window;

/// The longest one frame may stand for in a frame-counted bridge,
/// milliseconds. A frame is the unit a fling actually measures — four frames
/// of a 60 Hz scroll and four frames of a stalled one are the same offer to
/// scroll back into — but `requestAnimationFrame` stops entirely in a hidden
/// tab, so `frames × this` bounds the bridge in time as well and a
/// backgrounded reader cannot pin a surface indefinitely.
pub const FRAME_CEILING_MS: u32 = 120;

/// How an item that leaves the mount window is retired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPolicy {
    /// Unmounted in the same tick the window evicts it: no zombie DOM, and no
    /// surface a detached node keeps alive.
    Immediate,
    /// Kept rendered until `ms` after the eviction, at most `max` at a time.
    /// The right unit when the bridge must outlast a known wall-clock
    /// operation (a zoom's commit and the relayouts around it).
    Grace {
        /// How long an evicted item stays mounted, milliseconds.
        ms: u32,
        /// Ceiling on simultaneously retained items.
        max: usize,
    },
    /// Kept rendered for `frames` animation frames, so a scroll that turns
    /// around at once finds its DOM instead of rebuilding it: the rail's
    /// glide. Bounded by [`FRAME_CEILING_MS`] per frame and by `max` items.
    Frames {
        /// How many frames the bridge lasts.
        frames: u32,
        /// Ceiling on simultaneously retained items.
        max: usize,
    },
}

impl RetentionPolicy {
    /// Whether this policy keeps an evicted item mounted at all.
    pub fn bridges(&self) -> bool {
        self.max() > 0 && !matches!(self, Self::Immediate)
    }

    /// The ceiling on simultaneously retained items this policy allows.
    pub fn max(&self) -> usize {
        match self {
            Self::Immediate => 0,
            Self::Grace { max, .. } | Self::Frames { max, .. } => *max,
        }
    }
}

/// One evicted item, kept rendered until its deadline: out of `expires_at`
/// (wall-clock milliseconds on the caller's monotonic clock) or past
/// `frame_limit` on the adapter's frame counter, whichever comes first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetainedItem {
    /// The evicted item's index.
    pub index: usize,
    /// When the bridge ends at the latest, in milliseconds. For a
    /// frame-counted bridge this is the ceiling the frames are worth, so a
    /// frozen rAF clock still releases the item.
    pub expires_at: f64,
    /// The frame count this item is retired at; [`u64::MAX`] for a policy that
    /// counts milliseconds, so both clocks share one test.
    pub frame_limit: u64,
}

impl RetainedItem {
    /// Whether the bridge still holds at `now_ms` / `frame`.
    pub fn alive(&self, now_ms: f64, frame: u64) -> bool {
        now_ms < self.expires_at && frame < self.frame_limit
    }
}

/// The deadlines a policy gives an item evicted at `now_ms` / `frame`.
fn deadline(policy: &RetentionPolicy, now_ms: f64, frame: u64) -> (f64, u64) {
    match *policy {
        RetentionPolicy::Grace { ms, .. } => (now_ms + f64::from(ms), u64::MAX),
        RetentionPolicy::Frames { frames, .. } => (
            now_ms + f64::from(frames) * f64::from(FRAME_CEILING_MS),
            frame + u64::from(frames),
        ),
        // Unreachable through `retain_evicted` (no bridge, no deadlines);
        // "already expired" is the honest answer for anything that asks.
        RetentionPolicy::Immediate => (now_ms, frame),
    }
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
    frame: u64,
    policy: &RetentionPolicy,
) -> Vec<RetainedItem> {
    if !policy.bridges() {
        return Vec::new();
    }
    let (Some(old), Some(new)) = (old, new) else {
        return Vec::new();
    };
    let (expires_at, frame_limit) = deadline(policy, now_ms, frame);
    let mut evicted: Vec<RetainedItem> = (old.first..=old.last)
        .filter(|index| index < &new.first || index > &new.last)
        .map(|index| RetainedItem {
            index,
            expires_at,
            frame_limit,
        })
        .collect();
    let max = policy.max();
    if evicted.len() > max {
        // Bound the set by keeping the upper end of the (ascending) eviction
        // list — the items just below the new window. For a one-sided scroll
        // these are the ones closest to the viewport; on a two-sided shrink
        // the lower stragglers are dropped first.
        let drop = evicted.len() - max;
        evicted.drain(0..drop);
    }
    evicted
}

/// Drop retained items whose deadline has passed, and drop any that are back
/// inside the active window (an active item needs no bridge).
pub fn prune_retained(
    mut retained: Vec<RetainedItem>,
    active: Option<Window>,
    now_ms: f64,
    frame: u64,
) -> Vec<RetainedItem> {
    retained.retain(|item| item.alive(now_ms, frame));
    if let Some(window) = active {
        retained.retain(|item| item.index < window.first || item.index > window.last);
    }
    retained
}

/// Whether `index` is inside an unexpired bridge. One test for the mounted
/// items, the row list and the per-item state signal, so the three can never
/// disagree about who is a zombie.
pub fn is_retained(retained: &[RetainedItem], index: usize, now_ms: f64, frame: u64) -> bool {
    retained
        .iter()
        .any(|item| item.index == index && item.alive(now_ms, frame))
}

/// Milliseconds until the next deadline anyone is waiting on (always at least
/// 1, so a timer is always armed into the future). A frame-counted bridge is
/// woken by the adapter's frame chain; this is its ceiling and the only waker
/// a millisecond bridge has.
pub fn next_deadline_ms(retained: &[RetainedItem], now_ms: f64) -> u64 {
    retained
        .iter()
        .map(|item| (item.expires_at - now_ms).max(1.0))
        .fold(f64::INFINITY, f64::min)
        .ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(first: usize, last: usize) -> Option<Window> {
        Some(Window { first, last })
    }

    const GRACE: RetentionPolicy = RetentionPolicy::Grace { ms: 300, max: 12 };
    const BRIDGE: RetentionPolicy = RetentionPolicy::Frames { frames: 4, max: 6 };

    #[test]
    fn a_window_move_retains_only_the_evicted_side() {
        // Scrolling down: 0..=4 -> 2..=6 evicts 0 and 1.
        let retained = retain_evicted(window(0, 4), window(2, 6), 1_000.0, 0, &GRACE);
        let indices: Vec<usize> = retained.iter().map(|r| r.index).collect();
        assert_eq!(indices, vec![0, 1]);
        assert!((retained[0].expires_at - 1_300.0).abs() < 1e-9);
        // A millisecond bridge is not bounded by frames at all.
        assert_eq!(retained[0].frame_limit, u64::MAX);
    }

    #[test]
    fn a_zooms_worth_of_eviction_on_both_sides_is_retained() {
        // A commit can shrink the window from both ends at once.
        let retained = retain_evicted(window(4, 20), window(8, 14), 0.0, 0, &GRACE);
        let indices: Vec<usize> = retained.iter().map(|r| r.index).collect();
        assert_eq!(indices, (4..=7).chain(15..=20).collect::<Vec<_>>());
    }

    #[test]
    fn immediate_and_empty_budgets_retain_nothing() {
        let none = retain_evicted(
            window(0, 4),
            window(2, 6),
            0.0,
            0,
            &RetentionPolicy::Immediate,
        );
        assert!(none.is_empty());
        assert!(!RetentionPolicy::Immediate.bridges());
        let unbounded = RetentionPolicy::Grace { ms: 300, max: 0 };
        let none = retain_evicted(window(0, 4), window(2, 6), 0.0, 0, &unbounded);
        assert!(none.is_empty());
        assert!(!unbounded.bridges());
    }

    #[test]
    fn an_empty_or_appearing_window_retains_nothing() {
        assert!(retain_evicted(None, window(0, 4), 0.0, 0, &GRACE).is_empty());
        // A window disappearing unmounts everything; retaining the whole
        // document would defeat virtualization, so nothing is kept.
        assert!(retain_evicted(window(0, 4), None, 0.0, 0, &GRACE).is_empty());
    }

    #[test]
    fn the_retained_set_is_bounded_and_keeps_the_closest_items() {
        // 18 evicted, room for 6: keep the six nearest the new window.
        let retained = retain_evicted(window(0, 19), window(18, 19), 0.0, 0, &BRIDGE);
        let indices: Vec<usize> = retained.iter().map(|r| r.index).collect();
        assert_eq!(indices, vec![12, 13, 14, 15, 16, 17]);
        assert_eq!(retained.len(), 6);
    }

    #[test]
    fn a_frame_bridge_survives_a_slow_clock_and_ends_on_its_count() {
        let retained = retain_evicted(window(0, 4), window(3, 6), 1_000.0, 10, &BRIDGE);
        let indices: Vec<usize> = retained.iter().map(|r| r.index).collect();
        assert_eq!(indices, vec![0, 1, 2]);
        // A slow renderer spends 300ms a frame and still owes exactly four
        // frames: the bridge belongs to the scroll, not to the wall clock.
        assert!(retained[0].alive(1_300.0, 13));
        assert!(is_retained(&retained, 1, 1_300.0, 13));
        // Its own count closes it, whatever the clock says.
        assert!(!retained[0].alive(1_300.0, 14));
        assert!(!is_retained(&retained, 1, 1_300.0, 14));
    }

    #[test]
    fn a_frozen_frame_clock_still_expires_the_bridge() {
        // A hidden tab stops rAF entirely: the ceiling is the wake, so the
        // surface cannot be pinned by frames that will never arrive.
        let retained = retain_evicted(window(0, 4), window(3, 6), 0.0, 0, &BRIDGE);
        assert!(retained[0].alive(0.0, 0));
        let ceiling = f64::from(4 * FRAME_CEILING_MS);
        assert_eq!(retained[0].expires_at, ceiling, "one ceiling per frame");
        assert!(!retained[0].alive(ceiling, 0));
        assert_eq!(
            next_deadline_ms(&retained, 0.0),
            u64::from(4 * FRAME_CEILING_MS)
        );
    }

    #[test]
    fn expiry_and_reactivation_prune() {
        let retained = vec![
            RetainedItem {
                index: 0,
                expires_at: 500.0,
                frame_limit: u64::MAX,
            },
            RetainedItem {
                index: 9,
                expires_at: 9_000.0,
                frame_limit: u64::MAX,
            },
        ];
        // At t=1000 the first has expired; 9 is alive but back in the window.
        let pruned = prune_retained(retained, window(8, 12), 1_000.0, 0);
        assert!(pruned.is_empty());
    }

    #[test]
    fn unexpired_items_outside_the_window_survive_pruning() {
        let retained = vec![RetainedItem {
            index: 3,
            expires_at: 9_000.0,
            frame_limit: u64::MAX,
        }];
        let pruned = prune_retained(retained, window(8, 12), 1_000.0, 0);
        assert_eq!(pruned.len(), 1);
    }

    #[test]
    fn the_next_deadline_waits_for_the_soonest_one() {
        let retained = vec![
            RetainedItem {
                index: 1,
                expires_at: 500.0,
                frame_limit: u64::MAX,
            },
            RetainedItem {
                index: 2,
                expires_at: 9_000.0,
                frame_limit: u64::MAX,
            },
        ];
        assert_eq!(next_deadline_ms(&retained, 400.0), 100);
        // A deadline already passed still reports a tick: the prune must run
        // on the next one, not wait for a deadline that is behind us.
        assert_eq!(next_deadline_ms(&retained, 600.0), 1);
    }
}

// only the changed file was rewritten
