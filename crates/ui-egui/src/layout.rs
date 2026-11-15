//! Where pages sit: two scrolling strips and two paged frames.

use std::ops::Range;

use egui::{Align, Pos2, Rect, Vec2, pos2, vec2};
use mareader_session::PageSize;

/// Between pages in a strip unless No Gap; around the content.
pub const GAP: f32 = 12.0;
pub const MARGIN: f32 = 16.0;

use mareader_engine::pane::ViewMode;

/// What a fit measures: what shows together, at zoom 1.
pub fn fit_box(
    mode: ViewMode,
    sizes: &[PageSize],
    (page, lead): (usize, usize),
) -> Option<PageSize> {
    let (start, shown) = mode.span(page, lead);
    let first = *sizes.get(start).or(sizes.first())?;
    let second = sizes.get(start + 1).filter(|_| shown == 2);
    Some(second.map_or(first, |s| PageSize {
        width: first.width + s.width,
        height: first.height.max(s.height),
    }))
}

/// Everything a layout depends on; it rebuilds when one changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inputs {
    pub mode: ViewMode,
    /// The pane, and what a pinned bar covers of its top.
    pub frame: Vec2,
    pub inset: f32,
    pub zoom: f32,
    /// The paged modes' first page shown.
    pub focus: usize,
    /// Space between a strip's pages, and air beside the pages.
    pub gap: f32,
    pub margin: f32,
    /// Where vertical pages sit across a wider pane, `margin` off an edge.
    pub align: Align,
    /// A spread's first page sits right, as a right-to-left book's.
    pub rtl: bool,
    /// The pages a book wants alone before spreads pair them.
    pub lead: usize,
}

/// Page rects in content space, for the pages laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub inputs: Inputs,
    /// Rects of pages `first..first + rects.len()`.
    first: usize,
    rects: Vec<Rect>,
    pub size: Vec2,
}

impl Layout {
    /// One zoom for every page, so text stays one size.
    pub fn new(sizes: &[PageSize], inputs: Inputs) -> Self {
        let zoom = inputs.zoom;
        let scaled = |s: &PageSize| vec2(s.width, s.height) * zoom;
        let top = inputs.inset + MARGIN;
        let frame = inputs.frame;
        match inputs.mode {
            ViewMode::Vertical => {
                let (gap, side) = (inputs.gap, inputs.margin);
                let widest = sizes.iter().map(|s| scaled(s).x).fold(0.0, f32::max);
                let width = frame.x.max(widest + 2.0 * side);
                let mut y = top;
                let rects = sizes
                    .iter()
                    .map(|s| {
                        let size = scaled(s);
                        let x = match inputs.align {
                            Align::Min => side,
                            Align::Center => (width - size.x) / 2.0,
                            Align::Max => width - side - size.x,
                        };
                        let r = Rect::from_min_size(pos2(x, y), size);
                        y += size.y + gap;
                        r
                    })
                    .collect();
                let size = vec2(width, (y - gap + MARGIN).max(top + MARGIN));
                Self::with(inputs, 0, rects, size)
            }
            ViewMode::Horizontal => {
                let tallest = sizes.iter().map(|s| scaled(s).y).fold(0.0, f32::max);
                let height = frame.y.max(top + tallest + MARGIN);
                let (gap, room) = (inputs.gap, height - top - MARGIN);
                let mut x = MARGIN;
                let rects = sizes
                    .iter()
                    .map(|s| {
                        let size = scaled(s);
                        let r = Rect::from_min_size(pos2(x, top + (room - size.y) / 2.0), size);
                        x += size.x + gap;
                        r
                    })
                    .collect();
                let size = vec2((x - gap + MARGIN).max(2.0 * MARGIN), height);
                Self::with(inputs, 0, rects, size)
            }
            ViewMode::Single | ViewMode::Spread => {
                let (first, count) = inputs.mode.span(inputs.focus, inputs.lead);
                let shown: Vec<Vec2> = sizes.iter().skip(first).take(count).map(scaled).collect();
                let block = vec2(
                    shown.iter().map(|s| s.x).sum::<f32>(),
                    shown.iter().map(|s| s.y).fold(0.0, f32::max),
                ) + 2.0 * vec2(inputs.margin, MARGIN);
                // Centred in what the bar leaves, scrolling once larger.
                let room = vec2(frame.x, frame.y - inputs.inset);
                let at = pos2(
                    ((room.x - block.x) / 2.0).max(0.0) + inputs.margin,
                    inputs.inset + ((room.y - block.y) / 2.0).max(0.0) + MARGIN,
                );
                let mut x = at.x;
                let mut rects: Vec<Rect> = shown
                    .iter()
                    .map(|s| {
                        let r = Rect::from_min_size(pos2(x, at.y), *s);
                        x += s.x;
                        r
                    })
                    .collect();
                if inputs.rtl
                    && let [first, second] = rects.as_mut_slice()
                {
                    // The pair swaps sides, keeping its own widths.
                    let (left, w) = (first.left(), second.width());
                    *second = second.translate(vec2(left - second.left(), 0.0));
                    *first = first.translate(vec2(w, 0.0));
                }
                let size = vec2(frame.x.max(block.x), frame.y.max(inputs.inset + block.y));
                Self::with(inputs, first, rects, size)
            }
        }
    }

    fn with(inputs: Inputs, first: usize, rects: Vec<Rect>, size: Vec2) -> Self {
        Self {
            inputs,
            first,
            rects,
            size,
        }
    }

    pub fn zoom(&self) -> f32 {
        self.inputs.zoom
    }

    /// The pages laid out: all of them in a strip.
    pub fn pages(&self) -> Range<usize> {
        self.first..self.first + self.rects.len()
    }

    pub fn rect(&self, page: usize) -> Option<Rect> {
        self.rects.get(page.checked_sub(self.first)?).copied()
    }

    /// The pages meeting `viewport`, in content space.
    pub fn visible(&self, viewport: Rect) -> Range<usize> {
        let (first, end) = match self.inputs.mode {
            ViewMode::Vertical => (
                self.rects.partition_point(|r| r.bottom() <= viewport.top()),
                self.rects.partition_point(|r| r.top() < viewport.bottom()),
            ),
            ViewMode::Horizontal => (
                self.rects.partition_point(|r| r.right() <= viewport.left()),
                self.rects.partition_point(|r| r.left() < viewport.right()),
            ),
            ViewMode::Single | ViewMode::Spread => {
                let meets = |r: &Rect| r.intersects(viewport);
                let first = self.rects.iter().position(meets);
                let last = self.rects.iter().rposition(meets);
                match (first, last) {
                    (Some(a), Some(b)) => (a, b + 1),
                    _ => (0, 0),
                }
            }
        };
        self.first + first..self.first + end.max(first)
    }

    /// The page at a content point, and the point in page units.
    pub fn locate(&self, p: Pos2) -> Option<(usize, Vec2)> {
        let last = self.rects.len().checked_sub(1)?;
        let gap = self.inputs.gap;
        let i = match self.inputs.mode {
            ViewMode::Vertical => self.rects.partition_point(|r| r.bottom() + gap <= p.y),
            ViewMode::Horizontal => self.rects.partition_point(|r| r.right() + gap <= p.x),
            // A spread's pages touch: the one nearer the point.
            ViewMode::Single | ViewMode::Spread => self
                .rects
                .iter()
                .map(|r| r.distance_sq_to_pos(p))
                .enumerate()
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(0, |(i, _)| i),
        };
        let i = i.min(last);
        let rect = self.rects.get(i)?;
        Some((self.first + i, (p - rect.min) / self.zoom()))
    }

    /// Where a point `locate` found sits in this layout.
    pub fn place(&self, page: usize, at: Vec2) -> Option<Pos2> {
        Some(self.rect(page)?.min + at * self.zoom())
    }

    /// The scroll offset that starts the view at `page`.
    pub fn reveal(&self, page: usize) -> Option<Vec2> {
        let rect = self.rect(page)?;
        let frame = self.inputs.frame;
        let centred = ((self.size - frame) / 2.0).max(Vec2::ZERO);
        // Only the gap shows above a later page, not its neighbour.
        let before = if page == 0 { MARGIN } else { self.inputs.gap };
        let offset = match self.inputs.mode {
            ViewMode::Vertical => vec2(centred.x, rect.top() - self.inputs.inset - before),
            ViewMode::Horizontal => vec2(rect.left() - before, centred.y),
            ViewMode::Single | ViewMode::Spread => vec2(centred.x, 0.0),
        };
        Some(offset.max(Vec2::ZERO))
    }
}

#[cfg(test)]
mod tests {
    use mareader_engine::pane::Arrow;

    use super::*;

    const LETTER: PageSize = PageSize {
        width: 612.0,
        height: 792.0,
    };
    const WIDE: PageSize = PageSize {
        width: 792.0,
        height: 612.0,
    };

    fn inputs(mode: ViewMode, zoom: f32, focus: usize) -> Inputs {
        Inputs {
            mode,
            frame: vec2(1000.0, 800.0),
            inset: 0.0,
            zoom,
            focus,
            gap: GAP,
            margin: MARGIN,
            align: Align::Center,
            rtl: false,
            lead: 0,
        }
    }

    fn layout(mode: ViewMode, sizes: &[PageSize], zoom: f32, focus: usize) -> Layout {
        Layout::new(sizes, inputs(mode, zoom, focus))
    }

    #[test]
    fn a_column_off_centre_keeps_the_margin_from_its_edge() {
        let pages = [LETTER; 3];
        let centre = (1000.0 - LETTER.width) / 2.0;
        let right = 1000.0 - MARGIN - LETTER.width;
        for (align, x) in [
            (Align::Min, MARGIN),
            (Align::Center, centre),
            (Align::Max, right),
        ] {
            let inputs = Inputs {
                align,
                ..inputs(ViewMode::Vertical, 1.0, 0)
            };
            let l = Layout::new(&pages, inputs);
            assert_eq!(l.rect(1).map(|r| r.left()), Some(x), "{align:?}");
        }
    }

    #[test]
    fn a_right_to_left_spread_shows_its_first_page_right() {
        let sizes = [LETTER, WIDE, LETTER];
        let ltr = layout(ViewMode::Spread, &sizes, 1.0, 0);
        let rtl = Layout::new(
            &sizes,
            Inputs {
                rtl: true,
                ..inputs(ViewMode::Spread, 1.0, 0)
            },
        );
        let left = |l: &Layout, p| l.rect(p).map(|r| r.left());
        assert_eq!(left(&rtl, 1), left(&ltr, 0));
        assert_eq!(left(&rtl, 0), left(&ltr, 0).map(|x| x + WIDE.width));
        assert_eq!(rtl.rect(0).map(|r| r.width()), Some(LETTER.width));
    }

    #[test]
    fn spreads_pair_pages_from_the_first_and_steps_clamp() {
        let m = ViewMode::Spread;
        assert_eq!(
            (m.start(0, 0), m.start(1, 0), m.start(2, 0), m.start(5, 0)),
            (0, 0, 2, 4)
        );
        assert_eq!(m.turn(1, true, (5, 0)), 2);
        assert_eq!(m.turn(3, false, (5, 0)), 0);
        // The last spread of five pages holds page 5 alone.
        assert_eq!(m.turn(4, true, (5, 0)), 4);
        assert_eq!(m.turn(0, false, (5, 0)), 0);
        let s = ViewMode::Single;
        assert_eq!((s.turn(3, true, (4, 0)), s.turn(0, false, (4, 0))), (3, 0));
        assert_eq!(s.turn(0, true, (0, 0)), 0);
    }

    #[test]
    fn a_lead_page_shows_alone_before_spreads_pair() {
        let m = ViewMode::Spread;
        // A cover on the far side: 0 alone, then 1-2, 3-4.
        assert_eq!(
            (m.span(0, 1), m.span(2, 1), m.span(3, 1)),
            ((0, 1), (1, 2), (3, 2))
        );
        assert_eq!((m.turn(0, true, (5, 1)), m.turn(1, true, (5, 1))), (1, 3));
        assert_eq!((m.turn(3, false, (5, 1)), m.turn(1, false, (5, 1))), (1, 0));
        // A book wanting no spreads shows every page alone.
        assert_eq!(m.span(7, usize::MAX), (7, 1));
        assert_eq!(m.turn(2, true, (5, usize::MAX)), 3);
    }

    #[test]
    fn arrows_turn_except_along_a_strip() {
        assert_eq!(Arrow::Down.turns(ViewMode::Single, false), Some(true));
        assert_eq!(Arrow::Left.turns(ViewMode::Spread, false), Some(false));
        assert_eq!(Arrow::Right.turns(ViewMode::Vertical, false), Some(true));
        assert_eq!(Arrow::Down.turns(ViewMode::Vertical, false), None);
        assert_eq!(Arrow::Right.turns(ViewMode::Horizontal, false), None);
        assert_eq!(Arrow::Up.turns(ViewMode::Horizontal, false), None);
        // Right to left, Left turns on and Right back.
        assert_eq!(Arrow::Left.turns(ViewMode::Spread, true), Some(true));
        assert_eq!(Arrow::Right.turns(ViewMode::Single, true), Some(false));
        assert_eq!(Arrow::Right.turns(ViewMode::Vertical, true), Some(true));
    }

    #[test]
    fn a_spread_fits_both_pages_and_the_strips_one() {
        let sizes = [LETTER, WIDE, LETTER];
        let pair = fit_box(ViewMode::Spread, &sizes, (1, 0));
        assert_eq!(pair.map(|p| (p.width, p.height)), Some((1404.0, 792.0)));
        // The last spread's lone page fits alone.
        let lone = fit_box(ViewMode::Spread, &sizes, (2, 0));
        assert_eq!(lone.map(|p| p.width), Some(612.0));
        let one = fit_box(ViewMode::Vertical, &sizes, (1, 0));
        assert_eq!(one.map(|p| p.width), Some(792.0));
        assert_eq!(fit_box(ViewMode::Single, &[], (0, 0)), None);
        // A lead page alone fits alone; the pair after it together.
        let led = fit_box(ViewMode::Spread, &sizes, (0, 1));
        assert_eq!(led.map(|p| p.width), Some(612.0));
    }

    #[test]
    fn the_strip_runs_sideways_and_centres_each_page() {
        let l = layout(ViewMode::Horizontal, &[LETTER, WIDE], 0.5, 0);
        let (a, b) = (
            l.rect(0).unwrap_or(Rect::NOTHING),
            l.rect(1).unwrap_or(Rect::NOTHING),
        );
        assert_eq!(a.left(), MARGIN);
        assert_eq!(b.left(), MARGIN + 306.0 + GAP);
        assert_eq!(a.center().y, b.center().y);
        let wide = Rect::from_min_size(pos2(MARGIN + 310.0, 0.0), vec2(10.0, 800.0));
        assert_eq!(l.visible(wide), 1..2);
        assert_eq!(
            l.locate(pos2(MARGIN + 320.0, 400.0)).map(|(p, _)| p),
            Some(1)
        );
        assert_eq!(l.reveal(1).map(|o| o.x), Some(306.0 + MARGIN));
    }

    #[test]
    fn a_page_alone_centres_and_a_spread_touches() {
        let single = layout(ViewMode::Single, &[LETTER, LETTER, LETTER], 0.5, 1);
        assert_eq!(single.pages(), 1..2);
        let r = single.rect(1).unwrap_or(Rect::NOTHING);
        assert_eq!(r.center(), pos2(500.0, 400.0));
        let spread = layout(ViewMode::Spread, &[LETTER, LETTER, LETTER], 0.5, 1);
        assert_eq!(spread.pages(), 0..2);
        let (a, b) = (
            spread.rect(0).unwrap_or(Rect::NOTHING),
            spread.rect(1).unwrap_or(Rect::NOTHING),
        );
        assert_eq!(a.right(), b.left());
        assert_eq!(a.right(), 500.0);
        assert_eq!(spread.locate(pos2(520.0, 300.0)).map(|(p, _)| p), Some(1));
        // The lone last page.
        assert_eq!(layout(ViewMode::Spread, &[LETTER; 3], 0.5, 2).pages(), 2..3);
    }

    #[test]
    fn a_zoomed_page_scrolls_from_its_top_left_margin() {
        let l = layout(ViewMode::Single, &[LETTER], 2.0, 0);
        let r = l.rect(0).unwrap_or(Rect::NOTHING);
        assert_eq!(r.min, pos2(MARGIN, MARGIN));
        assert_eq!(l.size, r.size() + Vec2::splat(2.0 * MARGIN));
        assert_eq!(l.reveal(0), Some(vec2((l.size.x - 1000.0) / 2.0, 0.0)));
    }

    #[test]
    fn a_pinned_bar_pushes_paged_content_below_it() {
        let mut i = inputs(ViewMode::Single, 2.0, 0);
        i.inset = 48.0;
        let l = Layout::new(&[LETTER], i);
        assert_eq!(l.rect(0).map(|r| r.top()), Some(48.0 + MARGIN));
    }

    /// The old fixed-width column: `width` wide, pages from `top`.
    fn column(sizes: &[PageSize], zoom: f32, width: f32, top: f32) -> Layout {
        let mut i = inputs(ViewMode::Vertical, zoom, 0);
        (i.frame.x, i.inset) = (width, top - MARGIN);
        Layout::new(sizes, i)
    }

    fn first(l: &Layout) -> Rect {
        l.rect(0).unwrap_or(Rect::NOTHING)
    }

    #[test]
    fn a_column_lays_pages_at_its_zoom() {
        let c = column(&[LETTER; 3], 1.0, 644.0, 16.0);
        assert_eq!(
            first(&c),
            Rect::from_min_size(pos2(16.0, 16.0), vec2(612.0, 792.0))
        );
        assert_eq!(c.size, vec2(644.0, 16.0 + 3.0 * 792.0 + 2.0 * GAP + MARGIN));
    }

    #[test]
    fn wider_pages_keep_the_zoom_and_widen_the_content() {
        let c = column(&[LETTER, WIDE], 1.0, 644.0, 16.0);
        let plate = c.rect(1).unwrap_or(Rect::NOTHING);
        assert_eq!(plate.width(), 792.0);
        assert_eq!(c.size.x, 792.0 + 2.0 * MARGIN);
        // The narrower page is centred in the wider column.
        assert_eq!(first(&c).center().x, plate.center().x);
    }

    #[test]
    fn only_pages_meeting_the_viewport_are_visible() {
        let c = column(&[LETTER; 10_000], 1.0, 644.0, 16.0);
        let pitch = 792.0 + GAP;
        let at =
            |page: f32| Rect::from_min_size(pos2(0.0, 16.0 + page * pitch), vec2(644.0, 900.0));
        assert_eq!(c.visible(at(0.0)), 0..2);
        assert_eq!(c.visible(at(5_000.0)), 5_000..5_002);
        assert_eq!(c.visible(at(20_000.0)), 10_000..10_000);
        let gap_only = Rect::from_min_size(pos2(0.0, 16.0 + 792.0 + 1.0), vec2(9.0, 4.0));
        assert!(c.visible(gap_only).is_empty());
    }

    #[test]
    fn a_point_on_a_page_survives_a_relayout() {
        let old = column(&[LETTER; 20], 1.0, 644.0, 16.0);
        let new = column(&[LETTER, WIDE].repeat(10), 2.5, 900.0, 64.0);
        let spot = pos2(300.0, 16.0 + 7.0 * (792.0 + GAP) + 400.0);
        let (page, at) = old.locate(spot).unwrap_or_default();
        assert_eq!(page, 7);
        let moved = new.place(page, at).unwrap_or(Pos2::ZERO);
        let back = new.locate(moved).unwrap_or_default();
        assert_eq!(back.0, 7);
        assert!((back.1 - at).length() < 1e-3);
    }
}
