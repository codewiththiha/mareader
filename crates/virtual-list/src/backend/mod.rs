//! Shared backend trait: `StripBackend` defines the primitives every geometry
//! engine provides (`offset_sub`, `size_sub`, `total_sub`, `index_at_sub`,
//! `set_size_sub`). All windowing logic (`overlapping`, `visible`, `window`,
//! `dominant`) is written once against this trait, so a new backend — a tree,
//! a chunked column — only implements the primitives. The math stays in `i64`
//! sub-pixels (`to_sub` / `from_sub`) so boundary behavior is bit-for-bit
//! identical across backends.

use crate::units::{from_sub, to_sub};
use crate::window::{Budget, Slack, Window};

/// The primitive column geometry every backend provides, in sub-pixels.
pub trait StripBackend {
    /// How many items the backend holds.
    fn len(&self) -> usize;

    /// Whether the backend holds none; the default is `len() == 0`.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Gap between adjacent items, in sub-pixels.
    fn gap_sub(&self) -> i64;

    /// Offset of item `index` start, in sub-pixels. Returns total for
    /// `index >= len` (trailing-spacer friendly).
    fn offset_sub(&self, index: usize) -> i64;

    /// Size of item `index`, in sub-pixels. `0` out of range.
    fn size_sub(&self, index: usize) -> i64;

    /// Total extent (every item + gaps between them, no trailing gap), sub-px.
    fn total_sub(&self) -> i64;

    /// Index of the item whose span contains sub-pixel position `p`.
    /// Same leading-edge boundary rules as `Strip::index_at`.
    fn index_at_sub(&self, p: i64) -> usize;

    /// Set item `index` to `new_sub` (sub-pixels). Returns signed delta in sub-px.
    fn set_size_sub(&mut self, index: usize, new_sub: i64) -> i64;

    // f64 convenience wrappers (default implementations, can be overridden)
    /// Gap between adjacent items, in CSS pixels.
    fn gap(&self) -> f64 {
        from_sub(self.gap_sub())
    }

    /// Offset of the start of item `index`, in CSS pixels.
    fn offset(&self, index: usize) -> f64 {
        from_sub(self.offset_sub(index))
    }

    /// Size of item `index`, in CSS pixels. `0.0` out of range.
    fn size(&self, index: usize) -> f64 {
        from_sub(self.size_sub(index))
    }

    /// Total extent of the column: every item plus the gaps between them,
    /// with no trailing gap. `0.0` when empty.
    fn total(&self) -> f64 {
        from_sub(self.total_sub())
    }

    /// Average item extent — resolves [`crate::Overscan::Items`] budgets.
    fn mean_size(&self) -> f64 {
        let len = self.len();
        if len == 0 {
            0.0
        } else {
            self.total() / len as f64
        }
    }

    /// Index of the item whose span contains `pos` (f64 version).
    fn index_at(&self, pos: f64) -> usize {
        if pos <= 0.0 {
            return 0;
        }
        self.index_at_sub(to_sub(pos))
    }

    /// Set item `index` to `new_size` (f64). Returns signed delta in CSS pixels.
    fn set_size(&mut self, index: usize, new_size: f64) -> f64 {
        let new_sub = to_sub(new_size);
        let delta_sub = self.set_size_sub(index, new_sub);
        from_sub(delta_sub)
    }

    /// [`index_at`](Self::index_at) with a per-frame hint — the previous
    /// frame's answer, checked first. The default ignores the hint as a
    /// search seed and simply records the unhinted answer into it, so a
    /// custom backend that does not opt into the fast path still gets
    /// correct answers AND honest hint bookkeeping; [`Strip`] overrides it
    /// with the neighbour-then-gallop search.
    fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize {
        let index = self.index_at(pos);
        *hint = index;
        index
    }

    /// [`window`](window) with a per-frame hint. The default routes through
    /// the generic hinted windowing, which is correct for any backend — it
    /// leans on [`index_at_hinted`](Self::index_at_hinted), whose own
    /// default is simply the unhinted answer.
    fn window_hinted(
        &self,
        scroll_top: f64,
        viewport: f64,
        budget: Budget,
        hint: &mut usize,
    ) -> Option<Window> {
        window_hinted(self, scroll_top, viewport, budget, hint)
    }

    /// [`window_slack_hinted`](window_slack_hinted) for backends that want the
    /// padding split per side. The default is the shared implementation, which
    /// is written against this trait's primitives — so, like
    /// [`window_hinted`](Self::window_hinted), a backend that overrides
    /// neither gets correct answers and honest hint bookkeeping.
    fn window_slack_hinted(
        &self,
        scroll_top: f64,
        viewport: f64,
        slack: Slack,
        max_items: usize,
        hint: &mut usize,
    ) -> Option<Window> {
        window_slack_hinted(self, scroll_top, viewport, slack, max_items, hint)
    }
}

/// Shared `overlapping` — written once, identical for every backend.
/// Keeps the boundary-critical math in `i64` sub-pixels.
pub fn overlapping<B: StripBackend + ?Sized>(b: &B, top: f64, extent: f64) -> Option<Window> {
    let len = b.len();
    if len == 0 {
        return None;
    }
    let extent = extent.max(0.0);
    if extent == 0.0 {
        return None;
    }
    let top_sub = to_sub(top);
    let bottom_sub = to_sub(top + extent);

    let mut first = b.index_at_sub(top_sub);
    if b.offset_sub(first).saturating_add(b.size_sub(first)) <= top_sub {
        first += 1;
    }
    overlapping_from_first(b, first, bottom_sub)
}

/// [`overlapping`] with a per-frame hint for the LEADING item — the same
/// boundary rules, with the leading-edge search seeded from the previous
/// frame's answer (amortized `O(1)` for continuous scrolling). The trailing
/// binary search is shared with the unhinted path, so the two can never
/// disagree about where the window ends.
pub fn overlapping_hinted<B: StripBackend + ?Sized>(
    b: &B,
    top: f64,
    extent: f64,
    hint: &mut usize,
) -> Option<Window> {
    let len = b.len();
    if len == 0 {
        return None;
    }
    let extent = extent.max(0.0);
    if extent == 0.0 {
        return None;
    }
    let top_sub = to_sub(top);
    let bottom_sub = to_sub(top + extent);

    let mut first = b.index_at_hinted(top, hint);
    if b.offset_sub(first).saturating_add(b.size_sub(first)) <= top_sub {
        first += 1;
    }
    overlapping_from_first(b, first, bottom_sub)
}

/// The tail half of an overlap query, once the leading item is known:
/// boundary-check it, then binary-search the last item whose start is
/// strictly below `bottom_sub`. In `i64` sub-pixels so the boundary
/// behaviour is bit-for-bit identical for every backend and every entry
/// point.
fn overlapping_from_first<B: StripBackend + ?Sized>(
    b: &B,
    first: usize,
    bottom_sub: i64,
) -> Option<Window> {
    let len = b.len();
    if first >= len || b.offset_sub(first) >= bottom_sub {
        return None;
    }
    // Binary search for the last item whose start is strictly below bottom.
    let mut lo = first;
    let mut hi = len;
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if b.offset_sub(mid) < bottom_sub {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let last = lo;
    (last >= first).then_some(Window { first, last })
}

/// Shared `visible` — shorthand for `overlapping` with the raw viewport.
#[inline]
pub fn visible<B: StripBackend + ?Sized>(
    b: &B,
    scroll_top: f64,
    viewport: f64,
) -> Option<Window> {
    overlapping(b, scroll_top, viewport)
}

/// Shared `dominant` — the item occupying the most viewport area.
pub fn dominant<B: StripBackend + ?Sized>(b: &B, scroll_top: f64, viewport: f64) -> usize {
    if b.is_empty() {
        return 0;
    }
    if viewport <= 0.0 {
        return b.index_at(scroll_top);
    }
    let Some(win) = visible(b, scroll_top, viewport) else {
        return b.index_at(scroll_top);
    };
    let bottom = scroll_top + viewport;
    let mut best = win.first;
    let mut best_cover = -1.0;
    for i in win.first..=win.last {
        let top = b.offset(i);
        let cover = (top + b.size(i)).min(bottom) - top.max(scroll_top);
        if cover > best_cover {
            best_cover = cover;
            best = i;
        }
    }
    best
}

/// Shared `window` — visible + overscan, trimmed to budget.
pub fn window<B: StripBackend + ?Sized>(
    b: &B,
    scroll_top: f64,
    viewport: f64,
    budget: Budget,
) -> Option<Window> {
    window_slack(
        b,
        scroll_top,
        viewport,
        budget.slack(viewport.max(0.0), b.mean_size()),
        budget.max_items,
    )
}

/// [`window`] with the padding split per side of the viewport instead of
/// resolved from a [`Budget`]. Every windowing path funnels through here (and
/// its hinted twin), so a symmetric caller and a directional one share one
/// boundary rule and one trim.
pub fn window_slack<B: StripBackend + ?Sized>(
    b: &B,
    scroll_top: f64,
    viewport: f64,
    slack: Slack,
    max_items: usize,
) -> Option<Window> {
    if b.is_empty() {
        return None;
    }
    let vh = viewport.max(0.0);

    if vh == 0.0 {
        return (scroll_top < b.total()).then(|| Window {
            first: b.index_at(scroll_top),
            last: b.index_at(scroll_top),
        });
    }

    let padded = overlapping(b, scroll_top - slack.before, vh + slack.total())?;
    // What is strictly on screen must survive any trim.
    let vis = visible(b, scroll_top, vh);
    Some(trim_to_budget(padded, vis, max_items))
}

/// Shared `window_hinted` — [`window`] with a per-frame hint (amortized
/// `O(1)`). Everything except the leading-edge seed is the unhinted path:
/// the padded range resolves through [`overlapping_hinted`], the
/// strictly-visible range through the unhinted [`visible`], and the budget
/// trim is the one shared implementation — so the hinted and unhinted
/// windows can never disagree about what stays mounted.
pub fn window_hinted<B: StripBackend + ?Sized>(
    b: &B,
    scroll_top: f64,
    viewport: f64,
    budget: Budget,
    hint: &mut usize,
) -> Option<Window> {
    window_slack_hinted(
        b,
        scroll_top,
        viewport,
        budget.slack(viewport.max(0.0), b.mean_size()),
        budget.max_items,
        hint,
    )
}

/// [`window_slack`] with a per-frame hint. The zero-viewport degenerate case
/// seeds the hint through [`StripBackend::index_at_hinted`] so the bookkeeping
/// stays honest on a container that has not been measured yet.
pub fn window_slack_hinted<B: StripBackend + ?Sized>(
    b: &B,
    scroll_top: f64,
    viewport: f64,
    slack: Slack,
    max_items: usize,
    hint: &mut usize,
) -> Option<Window> {
    if b.is_empty() {
        return None;
    }
    let vh = viewport.max(0.0);

    if vh == 0.0 {
        return (scroll_top < b.total()).then(|| {
            let index = b.index_at_hinted(scroll_top, hint);
            Window {
                first: index,
                last: index,
            }
        });
    }

    let padded = overlapping_hinted(b, scroll_top - slack.before, vh + slack.total(), hint)?;
    // What is strictly on screen must survive any trim.
    let vis = visible(b, scroll_top, vh);
    Some(trim_to_budget(padded, vis, max_items))
}

/// The one budget trim — the invariant every windowing path answers
/// identically. What is strictly visible survives; the item furthest from
/// the viewport is evicted first; the item below it (in reading direction)
/// is the last to go. `vis` of `None` means nothing was strictly on screen,
/// so the padded range stands as it came.
fn trim_to_budget(padded: Window, vis: Option<Window>, max_items: usize) -> Window {
    let max = max_items.max(1);
    let mut first = padded.first;
    let mut last = padded.last;
    let Some(vis) = vis else {
        return Window { first, last };
    };
    while last - first + 1 > max {
        if first < vis.first {
            first += 1;
        } else if last > vis.last {
            last -= 1;
        } else {
            // Everything left is visible; the reader wins over the budget.
            break;
        }
    }
    Window { first, last }
}

pub mod strip;

pub use strip::Strip;

#[cfg(test)]
mod tests {
    use super::*;

    /// The trait's primitive half ONLY — no hinted overrides. A backend that
    /// opts out of the fast path must still get correct answers out of the
    /// default `index_at_hinted` / `window_hinted`. It forwards to a [`Strip`]
    /// through the f64 conveniences, which round-trip exactly for the clean
    /// sizes these tests use.
    struct Bare(Strip);

    impl Bare {
        fn new(sizes: impl IntoIterator<Item = f64>, gap: f64) -> Self {
            Self(Strip::new(sizes, gap))
        }
    }

    impl StripBackend for Bare {
        fn len(&self) -> usize {
            self.0.len()
        }

        fn gap_sub(&self) -> i64 {
            to_sub(self.0.gap())
        }

        fn offset_sub(&self, index: usize) -> i64 {
            to_sub(self.0.offset(index))
        }

        fn size_sub(&self, index: usize) -> i64 {
            to_sub(self.0.size(index))
        }

        fn total_sub(&self) -> i64 {
            to_sub(self.0.total())
        }

        fn index_at_sub(&self, p: i64) -> usize {
            self.0.index_at(from_sub(p))
        }

        fn set_size_sub(&mut self, index: usize, new_sub: i64) -> i64 {
            to_sub(self.0.set_size(index, from_sub(new_sub)))
        }
    }

    #[test]
    fn a_backend_without_overrides_gets_the_hinted_defaults_for_free() {
        let bare = Bare::new([100.0, 200.0, 150.0, 100.0, 200.0], 24.0);
        // The default index_at_hinted answers like index_at and records it.
        let want = bare.index_at(300.0);
        let mut hint = 0usize;
        assert_eq!(StripBackend::index_at_hinted(&bare, 300.0, &mut hint), want);
        assert_eq!(hint, want);
        // And the default window_hinted agrees with the unhinted window.
        let budget = Budget::screenfuls(0.5, 4);
        let mut hint = 0usize;
        let mut top = 0.0;
        while top < bare.total() {
            assert_eq!(
                StripBackend::window_hinted(&bare, top, 300.0, budget, &mut hint),
                window(&bare, top, 300.0, budget),
                "default hinted window disagrees at top={top}"
            );
            top += 47.0;
        }
    }
}

#[cfg(test)]
mod slack_tests {
    //! The directional half of the windowing: what a per-side padding buys,
    //! and the one invariant it must not break — the reader's own items are
    //! mounted whatever the two sides disagree about.

    use super::*;
    use crate::{GridLayout, GridSpec, Layout, ListLayout, Viewport};

    fn strip() -> Strip {
        Strip::new(core::iter::repeat_n(100.0, 200), 0.0)
    }

    #[test]
    fn a_symmetric_slack_is_the_budget_window() {
        // The asymmetric path has to agree with the symmetric one exactly when
        // the two sides are equal, or every existing caller's window would
        // move the moment it is routed through it.
        let b = strip();
        let budget = Budget::screenfuls(0.5, 9);
        let mut hint_a = 0usize;
        let mut hint_b = 0usize;
        let mut top = 0.0;
        while top < 19_000.0 {
            let want = window_hinted(&b, top, 400.0, budget, &mut hint_a);
            let got = window_slack_hinted(
                &b,
                top,
                400.0,
                budget.slack(400.0, b.mean_size()),
                budget.max_items,
                &mut hint_b,
            );
            assert_eq!(got, want, "at top={top}");
            assert_eq!(hint_a, hint_b, "hint bookkeeping at top={top}");
            top += 137.0;
        }
    }

    #[test]
    fn an_asymmetric_slack_reaches_one_side_further() {
        let b = strip();
        let viewport = 400.0;
        let top = 5_000.0;
        let slack = Slack::split(100.0, 500.0);
        let window = window_slack(&b, top, viewport, slack, 100).expect("a window");
        // 4900..5900 of content — the viewport's 400px plus 100 above and 500
        // below — which is items 49..=58. The same viewport padded by the
        // smaller of the two on both sides stops four items short.
        assert_eq!(window.first, 49);
        assert_eq!(window.last, 58);
        let symmetric = window_slack(&b, top, viewport, Slack::symmetric(100.0), 100).unwrap();
        assert_eq!(symmetric, Window { first: 49, last: 54 });
        // And mirrored, the same slack reaches the other way.
        let mirrored = window_slack(&b, top, viewport, Slack::split(500.0, 100.0), 100).unwrap();
        assert_eq!(mirrored.first, 45);
        assert_eq!(mirrored.last, 54);
    }

    #[test]
    fn the_ceiling_trims_the_far_side_and_never_the_viewport() {
        let b = strip();
        // Four screens of look-ahead, a ceiling of five items, and a viewport
        // holding four: the trim has to give up the look-ahead, not the reader.
        let window = window_slack(&b, 5_000.0, 400.0, Slack::split(0.0, 1_600.0), 5).unwrap();
        assert_eq!(window.len(), 5);
        let visible = visible(&b, 5_000.0, 400.0).unwrap();
        assert!(window.first <= visible.first && window.last >= visible.last);
        // Everything the ceiling gave up came off the far side.
        assert_eq!(window.first, visible.first);
    }

    #[test]
    fn a_negative_padding_clamps_to_zero() {
        let b = strip();
        let clamped = window_slack(&b, 5_000.0, 400.0, Slack::split(-900.0, -1.0), 100).unwrap();
        let plain = window_slack(&b, 5_000.0, 400.0, Slack::symmetric(0.0), 100).unwrap();
        assert_eq!(clamped, plain);
        assert_eq!(Slack::split(-5.0, 7.0), Slack::split(0.0, 7.0));
        assert_eq!(Slack::symmetric(-5.0).total(), 0.0);
        assert_eq!(Slack::split(3.0, 9.0).total(), 12.0);
    }

    #[test]
    fn a_zero_viewport_still_answers_one_item() {
        let b = strip();
        let window = window_slack(&b, 1_250.0, 0.0, Slack::split(1_000.0, 1_000.0), 100).unwrap();
        assert_eq!(window, Window { first: 12, last: 12 });
        let mut hint = 0usize;
        let hinted =
            window_slack_hinted(&b, 1_250.0, 0.0, Slack::split(1_000.0, 1_000.0), 100, &mut hint)
                .unwrap();
        assert_eq!(hinted, window);
        assert_eq!(hint, 12);
        // Nothing mounted at all past the end.
        assert!(window_slack(&b, 40_000.0, 0.0, Slack::symmetric(100.0), 100).is_none());
    }

    #[test]
    fn a_grid_windows_rows_in_the_direction_of_travel() {
        // Two columns of 100px rows: the slack applies to the ROW strip, and
        // the row window expands to items, so a directional window mounts whole
        // rows on both axes of the grid.
        let grid = GridLayout::resolve(GridSpec::fixed(2, 0.0), 40, 100.0, 200.0);
        let viewport = Viewport::new(300.0, 200.0);
        let mut hint = 0usize;
        let window = grid
            .window_slack_hinted(1_000.0, viewport, Slack::split(0.0, 300.0), 20, &mut hint)
            .expect("a window");
        // Rows 10..=15 (1000..1600 of content), expanded to items 20..=31.
        assert_eq!(window.first, 20);
        assert_eq!(window.last, 31);
        assert_eq!(window.first % 2, 0, "a row's cells mount together");
        let symmetric = grid
            .window_slack_hinted(1_000.0, viewport, Slack::symmetric(0.0), 20, &mut 0usize)
            .unwrap();
        assert!(window.last > symmetric.last);
        assert_eq!(window.first, symmetric.first);
    }

    #[test]
    fn a_list_layout_routes_the_slack_to_its_backend() {
        // The Layout contract, so a caller holding a LayoutKind gets the same
        // answer as one holding the strip.
        let layout: ListLayout<Strip> = ListLayout::uniform(200, 100.0, 0.0);
        let viewport = Viewport::main_only(400.0);
        let mut hint = 0usize;
        let window = layout
            .window_slack_hinted(5_000.0, viewport, Slack::split(100.0, 500.0), 100, &mut hint)
            .unwrap();
        let b = strip();
        let mut other = 0usize;
        assert_eq!(
            window,
            window_slack_hinted(&b, 5_000.0, 400.0, Slack::split(100.0, 500.0), 100, &mut other)
                .unwrap()
        );
    }
}
