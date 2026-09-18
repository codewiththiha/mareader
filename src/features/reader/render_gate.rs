//! The paint window: which of the mounted pages get real renders, as a
//! function of the scroll phase and the dominant page. Everything outside
//! the window shows the ghost placeholder — mounted-but-unpainted is the
//! cheap state, painted is the expensive one, and the window is what keeps
//! the expensive state small while a fling is in flight.
//!
//! The one knob a caller tunes is [`PrefetchCfg`]: how far ahead of the
//! reader the window reaches. With the defaults, sitting on page 2 has
//! pages 3 and 4 rendering, and a moment of idle bakes page 5 — the deep
//! prefetch expressed as a config rather than a hardcode.

use crate::features::reader::scroll_kinetics::ScrollPhase;

/// How far the paint window reaches around the dominant page, per phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefetchCfg {
    /// Pages rendered ahead of the dominant one while the reader moves.
    pub forward: usize,
    /// Pages rendered behind the dominant one while the reader moves.
    pub back: usize,
    /// How far ahead to pre-bake while the reader is still (nobody is
    /// looking — spend it).
    pub idle_forward: usize,
}

impl Default for PrefetchCfg {
    fn default() -> Self {
        Self {
            forward: 2,
            back: 1,
            idle_forward: 3,
        }
    }
}

/// The absolute 0-based paint window for `dominant`, or `None` while a
/// fling parks every render (ghosts only — the settle flush re-arms the
/// window the moment the phase leaves the parked states).
pub fn paint_range(
    phase: ScrollPhase,
    dominant: usize,
    count: usize,
    cfg: &PrefetchCfg,
) -> Option<(usize, usize)> {
    let (back, forward) = match phase {
        ScrollPhase::Fling => return None,
        ScrollPhase::Settling => (0, cfg.forward),
        ScrollPhase::Cruising => (cfg.back, cfg.forward),
        ScrollPhase::Idle => (cfg.back + 1, cfg.idle_forward),
    };
    if count == 0 {
        return None;
    }
    let lo = dominant.saturating_sub(back);
    let hi = (dominant + forward).min(count - 1);
    Some((lo, hi))
}

/// The highest-pressure phase of a set: merging the two strip axes (and the
/// zoom, which is folded in as a Fling) is a max of the phase rank.
pub fn merge_phases(phases: &[ScrollPhase]) -> ScrollPhase {
    phases
        .iter()
        .copied()
        .reduce(ScrollPhase::max_pressure)
        .unwrap_or(ScrollPhase::Idle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fling_paints_nothing_until_the_flush_reopens() {
        let cfg = PrefetchCfg::default();
        assert_eq!(paint_range(ScrollPhase::Fling, 3, 10, &cfg), None);
    }

    #[test]
    fn settling_opens_forward_only() {
        let cfg = PrefetchCfg::default();
        // Landed on page 4 (index 3): the page under the eyes plus the
        // forward depth, nothing behind (the reader was moving forward).
        assert_eq!(paint_range(ScrollPhase::Settling, 3, 10, &cfg), Some((3, 5)));
    }

    #[test]
    fn cruising_covers_back_and_forward() {
        let cfg = PrefetchCfg::default();
        assert_eq!(paint_range(ScrollPhase::Cruising, 3, 10, &cfg), Some((2, 5)));
    }

    #[test]
    fn idle_reaches_deeper_ahead() {
        let cfg = PrefetchCfg::default();
        // Sitting on page 4 (index 3): one page behind (back+1) and three
        // ahead (idle_forward) — the deep prefetch nobody is watching.
        assert_eq!(paint_range(ScrollPhase::Idle, 3, 10, &cfg), Some((1, 6)));
    }

    #[test]
    fn the_window_never_leaves_the_document() {
        let cfg = PrefetchCfg::default();
        // Three-page book, on the last page.
        assert_eq!(paint_range(ScrollPhase::Idle, 2, 3, &cfg), Some((0, 2)));
        // An empty document paints nothing.
        assert_eq!(paint_range(ScrollPhase::Idle, 0, 0, &cfg), None);
    }

    #[test]
    fn sitting_on_page_two_prefetches_three_and_four() {
        // The headline semantic, 0-based: dominant 1 is page 2.
        let cfg = PrefetchCfg::default();
        let (lo, hi) = paint_range(ScrollPhase::Cruising, 1, 10, &cfg).unwrap();
        assert!((lo..=hi).contains(&2) && (lo..=hi).contains(&3));
    }

    #[test]
    fn merge_takes_the_highest_pressure() {
        assert_eq!(
            merge_phases(&[ScrollPhase::Idle, ScrollPhase::Cruising, ScrollPhase::Fling]),
            ScrollPhase::Fling
        );
        assert_eq!(
            merge_phases(&[ScrollPhase::Settling, ScrollPhase::Idle]),
            ScrollPhase::Settling
        );
        assert_eq!(merge_phases(&[]), ScrollPhase::Idle);
    }
}
