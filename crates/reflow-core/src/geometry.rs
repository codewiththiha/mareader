//! The reflowable page's geometry: A4 at 96dpi, and the book gutter.

const PAGE_WIDTH: f64 = 794.0;
pub const PAGE_HEIGHT: f64 = 1123.0;

const PAD: f64 = 72.0;
const GUTTER: f64 = 92.0;
const EDGE: f64 = 56.0;

const MIN_COLUMN_PCT: f64 = 60.0;
const MAX_COLUMN_PCT: f64 = 140.0;
/// The narrowest text column any dial combination may leave.
const MIN_CONTENT_WIDTH: f64 = 160.0;

/// Where a page sits relative to the spine while a book layout is on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SpineSide {
    /// Derive the side from the page's parity, recto/verso like a bound book.
    #[default]
    Auto,
    /// Fixed LEFT of the spine: the gutter faces right, toward its neighbour.
    Left,
    /// Fixed RIGHT of the spine (a spread's right-hand page): the gutter faces
    /// left.
    Right,
}

/// One page's geometry at scale 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeometry {
    pub width: f64,
    pub height: f64,
    pub pad_block: f64,
    pub pad_inline_left: f64,
    pub pad_inline_right: f64,
    /// Reader margin spent inside the card, on top of the pads above.
    pub extra_inline: f64,
    pub content_width: f64,
    pub content_height: f64,
}

impl PageGeometry {
    /// Inline paddings of `page`: the gutter faces the spine in book layout.
    fn inline_pads(&self, book_layout: bool, page: usize) -> (f64, f64) {
        if !book_layout {
            return (self.pad_inline_left, self.pad_inline_right);
        }
        if page.is_multiple_of(2) {
            (GUTTER, EDGE)
        } else {
            (EDGE, GUTTER)
        }
    }

    /// Inline paddings of a page fixed to one side of a spread.
    fn spread_pads(&self, book_layout: bool, right_page: bool) -> (f64, f64) {
        if !book_layout {
            return (self.pad_inline_left, self.pad_inline_right);
        }
        if right_page {
            (GUTTER, EDGE)
        } else {
            (EDGE, GUTTER)
        }
    }

    /// Spend `extra` px on both sides: the pads grow, the column shrinks.
    pub fn with_extra_inline(mut self, extra: f64) -> Self {
        self.extra_inline = extra.max(0.0);
        self.content_width = (self.content_width - 2.0 * self.extra_inline).max(MIN_CONTENT_WIDTH);
        let pads = self.pad_inline_left + self.pad_inline_right + 2.0 * self.extra_inline;
        self.width = pads + self.content_width;
        self
    }

    /// Scale the text column to `pct`, growing the page box with it.
    pub fn with_column_pct(mut self, pct: f64) -> Self {
        let factor = (pct / 100.0).clamp(MIN_COLUMN_PCT / 100.0, MAX_COLUMN_PCT / 100.0);
        self.content_width = (self.content_width * factor).max(MIN_CONTENT_WIDTH);
        let pads = self.pad_inline_left + self.pad_inline_right + 2.0 * self.extra_inline;
        self.width = pads + self.content_width;
        self
    }
}

impl PageGeometry {
    /// The inline paddings of `page` as it sits on the spine.
    pub fn pads(&self, book_layout: bool, page: usize, spine: SpineSide) -> (f64, f64) {
        let (left, right) = match spine {
            SpineSide::Auto => self.inline_pads(book_layout, page),
            SpineSide::Left => self.spread_pads(book_layout, false),
            SpineSide::Right => self.spread_pads(book_layout, true),
        };
        (left + self.extra_inline, right + self.extra_inline)
    }
}

impl Default for PageGeometry {
    /// The symmetric (non-book) geometry — what a fresh reader starts from.
    fn default() -> Self {
        geometry(false)
    }
}

/// The geometry for a layout choice.
pub fn geometry(book_layout: bool) -> PageGeometry {
    let (left, right) = if book_layout {
        (GUTTER, EDGE)
    } else {
        (PAD, PAD)
    };
    PageGeometry {
        width: PAGE_WIDTH,
        height: PAGE_HEIGHT,
        pad_block: PAD,
        pad_inline_left: left,
        pad_inline_right: right,
        extra_inline: 0.0,
        content_width: PAGE_WIDTH - left - right,
        content_height: PAGE_HEIGHT - 2.0 * PAD,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetric_pages_center_the_column() {
        let g = geometry(false);
        assert_eq!(g.width, PAGE_WIDTH);
        assert_eq!(g.height, PAGE_HEIGHT);
        assert_eq!(g.pad_inline_left, g.pad_inline_right);
        assert!((g.content_width - (PAGE_WIDTH - 2.0 * PAD)).abs() < 1e-9);
        assert!((g.content_height - (PAGE_HEIGHT - 2.0 * PAD)).abs() < 1e-9);
    }

    #[test]
    fn book_layout_alternates_the_gutter() {
        let g = geometry(true);
        // The gutter always faces the spine: left on recto pages, right
        // on verso ones.
        for page in 0..4usize {
            let (l, r) = g.inline_pads(true, page);
            if page % 2 == 0 {
                assert_eq!((l, r), (GUTTER, EDGE));
            } else {
                assert_eq!((l, r), (EDGE, GUTTER));
            }
        }
        // Content width is the same whichever side the gutter sits on.
        assert!((g.content_width - (PAGE_WIDTH - GUTTER - EDGE)).abs() < 1e-9);
        // Without a book layout the pads are the stored symmetric pair.
        assert_eq!(
            g.inline_pads(false, 1),
            (g.pad_inline_left, g.pad_inline_right)
        );
    }

    #[test]
    fn a_fixed_spine_side_overrides_the_parity() {
        let g = geometry(true);
        // The spread's left host is a verso whatever page number it carries.
        for page in 0..4usize {
            assert_eq!(g.pads(true, page, SpineSide::Left), (EDGE, GUTTER));
            assert_eq!(g.pads(true, page, SpineSide::Right), (GUTTER, EDGE));
            assert_eq!(
                g.pads(true, page, SpineSide::Auto),
                g.inline_pads(true, page)
            );
        }
    }

    #[test]
    fn the_reader_margin_widens_the_pads_inside_the_same_card() {
        let base = geometry(false);
        let g = base.with_extra_inline(16.0);
        // The card keeps its A4 box; the margin is spent inside it.
        assert_eq!(g.width, base.width);
        assert_eq!(g.pads(false, 3, SpineSide::Auto), (PAD + 16.0, PAD + 16.0));
        assert!((g.content_width - (base.content_width - 32.0)).abs() < 1e-9);
        // The gutter side carries it too, so a book answers the dial.
        let book = geometry(true).with_extra_inline(8.0);
        assert_eq!(
            book.pads(true, 0, SpineSide::Auto),
            (GUTTER + 8.0, EDGE + 8.0)
        );
        assert_eq!(
            book.pads(true, 1, SpineSide::Auto),
            (EDGE + 8.0, GUTTER + 8.0)
        );
    }

    #[test]
    fn an_absurd_margin_still_leaves_a_readable_column() {
        let g = geometry(false).with_extra_inline(500.0);
        assert_eq!(g.content_width, MIN_CONTENT_WIDTH);
        assert!(g.content_width > 0.0);
    }

    #[test]
    fn the_column_dial_grows_the_card_around_the_column() {
        let base = geometry(false);
        let wide = base.with_column_pct(140.0);
        // The card wraps the column: pads unchanged, both widths grown by
        // the same forty percent.
        assert!((wide.content_width - base.content_width * 1.4).abs() < 1e-9);
        assert!((wide.width - base.width - base.content_width * 0.4).abs() < 1e-9);
        assert_eq!(wide.height, base.height);
        assert_eq!(wide.pad_inline_left, base.pad_inline_left);
        // Narrow works the same way, and the dial clamps outside its range.
        let narrow = base.with_column_pct(60.0);
        assert!((narrow.content_width - base.content_width * 0.6).abs() < 1e-9);
        assert!((narrow.width - (PAD + PAD + narrow.content_width)).abs() < 1e-9);
        assert_eq!(base.with_column_pct(500.0).width, wide.width);
        assert_eq!(
            base.with_column_pct(10.0).content_width,
            narrow.content_width
        );
    }

    #[test]
    fn margin_and_column_dial_compose() {
        let g = geometry(true)
            .with_extra_inline(12.0)
            .with_column_pct(120.0);
        // Whatever order the dials land in, the card is column plus pads.
        assert!((g.width - (GUTTER + 12.0 + g.content_width + EDGE + 12.0)).abs() < 1e-9);
        assert!((g.content_width - (PAGE_WIDTH - GUTTER - EDGE - 24.0) * 1.2).abs() < 1e-9);
    }
}
