//! Navigation, zoom/mode transitions, sheet settling, and seek on `PdfView`.

use egui::{Pos2, Rect, Vec2, vec2};

use super::*;

/// Along a strip, a page of another size refits once read this long.
const RESIZE_SETTLE: f64 = 0.25;
/// How long a resized pane holds still before its book is set anew.
const SHEET_SETTLE: f64 = 0.3;
/// Air a revealed hit keeps from the other edges.
const SEEK_MARGIN: f32 = 24.0;

impl PdfView {
    /// The pane-sized page its book is set on; none is the A4 card.
    pub fn sheet(&self) -> Option<(f32, f32)> {
        self.sheet
    }

    /// Takes `want` once it holds still; else how long to wait.
    pub(super) fn settle_sheet(&mut self, want: Option<(f32, f32)>, now: f64) -> Option<f64> {
        if want == self.sheet {
            self.sheet_want = None;
            return None;
        }
        let since = match self.sheet_want {
            Some((w, at)) if w == want => at,
            _ => {
                self.sheet_want = Some((want, now));
                now
            }
        };
        // A first page or a mode change sets at once; resizes settle.
        let quiet = now - since;
        if self.sheet.is_some() && want.is_some() && quiet < SHEET_SETTLE {
            return Some(SHEET_SETTLE - quiet);
        }
        self.sheet = want;
        self.sheet_want = None;
        self.sheet_moved = true;
        None
    }

    /// The place the last re-set landed on, while it's still read.
    pub fn place(&self, at: &Place) -> Option<mareader_session::Anchor> {
        // A strip holds it while unmoved since; scrolled, it reads elsewhere.
        if at.mode == ViewMode::Vertical {
            let still = |at: Vec2| (at - self.offset).length() < 2.0;
            return self
                .place
                .filter(|_| self.placed_at.is_some_and(still))
                .map(|(a, _)| a);
        }
        self.place
            .filter(|(_, page)| *page == at.current)
            .map(|(a, _)| a)
    }

    /// A strip's reading line: the page at the view's top, and how far down.
    pub fn reading_line(&self, at: &Place) -> Option<(usize, f32)> {
        let layout = self
            .layout
            .as_ref()
            .filter(|_| at.mode == ViewMode::Vertical)?;
        let top = vec2(self.viewport.x / 2.0, layout.inputs.inset);
        let (page, at) = layout.locate(Pos2::ZERO + self.offset + top)?;
        Some((page, at.y.max(0.0)))
    }

    /// Shows height `y` of `page` at a strip's top, `air` below its edge.
    pub fn land(&mut self, at: &Place, page: usize, y: f32, air: f32) {
        self.land = (at.mode == ViewMode::Vertical).then_some((page, y, air));
    }

    /// Keeps `place` as the page being read's, as a re-set does.
    pub fn hold_place(&mut self, at: &Place, place: Option<mareader_session::Anchor>) {
        self.place = place.map(|a| (a, at.current));
    }

    /// Shows the book set again as `doc`, from `page`; mode and zoom stay.
    pub fn retypeset(
        &mut self,
        at: &mut Place,
        doc: DocId,
        page: usize,
        count: usize,
        place: Option<mareader_session::Anchor>,
    ) {
        let page = page.min(count.saturating_sub(1));
        at.doc = doc;
        // Page rects, selection and motion belong to the old pages.
        self.layout = None;
        at.selection = None;
        self.sheets.clear();
        at.visible = page..page + 1;
        (self.jump, self.aim, self.hold, self.nudge) = (None, None, None, None);
        self.scroll_to = None;
        self.resize = None;
        self.zoom.fit_page = page;
        self.place = place.map(|a| (a, page));
        (self.placed_at, self.land) = (None, None);
        self.go(at, page);
    }

    /// Moves to `doc`, results laid in or out; false if the page went.
    pub fn respliced(
        &mut self,
        at: &mut Place,
        doc: DocId,
        count: usize,
        map: impl Fn(usize) -> Option<usize>,
    ) -> bool {
        let Some(page) = map(at.current) else {
            return false;
        };
        let centre = Pos2::ZERO + self.offset + self.viewport / 2.0;
        let spot = self.layout.as_ref().and_then(|l| {
            l.locate(centre).or_else(|| {
                let rect = l.rect(at.current)?;
                Some((at.current, (centre - rect.min) / l.zoom().max(0.01)))
            })
        });
        let keep = spot.and_then(|(p, at)| Some((map(p)?, at)));
        let anchor = self.place.and_then(|(a, p)| Some((a, map(p)?)));
        let hold = self.hold;
        let selection = at.selection.and_then(|sel| {
            Some(mareader_session::Selection {
                anchor: mareader_session::Caret {
                    page: map(sel.anchor.page)?,
                    ..sel.anchor
                },
                focus: mareader_session::Caret {
                    page: map(sel.focus.page)?,
                    ..sel.focus
                },
            })
        });
        let panned = self.offset != Vec2::ZERO;
        let paged = at.mode.paged();
        self.retypeset(at, doc, page, count, None);
        at.selection = selection;
        self.hold = hold;
        self.place = anchor;
        self.keep = keep.filter(|_| !paged || panned);
        true
    }

    /// Visible-page rendering hint to prime a retypeset off the UI thread.
    pub fn prime_hint(&self, place: &Place, doc: &Document) -> Option<mareader_session::Prime> {
        let zoom = self.zoom.sharp();
        if !zoom.is_finite() || zoom <= 0.0 || self.ppp <= 0.0 {
            return None;
        }
        let wanted = Scale::covering(zoom * self.ppp);
        let sizes = doc.page_sizes();
        let size = sizes
            .get(place.current)
            .or_else(|| sizes.first())
            .copied()?;
        let long = size.width.max(size.height).max(1.0);
        let cap = Scale::within(
            doc.fitted_scale(place.current, self.max_side / long)
                .unwrap_or(1.0),
        );
        if wanted > cap {
            return None;
        }
        let pages = if place.mode == ViewMode::Single { 1 } else { 2 };
        Some(mareader_session::Prime {
            scale: wanted,
            pages,
            spread: place.mode == ViewMode::Spread,
        })
    }

    pub fn command(&mut self, at: &mut Place, command: ZoomCommand, now: f64) {
        let shown = self.layout.as_ref().map(Layout::zoom);
        let shown = shown.filter(|_| self.ease);
        self.zoom.command(command, shown, at.current, now);
        at.zoom = self.zoom.sharp();
    }

    /// Switches mode on the page being read; `fit` fits the paged modes.
    pub fn set_mode(&mut self, at: &mut Place, mode: ViewMode, now: f64) {
        if mode == at.mode {
            return;
        }
        self.apply_mode(at, mode, now);
    }

    /// Applies `mode`'s zoom and focus updates after the engine changed mode.
    pub fn apply_mode(&mut self, at: &mut Place, mode: ViewMode, now: f64) {
        at.mode = mode;
        self.hold = None;
        // Single pages and spreads fit their width; the strip never.
        if mode.paged() {
            self.zoom
                .command(ZoomCommand::Fit(Fit::Width), None, at.current, now);
        } else if mode == ViewMode::Horizontal {
            self.zoom.unfit();
            self.zoom.set_basis(Fit::Page);
        } else if self.zoom.fit.is_none() {
            self.zoom.set_basis(Fit::Width);
        }
        at.zoom = self.zoom.sharp();
        self.go(at, at.current);
    }

    /// Shift+A: auto-scroll on or off; false in a paged mode.
    pub fn toggle_drift(&mut self, at: &Place, now: f64) -> bool {
        if at.mode.paged() {
            return false;
        }
        self.drift = match self.drift {
            Some(_) => None,
            None => Some(now),
        };
        true
    }

    /// Stops auto-scroll; true if it was running.
    pub fn stop_drift(&mut self) -> bool {
        self.drift.take().is_some()
    }

    pub fn drifting(&self) -> bool {
        self.drift.is_some()
    }

    /// An arrow key: a turn, or along a strip a line and then a glide.
    pub fn arrow(&mut self, at: &mut Place, arrow: Arrow, count: usize, now: f64) {
        if let Some(forward) = arrow.turns(at.mode, at.rtl) {
            let to = at.mode.turn(at.current, forward, (count, at.lead));
            self.go(at, to);
            return;
        }
        // Up and down across the strip do nothing, as in mareader.
        let sideways = matches!(arrow, Arrow::Left | Arrow::Right);
        if sideways != (at.mode == ViewMode::Horizontal) {
            return;
        }
        // The key repeating while held: the glide is already on its way.
        if self.hold.is_some_and(|h| h.arrow == arrow) {
            return;
        }
        self.hold = Some(Hold {
            arrow,
            since: now,
            last: now,
        });
        self.nudge = Some(Nudge::Line(arrow.sign()));
    }

    /// A page key: nearly a screen along a strip, a turn when paged.
    pub fn page_key(&mut self, at: &mut Place, forward: bool, count: usize) {
        if at.mode.paged() {
            self.turn(at, forward, count);
        } else {
            self.nudge = Some(Nudge::Screen(if forward { 1.0 } else { -1.0 }));
        }
    }

    /// Where the backdrop reads its paper: pages weighted by their share.
    pub fn paper_at(&self, at: &Place) -> f64 {
        self.paper_at.unwrap_or(at.current as f64)
    }

    /// Last frame's scroll, in content points; narrower pages sit centred.
    pub fn offset(&self) -> Vec2 {
        self.offset
    }

    /// A page turn from a button: it turns in every mode.
    pub fn turn(&mut self, at: &mut Place, forward: bool, count: usize) {
        let to = at.mode.turn(at.current, forward, (count, at.lead));
        self.go(at, to);
    }

    /// Jumps to `page`, kept within the document's `count`.
    pub fn go_to(&mut self, at: &mut Place, page: usize, count: usize) {
        self.go(at, page.min(count.saturating_sub(1)));
    }

    /// True while a page turn or reveal still glides.
    pub fn gliding(&self) -> bool {
        self.jump.is_some() || self.reveal.is_some() || self.aim.is_some()
    }

    /// Shows `spot` on `page`, `clear` above the foot; false if unknown.
    pub fn seek(
        &mut self,
        at: &mut Place,
        page: usize,
        spot: Option<Bounds>,
        band: Vec2,
        count: usize,
        clear: f32,
    ) -> bool {
        // Already heading there: going again would restart the glide.
        let heading = self.jump.is_some() || self.reveal.is_some();
        let shows = at.visible.contains(&page);
        let paged = at.mode.paged();
        if paged || spot.is_none() || self.layout.is_none() {
            if !shows && !heading {
                self.go_to(at, page, count);
            }
            return paged;
        }
        let (Some(layout), Some(b)) = (&self.layout, spot) else {
            return false;
        };
        let Some(rect) = layout.rect(page) else {
            return false;
        };
        let zoom = layout.zoom();
        // Only the vertical strip cuts pages to their bands.
        let band = if at.mode == ViewMode::Vertical {
            band
        } else {
            Vec2::ZERO
        };
        let origin = rect.min - band * zoom;
        let at = |x: f32, y: f32| origin + vec2(x, y) * zoom;
        let hit = Rect::from_min_max(at(b.x0, b.y0), at(b.x1, b.y1));
        let (o, v) = (self.offset, self.viewport);
        // The find bar floats over the pane's top, under any pinned bar.
        let top = layout.inputs.inset + FIND_CLEARANCE;
        let y = reveal(
            hit.top()..hit.bottom(),
            o.y + top..o.y + v.y - SEEK_MARGIN - clear,
            o.y,
        );
        let x = reveal(
            hit.left()..hit.right(),
            o.x + SEEK_MARGIN..o.x + v.x - SEEK_MARGIN,
            o.x,
        );
        if x.is_some() || y.is_some() {
            self.scroll_to = Some((vec2(x.unwrap_or(o.x), y.unwrap_or(o.y)), 2));
            self.jump = None;
            self.aim = None;
            self.reveal = None;
        }
        true
    }

    /// A page of another size: a fit refits; `keep` lets a set zoom too.
    pub(super) fn follow_size(
        &mut self,
        at: &mut Place,
        sizes: &[PageSize],
        keep: bool,
        now: f64,
    ) -> Option<f64> {
        let page = at.current;
        let fit_box = |p| layout::fit_box(at.mode, sizes, (p, at.lead));
        let (was, here) = (fit_box(self.zoom.fit_page), fit_box(page));
        if was == here {
            self.resize = None;
            return None;
        }
        let fitted = self.zoom.fit.is_some();
        // A set zoom not kept holds, measured from the page read.
        if !fitted && !keep {
            self.zoom.fit_page = page;
            self.resize = None;
            return None;
        }
        // A refit mid-glide would move the glide's end; it waits.
        if self.jump.is_some() || self.aim.is_some() {
            self.resize = None;
            return None;
        }
        // The zoom then measures the new page, a kept one as a fit does.
        let land = |view: &mut Self| {
            view.zoom.fit_page = page;
            view.resize = None;
        };
        // A turn lands at once, so it refits at once: no wrong size shows.
        if at.mode.paged() {
            land(self);
            return None;
        }
        match self.resize {
            Some((waiting, since)) if waiting == page => {
                let left = RESIZE_SETTLE - (now - since);
                if left > 0.0 {
                    return Some(left);
                }
                land(self);
                None
            }
            _ => {
                self.resize = Some((page, now));
                Some(RESIZE_SETTLE)
            }
        }
    }

    /// Shows `place`'s `page` from its start, or the nearest page study
    /// mode holds the pane to.
    fn go(&mut self, at: &mut Place, page: usize) {
        let page = self.study_span.clamp(page);
        self.focus = at.mode.start(page, at.lead);
        at.current = page;
        self.reveal = Some(if at.mode.paged() { self.focus } else { page });
    }
}

/// Set anew, a strip starts at the place read, in the pages' first frame.
pub(super) fn land(view: &mut PdfView) {
    let Some((page, y, air)) = view.land else {
        return;
    };
    let Some(layout) = view.layout.as_ref() else {
        return;
    };
    view.land = None;
    let Some(rect) = layout.rect(page) else {
        return;
    };
    let x = view.scroll_to.map_or(view.offset.x, |(to, _)| to.x);
    let top = rect.top() + y * layout.zoom() - layout.inputs.inset - air;
    let to = vec2(x, top).max(Vec2::ZERO);
    view.scroll_to = Some((to, 2));
    view.placed_at = Some(to);
    (view.jump, view.aim) = (None, None);
}

/// The offset keeping the point under the pivot there in `new`.
pub(super) fn hold(old: &Layout, new: &Layout, offset: Vec2, pivots: (Vec2, Vec2)) -> Option<Vec2> {
    let (page, at) = old.locate(Pos2::ZERO + offset + pivots.0)?;
    let p = new.place(page, at)?;
    Some((p - pivots.1).to_vec2().max(Vec2::ZERO))
}

/// A paged book's page: what the pane leaves each page, at its zoom.
pub(super) fn sheet_for(
    view: &PdfView,
    at: &Place,
    doc: &Document,
    room: Vec2,
) -> Option<(f32, f32)> {
    if !doc.is_reflowable() || !at.mode.paged() {
        return None;
    }
    let count = if at.mode == ViewMode::Spread {
        2.0
    } else {
        1.0
    };
    // A fit lands at 1; a set zoom makes the page's text larger.
    let zoom = match view.zoom.fit {
        Some(_) => 1.0,
        None => view.zoom.state().zoom.max(0.1),
    };
    let size = room / zoom;
    let (w, h) = ((size.x / count).floor(), size.y.floor());
    (w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0).then_some((w, h))
}

/// The strips' scroll along their axis; the paged modes' page.
pub(super) fn progress(at: &Place, layout: &Layout, out: &ScrollOutput<()>, count: usize) -> f32 {
    let room = (out.content_size - out.inner_rect.size()).max(Vec2::ZERO);
    let (done, total) = match at.mode {
        ViewMode::Vertical => (out.state.offset.y, room.y),
        ViewMode::Horizontal => (out.state.offset.x, room.x),
        ViewMode::Single | ViewMode::Spread => {
            let last = layout.pages().end.max(1);
            return last as f32 / count.max(1) as f32;
        }
    };
    if total <= 0.5 {
        1.0
    } else {
        (done / total).clamp(0.0, 1.0)
    }
}
