//! Uniform multi-column grids, windowed **per row**.
//!
//! Rows are the windowing unit because a row's cells mount and unmount
//! together — evicting half a row would leave torn grids. [`Budget`]
//! therefore counts rows for a grid: [`Overscan::Items(n)`](crate::Overscan::Items)
//! pre-mounts `n` rows on each side (the thumbnail `ROW_BUFFER` pattern),
//! and `max_items` caps mounted rows.
//!
//! # Viewport awareness
//!
//! - **Height** decides *how many rows mount*: the window is derived as
//!   `visible rows + overscan`, so a taller viewport simply mounts more —
//!   nothing is hardcoded ("6 rows at this height" is a measurement, not a
//!   setting).
//! - **Width** decides *how many columns exist*: [`GridSpec::columns_at`]
//!   resolves the column count from the cross extent, either fixed or
//!   responsive (`floor((width + gap) / (min_col + gap))`). Column count is
//!   resolved at construction, so all queries stay O(1)/O(log n); a live
//!   container re-resolves by rebuilding the layout when its width changes.
//!
//! # Geometry
//!
//! `row_pitch` is the full row stride — cell height **plus** the gap below
//! the row (the thumbnails' `row_height()`). Item `i` lives in row
//! `i / columns`, column `i % columns`; its scroll-axis offset is its row's
//! offset and its cross offset is `col * (cell_width + gap_cross)`.

use crate::units::{from_sub, to_sub};
use crate::{Budget, Strip, Viewport, Window};

use super::Layout;

/// How the column count is decided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridColumns {
    /// A fixed number of columns (thumbnails: 2).
    Fixed(usize),
    /// Fit as many columns of at least `min_width` as the cross extent
    /// allows: `max(1, floor((cross + gap_cross) / (min_width + gap_cross)))`.
    Responsive {
        /// Minimum column width, in the same unit as everything else.
        min_width: f64,
    },
}

/// Static grid configuration (column policy + cross-axis gap).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridSpec {
    /// Column policy.
    pub columns: GridColumns,
    /// Gap between columns.
    pub gap_cross: f64,
}

impl GridSpec {
    /// Fixed column count.
    pub const fn fixed(columns: usize, gap_cross: f64) -> Self {
        Self {
            columns: GridColumns::Fixed(columns),
            gap_cross,
        }
    }

    /// Responsive column count from a minimum column width.
    pub const fn responsive(min_width: f64, gap_cross: f64) -> Self {
        Self {
            columns: GridColumns::Responsive { min_width },
            gap_cross,
        }
    }

    /// Resolve the column count for a live cross extent (viewport width).
    pub fn columns_at(&self, cross_extent: f64) -> usize {
        match self.columns {
            GridColumns::Fixed(n) => n.max(1),
            GridColumns::Responsive { min_width } => {
                if cross_extent <= 0.0 || min_width <= 0.0 {
                    return 1;
                }
                let n = (cross_extent + self.gap_cross) / (min_width + self.gap_cross);
                (n.floor() as usize).max(1)
            }
        }
    }
}

/// A uniform grid of `items` cells in `columns` columns, windowed per row.
#[derive(Debug, Clone, PartialEq)]
pub struct GridLayout {
    /// One strip entry per row, each sized `row_pitch`, zero gap (the pitch
    /// already includes the gap below the row).
    rows: Strip,
    spec: GridSpec,
    columns: usize,
    items: usize,
    row_pitch: f64,
    cell_width: f64,
}

impl GridLayout {
    /// Resolve `spec` against the live cross extent (viewport width) and
    /// build the grid. `row_pitch` is the full row stride (cell height +
    /// the gap below the row).
    ///
    /// For [`GridColumns::Responsive`], cells are sized to fill the width:
    /// `cell_width = (cross - (cols - 1) * gap_cross) / cols`.
    pub fn resolve(spec: GridSpec, items: usize, row_pitch: f64, cross_extent: f64) -> Self {
        let columns = spec.columns_at(cross_extent);
        let row_pitch = from_sub(to_sub(row_pitch));
        let rows_len = items.div_ceil(columns);
        let cell_width = if cross_extent > 0.0 {
            ((cross_extent - (columns - 1) as f64 * spec.gap_cross) / columns as f64).max(0.0)
        } else {
            0.0
        };
        Self {
            rows: Strip::uniform(rows_len, row_pitch, 0.0),
            spec,
            columns,
            items,
            row_pitch,
            cell_width,
        }
    }

    /// Fixed-column convenience (thumbnails: 2 columns of a known width).
    pub fn uniform(
        items: usize,
        columns: usize,
        row_pitch: f64,
        cell_width: f64,
        gap_cross: f64,
    ) -> Self {
        let cols = columns.max(1);
        let cross = cols as f64 * cell_width + (cols - 1) as f64 * gap_cross;
        Self::resolve(GridSpec::fixed(cols, gap_cross), items, row_pitch, cross)
    }

    /// The resolved column count.
    #[inline]
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// Number of resolved rows.
    #[inline]
    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    /// The spec this grid was resolved from (for re-resolution on resize).
    #[inline]
    pub fn spec(&self) -> &GridSpec {
        &self.spec
    }

    /// Full row stride (cell height + gap below).
    #[inline]
    pub fn row_pitch(&self) -> f64 {
        self.row_pitch
    }

    /// Resolved cell width.
    #[inline]
    pub fn cell_width(&self) -> f64 {
        self.cell_width
    }

    /// Row containing item `index`.
    #[inline]
    pub fn row_of(&self, index: usize) -> usize {
        index / self.columns
    }

    /// Column containing item `index`.
    #[inline]
    pub fn col_of(&self, index: usize) -> usize {
        index % self.columns
    }

    /// The item indices in `row` (the last row may be partial).
    pub fn row_items(&self, row: usize) -> core::ops::Range<usize> {
        let first = row * self.columns;
        first..(first + self.columns).min(self.items)
    }

    /// Scroll-axis offset of a row's top.
    #[inline]
    pub fn row_offset(&self, row: usize) -> f64 {
        self.rows.offset(row)
    }

    /// The **row** window for this scroll position — what a row-rendering
    /// consumer (a `<For>` over rows) mounts directly.
    pub fn rows_window(&self, scroll: f64, viewport: Viewport, budget: Budget) -> Option<Window> {
        self.rows.window(scroll, viewport.main, budget)
    }

    /// [`rows_window`](Self::rows_window) with a per-frame hint.
    pub fn rows_window_hinted(
        &self,
        scroll: f64,
        viewport: Viewport,
        budget: Budget,
        hint: &mut usize,
    ) -> Option<Window> {
        self.rows.window_hinted(scroll, viewport.main, budget, hint)
    }

    /// Expand a row window into an item window (clamping a partial last row).
    fn expand(&self, rows: Window) -> Window {
        debug_assert!(self.items > 0);
        Window {
            first: rows.first * self.columns,
            last: ((rows.last + 1) * self.columns - 1).min(self.items - 1),
        }
    }
}

impl Layout for GridLayout {
    #[inline]
    fn item_count(&self) -> usize {
        self.items
    }

    #[inline]
    fn total(&self) -> f64 {
        self.rows.total()
    }

    fn offset(&self, index: usize) -> f64 {
        if self.items == 0 {
            return 0.0;
        }
        self.rows.offset(index / self.columns)
    }

    fn size(&self, index: usize) -> f64 {
        if index >= self.items {
            0.0
        } else {
            self.row_pitch
        }
    }

    fn cross_offset(&self, index: usize) -> f64 {
        if index >= self.items {
            return 0.0;
        }
        self.col_of(index) as f64 * (self.cell_width + self.spec.gap_cross)
    }

    fn cross_size(&self, index: usize) -> f64 {
        if index >= self.items {
            0.0
        } else {
            self.cell_width
        }
    }

    fn index_at(&self, pos: f64) -> usize {
        if self.items == 0 {
            return 0;
        }
        (self.rows.index_at(pos) * self.columns).min(self.items - 1)
    }

    fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize {
        if self.items == 0 {
            *hint = 0;
            return 0;
        }
        let mut row_hint = *hint / self.columns;
        let row = self.rows.index_at_hinted(pos, &mut row_hint);
        let idx = (row * self.columns).min(self.items - 1);
        *hint = idx;
        idx
    }

    fn overlapping(&self, top: f64, extent: f64) -> Option<Window> {
        self.rows.overlapping(top, extent).map(|w| self.expand(w))
    }

    fn window(&self, scroll: f64, viewport: Viewport, budget: Budget) -> Option<Window> {
        self.rows_window(scroll, viewport, budget)
            .map(|w| self.expand(w))
    }

    fn window_hinted(
        &self,
        scroll: f64,
        viewport: Viewport,
        budget: Budget,
        hint: &mut usize,
    ) -> Option<Window> {
        if self.items == 0 {
            return None;
        }
        let mut row_hint = *hint / self.columns;
        let rows = self.rows_window_hinted(scroll, viewport, budget, &mut row_hint)?;
        *hint = (row_hint * self.columns).min(self.items - 1);
        Some(self.expand(rows))
    }

    fn dominant(&self, scroll: f64, extent: f64) -> usize {
        if self.items == 0 {
            return 0;
        }
        (self.rows.dominant(scroll, extent) * self.columns).min(self.items - 1)
    }

    fn set_size(&mut self, _index: usize, _new_size: f64) -> f64 {
        0.0
    }

    #[inline]
    fn item_size_hint(&self) -> f64 {
        self.row_pitch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Overscan;

    fn thumbs(items: usize) -> GridLayout {
        let pitch = 120.0 * (792.0 / 612.0) + 8.0;
        GridLayout::uniform(items, 2, pitch, 120.0, 12.0)
    }

    #[test]
    fn mapping_and_partial_last_row() {
        let g = thumbs(5);
        assert_eq!(g.rows_len(), 3);
        assert_eq!(g.columns(), 2);
        assert_eq!(g.row_of(3), 1);
        assert_eq!(g.col_of(3), 1);
        assert_eq!(g.row_items(0), 0..2);
        assert_eq!(g.row_items(2), 4..5, "last row is partial");
        assert_eq!(g.cross_offset(0), 0.0);
        assert_eq!(g.cross_offset(1), 132.0);
        assert_eq!(g.cross_size(0), 120.0);
        assert_eq!(g.offset(2), g.row_pitch());
        assert_eq!(g.total(), 3.0 * g.row_pitch());
    }

    #[test]
    fn thousand_item_grid_mounts_a_bounded_window() {
        let g = thumbs(1_000);
        assert_eq!(g.rows_len(), 500);
        let budget = Budget::items(2, 64);
        let vp = Viewport::new(720.0, 264.0);

        let top_rows = g.rows_window(0.0, vp, budget).expect("window at top");
        assert!(top_rows.len() < 20, "mounted {} rows", top_rows.len());

        let mid = g.row_pitch() * 200.0;
        let mid_rows = g.rows_window(mid, vp, budget).expect("window mid-grid");
        assert!(mid_rows.len() < 20);
        assert!(mid_rows.first > 0 && mid_rows.last < 499);

        let items = g.window(mid, vp, budget).unwrap();
        assert_eq!(items.first, mid_rows.first * 2);
        assert_eq!(items.last, (mid_rows.last + 1) * 2 - 1);
    }

    #[test]
    fn mounted_rows_follow_viewport_height() {
        let g = GridLayout::uniform(200, 2, 120.0, 100.0, 8.0);
        let exact = Budget {
            overscan: Overscan::Screenfuls(0.0),
            max_items: 1_000,
        };
        let top = g.row_pitch() * 16.0;

        let small = g
            .rows_window(top, Viewport::main_only(720.0), exact)
            .unwrap();
        let tall = g
            .rows_window(top, Viewport::main_only(1_440.0), exact)
            .unwrap();

        assert_eq!(
            small.len(),
            6,
            "720px / 120px rows = exactly 6 visible rows"
        );
        assert_eq!(tall.len(), 12, "double the height mounts double the rows");

        let buffered = g
            .rows_window(top, Viewport::main_only(720.0), Budget::items(1, 1_000))
            .unwrap();
        assert_eq!(buffered.len(), 8, "6 visible + 1 buffer row each side");
    }

    #[test]
    fn responsive_columns_follow_viewport_width() {
        let spec = GridSpec::responsive(120.0, 12.0);
        assert_eq!(spec.columns_at(264.0), 2);
        assert_eq!(spec.columns_at(540.0), 4);
        assert_eq!(spec.columns_at(1_000.0), 7);
        assert_eq!(spec.columns_at(50.0), 1, "never zero columns");
        assert_eq!(spec.columns_at(0.0), 1);

        let g = GridLayout::resolve(spec, 100, 150.0, 540.0);
        assert_eq!(g.columns(), 4);
        assert_eq!(g.rows_len(), 25);
        assert!((g.cell_width() - 126.0).abs() < 1e-9);
        assert_eq!(g.col_of(5), 1);
        assert!((g.cross_offset(5) - 138.0).abs() < 1e-9);
    }

    #[test]
    fn one_column_grid_is_a_list() {
        let g = GridLayout::uniform(30, 1, 100.0, 200.0, 0.0);
        let l: super::super::ListLayout = super::super::ListLayout::uniform(30, 100.0, 0.0);
        assert_eq!(g.total(), l.total());
        for i in 0..30 {
            assert_eq!(g.offset(i), l.offset(i), "offset {i}");
        }
        let budget = Budget::default();
        let mut pos = 0.0;
        while pos < g.total() {
            assert_eq!(
                g.window(pos, Viewport::main_only(350.0), budget),
                l.window(pos, Viewport::main_only(350.0), budget),
                "window disagreement at {pos}"
            );
            pos += 37.0;
        }
    }

    #[test]
    fn grid_boundary_semantics() {
        let g = GridLayout::uniform(9, 3, 120.0, 100.0, 8.0);
        let exact = Budget {
            overscan: Overscan::Screenfuls(0.0),
            max_items: 1_000,
        };
        let vp = Viewport::main_only(120.0);

        let w = g.window(120.0, vp, exact).unwrap();
        assert_eq!((w.first, w.last), (3, 5));
        let w0 = g.window(0.0, vp, exact).unwrap();
        assert_eq!((w0.first, w0.last), (0, 2));
    }

    #[test]
    fn dominant_is_first_item_of_dominant_row() {
        let g = thumbs(100);
        let pitch = g.row_pitch();
        let top = 4.0 * pitch - pitch * 0.25;
        assert_eq!(g.dominant(top, 300.0), 8);
    }

    #[test]
    fn hinted_matches_unhinted() {
        let g = thumbs(500);
        let mut hint = 0usize;
        let mut pos = 0.0;
        while pos < g.total() {
            assert_eq!(
                g.index_at(pos),
                g.index_at_hinted(pos, &mut hint),
                "index_at {pos}"
            );
            pos += 23.0;
        }
        let budget = Budget::items(2, 64);
        let mut hint = 0usize;
        let mut top = 0.0;
        while top < g.total() {
            assert_eq!(
                g.window(top, Viewport::main_only(720.0), budget),
                g.window_hinted(top, Viewport::main_only(720.0), budget, &mut hint),
                "window at {top}"
            );
            top += 61.0;
        }
    }

    #[test]
    fn degenerate_inputs() {
        let g = thumbs(0);
        assert!(g.is_empty());
        assert_eq!(g.total(), 0.0);
        assert_eq!(
            g.window(0.0, Viewport::main_only(720.0), Budget::default()),
            None
        );
        assert_eq!(g.dominant(0.0, 720.0), 0);

        let g = thumbs(10);
        assert_eq!(
            g.window(99_999.0, Viewport::main_only(720.0), Budget::default()),
            None
        );

        let g = thumbs(1);
        let w = g
            .window(0.0, Viewport::main_only(720.0), Budget::default())
            .unwrap();
        assert_eq!((w.first, w.last), (0, 0));
    }
}
