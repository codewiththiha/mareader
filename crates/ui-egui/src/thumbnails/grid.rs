//! Thumbnail grid geometry and spread-aware active-page alignment.

use std::ops::Range;

use egui::{Rect, Vec2, pos2, vec2};
use mareader_engine::pane::ViewMode;

use crate::documents::Reading;

/// A cell's page width; two fit the rail with room either side.
pub const CELL_W: f32 = 120.0;
pub(super) const COLUMNS: usize = 2;
pub(super) const GAP: f32 = 12.0;
pub(super) const PAD: f32 = 12.0;
/// The page number band under each page.
pub(super) const BAND: f32 = 22.0;
const ROW_GAP: f32 = 8.0;

/// Where each page's cell sits, from the first page's shape.
pub(super) struct Grid {
    left: f32,
    /// The box every page is fitted into.
    pub(super) cell: Vec2,
    pub(super) pitch: f32,
    pub(super) rows: usize,
    offset: usize,
    rtl: bool,
}

impl Grid {
    pub(super) fn new(
        width: f32,
        first: Option<(f32, f32)>,
        count: usize,
        (offset, rtl): (usize, bool),
    ) -> Self {
        // Mixed sizes share the first page's box; 3:4 if it has none.
        let aspect = first
            .map_or(4.0 / 3.0, |(w, h)| h / w.max(1.0))
            .clamp(0.3, 3.0);
        let cell = vec2(CELL_W, (CELL_W * aspect).round());
        let span = COLUMNS as f32 * CELL_W + (COLUMNS - 1) as f32 * GAP;
        let rows = if count == 0 {
            0
        } else {
            (count + offset).div_ceil(COLUMNS)
        };
        Self {
            left: ((width - span) / 2.0).max(PAD),
            cell,
            pitch: cell.y + BAND + ROW_GAP,
            rows,
            offset,
            rtl,
        }
    }

    pub(super) fn height(&self) -> f32 {
        2.0 * PAD + self.rows as f32 * self.pitch - ROW_GAP
    }

    /// The cell of `page`, page box and band, from the content's top left.
    pub(super) fn cell(&self, page: usize) -> Rect {
        let slot = page + self.offset;
        let (row, raw_col) = (slot / COLUMNS, slot % COLUMNS);
        let col = if self.rtl {
            COLUMNS - 1 - raw_col
        } else {
            raw_col
        };
        let min = pos2(
            self.left + col as f32 * (CELL_W + GAP),
            PAD + row as f32 * self.pitch,
        );
        Rect::from_min_size(min, self.cell + vec2(0.0, BAND))
    }

    /// The cells covering `pages`, united into one box for scrolling.
    pub(super) fn span(&self, mut pages: Range<usize>) -> Option<Rect> {
        let first = self.cell(pages.next()?);
        Some(pages.fold(first, |box_, page| box_.union(self.cell(page))))
    }

    /// Pages whose cells fall in `rows`.
    pub(super) fn pages(&self, rows: Range<usize>, count: usize) -> Range<usize> {
        let start = (rows.start * COLUMNS).saturating_sub(self.offset);
        let end = (rows.end * COLUMNS).saturating_sub(self.offset).min(count);
        start.min(end)..end
    }

    /// Rows meeting the band from `top` to `bottom`.
    pub(super) fn rows(&self, top: f32, bottom: f32) -> Range<usize> {
        let first = ((top - PAD) / self.pitch).floor().max(0.0) as usize;
        let last = ((bottom - PAD) / self.pitch).ceil().max(0.0) as usize + 1;
        first.min(self.rows)..last.min(self.rows)
    }
}

/// Leading blank slot and column order so spreads pair on one row.
pub(super) fn spread_align(reading: &Reading) -> (usize, bool) {
    if reading.mode != ViewMode::Spread {
        return (0, false);
    }
    let lead = usize::from(reading.document.lead() == 1);
    (lead, reading.document.right_to_left())
}

/// The pages currently shown in the reader: one, or the active spread pair.
pub(super) fn active_pages(
    mode: ViewMode,
    current: usize,
    lead: usize,
    count: usize,
) -> Range<usize> {
    if count == 0 {
        return 0..0;
    }
    let page = current.min(count - 1);
    let (first, shown) = mode.span(page, lead);
    first..first.saturating_add(shown).min(count)
}

pub(super) fn shown_pages(reading: &Reading, count: usize) -> Range<usize> {
    active_pages(
        reading.mode,
        reading.current,
        reading.document.lead(),
        count,
    )
}

/// The page's own shape, fitted and centred in the shared box.
pub(super) fn fitted(boxed: Rect, size: (f32, f32)) -> Rect {
    let (w, h) = (size.0.max(1.0), size.1.max(1.0));
    let scale = (boxed.width() / w).min(boxed.height() / h);
    Rect::from_center_size(boxed.center(), vec2(w * scale, h * scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_mode_marks_both_pages_respecting_lead_and_end() {
        assert_eq!(active_pages(ViewMode::Single, 2, 0, 10), 2..3);
        assert_eq!(active_pages(ViewMode::Vertical, 2, 0, 10), 2..3);
        assert_eq!(active_pages(ViewMode::Horizontal, 2, 0, 10), 2..3);
        assert_eq!(active_pages(ViewMode::Spread, 0, 0, 5), 0..2);
        assert_eq!(active_pages(ViewMode::Spread, 1, 0, 5), 0..2);
        assert_eq!(active_pages(ViewMode::Spread, 2, 0, 5), 2..4);
        assert_eq!(active_pages(ViewMode::Spread, 4, 0, 5), 4..5);
        assert_eq!(active_pages(ViewMode::Spread, 0, 1, 6), 0..1);
        assert_eq!(active_pages(ViewMode::Spread, 1, 1, 6), 1..3);
        assert_eq!(active_pages(ViewMode::Spread, 2, 1, 6), 1..3);
        assert_eq!(active_pages(ViewMode::Spread, 5, 1, 6), 5..6);
        assert_eq!(active_pages(ViewMode::Spread, 2, usize::MAX, 6), 2..3);
        assert_eq!(active_pages(ViewMode::Spread, 0, 0, 0), 0..0);
    }

    #[test]
    fn spread_grid_pairs_led_and_rtl_pages_and_unites_their_span() {
        let ltr = Grid::new(288.0, Some((100.0, 100.0)), 6, (1, false));
        assert_eq!(ltr.rows, 4);
        assert_eq!(ltr.pages(0..1, 6), 0..1);
        assert_eq!(ltr.pages(1..2, 6), 1..3);
        assert_eq!(ltr.pages(3..4, 6), 5..6);
        assert!(ltr.cell(0).left() > ltr.cell(1).left());
        assert_eq!(ltr.cell(1).top(), ltr.cell(2).top());
        assert_eq!(ltr.span(0..0), None);
        assert_eq!(ltr.span(1..3), Some(ltr.cell(1).union(ltr.cell(2))));
        let rtl = Grid::new(288.0, Some((100.0, 100.0)), 6, (1, true));
        assert!(rtl.cell(0).left() < rtl.cell(1).left());
        assert_eq!(rtl.cell(1).top(), rtl.cell(2).top());
        assert!(rtl.cell(2).left() < rtl.cell(1).left());
    }
}
