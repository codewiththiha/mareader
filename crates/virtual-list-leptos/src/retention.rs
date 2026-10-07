//! Zombie retention: the pure bookkeeping that keeps freshly evicted
//! items rendered briefly.

use virtual_list::Window;

/// The longest one frame may stand for in a frame-counted bridge.
pub const FRAME_CEILING_MS: u32 = 120;

/// How an item that leaves the mount window is retired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPolicy {
    /// Unmounted in the same tick the window evicts it.
    Immediate,
    /// Kept rendered until `ms` after eviction, at most `max` at a time.
    Grace {
        /// How long an evicted item stays mounted, milliseconds.
        ms: u32,
        /// Ceiling on simultaneously retained items.
        max: usize,
    },
    /// Kept for the one frame that evicted it, only while seeking.
    MotionGated {
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
            Self::Grace { max, .. } | Self::MotionGated { max } => *max,
        }
    }
}

/// One evicted item, kept until its deadline on either clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetainedItem {
    /// The evicted item's index.
    pub index: usize,
    /// When the bridge ends at the latest, in ms.
    pub expires_at: f64,
    /// The frame this item retires at; `u64::MAX` for a millisecond policy.
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
        // One frame, with the ceiling standing in for a frame never arriving.
        RetentionPolicy::MotionGated { .. } => (now_ms + f64::from(FRAME_CEILING_MS), frame + 1),
        // Unreachable through `retain_evicted`: already expired is the answer.
        RetentionPolicy::Immediate => (now_ms, frame),
    }
}

/// Diff two windows and schedule the evicted indices for retention.
pub fn retain_evicted(
    old: Option<Window>,
    new: Option<Window>,
    now_ms: f64,
    frame: u64,
    policy: &RetentionPolicy,
    seeking: bool,
) -> Vec<RetainedItem> {
    let gated = matches!(policy, RetentionPolicy::MotionGated { .. });
    if !policy.bridges() || (gated && !seeking) {
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
        // Keep the upper end of the eviction list: the items just below.
        let drop = evicted.len() - max;
        evicted.drain(0..drop);
    }
    evicted
}

/// Drop retained items past their deadline, or back in the window.
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

/// Whether `index` is inside an unexpired bridge.
pub fn is_retained(retained: &[RetainedItem], index: usize, now_ms: f64, frame: u64) -> bool {
    retained
        .iter()
        .any(|item| item.index == index && item.alive(now_ms, frame))
}

/// Milliseconds until the next deadline, at least 1.
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
    const SEEK: RetentionPolicy = RetentionPolicy::MotionGated { max: 6 };

    #[test]
    fn a_window_move_retains_only_the_evicted_side() {
        // Scrolling down: 0..=4 -> 2..=6 evicts 0 and 1.
        let retained = retain_evicted(window(0, 4), window(2, 6), 1_000.0, 0, &GRACE, true);
        let indices: Vec<usize> = retained.iter().map(|r| r.index).collect();
        assert_eq!(indices, vec![0, 1]);
        assert!((retained[0].expires_at - 1_300.0).abs() < 1e-9);
        // A millisecond bridge is not bounded by frames at all.
        assert_eq!(retained[0].frame_limit, u64::MAX);
        let at_rest = retain_evicted(window(0, 4), window(2, 6), 1_000.0, 0, &GRACE, false);
        assert_eq!(at_rest, retained, "Grace does not ask about the scroll");
    }

    #[test]
    fn a_motion_bridge_needs_a_seek_and_ends_with_the_next_frame() {
        let evicted = retain_evicted(window(0, 4), window(2, 6), 1_000.0, 7, &SEEK, true);
        let indices: Vec<usize> = evicted.iter().map(|r| r.index).collect();
        assert_eq!(indices, vec![0, 1]);
        assert_eq!(evicted[0].frame_limit, 8, "one frame of bridge");
        assert_eq!(evicted[0].expires_at, 1_000.0 + f64::from(FRAME_CEILING_MS));
        assert!(evicted[0].alive(1_050.0, 7));
        assert!(!evicted[0].alive(1_050.0, 8), "and it is over next frame");

        // At reading speed the same eviction retains nothing.
        assert!(
            retain_evicted(window(0, 4), window(2, 6), 1_000.0, 7, &SEEK, false).is_empty(),
            "no seek, no bridge"
        );
    }

    #[test]
    fn a_zooms_worth_of_eviction_on_both_sides_is_retained() {
        // A commit can shrink the window from both ends at once.
        let retained = retain_evicted(window(4, 20), window(8, 14), 0.0, 0, &GRACE, false);
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
            true,
        );
        assert!(none.is_empty());
        assert!(!RetentionPolicy::Immediate.bridges());
        let unbounded = RetentionPolicy::Grace { ms: 300, max: 0 };
        let none = retain_evicted(window(0, 4), window(2, 6), 0.0, 0, &unbounded, true);
        assert!(none.is_empty());
        assert!(!unbounded.bridges());
        let gated_no_room = RetentionPolicy::MotionGated { max: 0 };
        assert!(!gated_no_room.bridges());
    }

    #[test]
    fn an_empty_or_appearing_window_retains_nothing() {
        assert!(retain_evicted(None, window(0, 4), 0.0, 0, &GRACE, true).is_empty());
        // A window disappearing unmounts everything: nothing is kept.
        assert!(retain_evicted(window(0, 4), None, 0.0, 0, &GRACE, true).is_empty());
    }

    #[test]
    fn the_retained_set_is_bounded_and_keeps_the_closest_items() {
        // 18 evicted, room for 6: keep the six nearest the new window.
        let retained = retain_evicted(window(0, 19), window(18, 19), 0.0, 0, &SEEK, true);
        let indices: Vec<usize> = retained.iter().map(|r| r.index).collect();
        assert_eq!(indices, vec![12, 13, 14, 15, 16, 17]);
        assert_eq!(retained.len(), 6);
    }

    #[test]
    fn a_frozen_frame_clock_still_expires_the_bridge() {
        // A hidden tab stops rAF: the ceiling is the wake.
        let retained = retain_evicted(window(0, 4), window(3, 6), 0.0, 0, &SEEK, true);
        assert!(retained[0].alive(0.0, 0));
        let ceiling = f64::from(FRAME_CEILING_MS);
        assert_eq!(retained[0].expires_at, ceiling, "one ceiling per bridge");
        assert!(!retained[0].alive(ceiling, 0));
        assert_eq!(
            next_deadline_ms(&retained, 0.0),
            u64::from(FRAME_CEILING_MS)
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
        // A passed deadline still reports a tick, so the prune runs.
        assert_eq!(next_deadline_ms(&retained, 600.0), 1);
    }
}
