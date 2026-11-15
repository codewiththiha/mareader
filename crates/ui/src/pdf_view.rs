//! A PDF in a pane, in any view mode, drawn as pages arrive.

use egui::emath::GuiRounding as _;
use egui::scroll_area::{DragScroll, ScrollAreaOutput as ScrollOutput, ScrollSource};
use std::sync::Arc;

use egui::style::ScrollAnimation;
use egui::{Color32, CornerRadius, Pos2, Rect, ScrollArea, Ui, Vec2, pos2, vec2};
use mareader_library::{BookId, MarkId};
use mareader_pdf::{Bounds, LinkTarget, web_address};
use mareader_search::reveal;
use mareader_session::{Band, DocId, Document, PageKey, PageSize, Part, Scale, Tiling, Want};
use mareader_settings::{ColumnAlign, HighlightSettings, LayoutSettings, StartupFit, TextSettings};

use crate::gpu::{Anchor, PagePass, PagesCallback, Quad};
use crate::layout::{self, Arrow, GAP, Inputs, Layout, MARGIN, ViewMode};
use crate::page_ai;
use crate::pages::{Lookup, PageCache, PageTexture};
use crate::reach::{self, Press, Stop};
use crate::read_aloud::marker::{self, Marker};
use crate::selection::{self, Pages, Selection, Sheet};
use crate::study::{self, BarPress, Gloss};
use crate::texts::TextLayers;
use crate::theme::Tokens;
use crate::vocab;
use crate::zoom::{Fit, PaneZoom, ZoomCommand};
use crate::{keymap, motion};
use mareader_appearance::{PageTone, Rgb};

/// Pages drawn ahead of the viewport, each way, in turns.
const PREFETCH: usize = 1;
/// A held arrow waits this long, then glides at this speed (mareader's).
const HOLD_DELAY: f64 = 0.35;
const HOLD_SPEED: f32 = 1000.0;
/// A slow frame moves a held glide no further than this one would.
const MAX_FRAME: f64 = 0.05;
/// Auto-scroll's pace along the strip, as mareader's 72 px a second.
pub const AUTO_SPEED: f32 = 72.0;
/// Along a strip, a page of another size refits once read this long.
const RESIZE_SETTLE: f64 = 0.25;
/// Tiles wait for the view to rest this long, so a pan queues none.
const TILE_REST: f64 = 0.25;
/// A tiled page's whole raster, stretched under its tiles.
const BACKDROP_SIDE: f32 = 2048.0;

/// What a page is drawn from: one raster, or a backdrop under tiles.
#[derive(Debug, Clone, Copy)]
enum Plan {
    Whole(PageKey),
    Tiled {
        backdrop: PageKey,
        tiling: Tiling,
        scale: Scale,
    },
}

impl Plan {
    /// The whole-page raster: the page itself, or its backdrop.
    fn base(self) -> PageKey {
        match self {
            Self::Whole(key) | Self::Tiled { backdrop: key, .. } => key,
        }
    }
}

/// A tap moves one reading line: 8 % of the view, 40 to 80 points.
pub fn line_step(len: f32) -> f32 {
    (len * 0.08).clamp(40.0, 80.0)
}

/// A page key moves nearly a screen, keeping a sliver to hold the place.
pub fn screen_step(len: f32) -> f32 {
    (len * 0.9).max(1.0)
}

/// A key's move along a strip, applied by the next frame.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Nudge {
    Line(f32),
    Screen(f32),
}

impl Nudge {
    fn points(self, len: f32) -> f32 {
        match self {
            Self::Line(dir) => dir * line_step(len),
            Self::Screen(dir) => dir * screen_step(len),
        }
    }
}

/// An arrow held along a strip, since `since`; `last` is the last frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Hold {
    arrow: Arrow,
    since: f64,
    last: f64,
}

/// A page jump gliding to `to` (a [`Glide`]); only its pages render meanwhile.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Jump {
    to: Vec2,
    started: bool,
    /// Given up by then: a wheel or a drag took the scroll over.
    until: f64,
}

/// How soon a page is needed; lower is sooner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    ActivePane,
    OtherPane,
    /// Text under the pointer matters more than a thumbnail.
    Text,
    /// Small and quick, and the backdrop waits on it.
    Paper,
    Thumbnail,
    Prefetch,
}

/// The frame's requests from every pane.
pub type Demand = Vec<(Urgency, Want)>;

/// One pane's view of a document; the document itself is shared.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfView {
    pub doc: DocId,
    /// The library book this pane reads as; none: the file's own.
    pub book: Option<BookId>,
    pub zoom: PaneZoom,
    pub mode: ViewMode,
    /// The paged modes' page; the strips follow the scroll instead.
    focus: usize,
    layout: Option<Layout>,
    /// A page to bring to the start of the view next frame.
    reveal: Option<usize>,
    /// An offset to reach, and the frames left to reach it.
    scroll_to: Option<(Vec2, u8)>,
    offset: Vec2,
    viewport: Vec2,
    /// Pages on screen last frame.
    pub visible: std::ops::Range<usize>,
    /// The page read: the paged modes' page, or under a strip's centre.
    pub current: usize,
    /// How far through the document, 0 to 1.
    pub progress: f32,
    pub selection: Option<Selection>,
    /// Where reading aloud will start, until it does.
    pub marker: Option<Marker>,
    /// Pages on screen last frame, where they sat.
    pub sheets: Vec<Sheet>,
    /// The page looked at last frame, fractional, for the backdrop.
    paper_at: Option<f64>,
    nudge: Option<Nudge>,
    /// Where the last key's glide is heading along the strip, and until when.
    aim: Option<Glide>,
    hold: Option<Hold>,
    /// Auto-scroll is on: when it last moved the strip.
    drift: Option<f64>,
    jump: Option<Jump>,
    /// A strip's page jumped to, and where it landed: read until moved.
    landed: Option<(usize, Option<Vec2>)>,
    /// Zoom steps ease, as of last frame's motion settings.
    ease: bool,
    /// Until first drawn: the default fit is still to apply.
    fresh: bool,
    /// A page of another size the zoom waits to follow, and since when.
    resize: Option<(usize, f64)>,
    /// A strip's spot to keep once the pages are laid out anew.
    keep: Option<(usize, Vec2)>,
    /// The document's pages turn right to left.
    rtl: bool,
    /// The pages it wants alone before spreads pair them.
    lead: usize,
    /// When the view last moved, for tiles to wait on.
    moved_at: f64,
    /// A book's footnote open in a card on its mark.
    pub footnote: Option<crate::footnote::Footnote>,
    /// Clip transcripts open in the book's pages.
    pub transcripts: crate::transcript::Transcripts,
    /// What the book's triggers have shown, hidden and played.
    pub triggers: crate::trigger::Triggers,
    /// A book's picture opened large over the window.
    pub viewer: Option<crate::image_viewer::Viewer>,
    /// Where the last re-set landed, and on which page; reuse never drifts.
    place: Option<(mareader_session::Anchor, usize)>,
    /// A strip's offset once landed on `place`; moved from, it is stale.
    placed_at: Option<Vec2>,
    /// Set anew: a page, a height on it and the air above, to show first.
    land: Option<(usize, f32, f32)>,
    /// A paged book's pane-sized page, in layout units, once settled.
    sheet: Option<(f32, f32)>,
    /// The page size the pane wants next, and since when.
    sheet_want: Option<(Option<(f32, f32)>, f64)>,
    /// The sheet changed this frame: the book is to be set anew.
    pub sheet_moved: bool,
}

impl PdfView {
    pub fn new(doc: DocId) -> Self {
        Self {
            doc,
            book: None,
            zoom: PaneZoom::default(),
            mode: ViewMode::default(),
            focus: 0,
            layout: None,
            reveal: None,
            scroll_to: None,
            offset: Vec2::ZERO,
            viewport: Vec2::ZERO,
            visible: 0..0,
            current: 0,
            progress: 0.0,
            selection: None,
            marker: None,
            sheets: Vec::new(),
            paper_at: None,
            fresh: true,
            resize: None,
            keep: None,
            rtl: false,
            lead: 0,
            moved_at: 0.0,
            footnote: None,
            transcripts: Default::default(),
            triggers: Default::default(),
            viewer: None,
            nudge: None,
            aim: None,
            hold: None,
            drift: None,
            jump: None,
            landed: None,
            ease: true,
            place: None,
            placed_at: None,
            land: None,
            sheet: None,
            sheet_want: None,
            sheet_moved: false,
        }
    }

    /// The pane-sized page its book is set on; none is the A4 card.
    pub fn sheet(&self) -> Option<(f32, f32)> {
        self.sheet
    }

    /// Takes `want` once it holds still; else how long to wait.
    fn settle_sheet(&mut self, want: Option<(f32, f32)>, now: f64) -> Option<f64> {
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
    pub fn place(&self) -> Option<mareader_session::Anchor> {
        // A strip holds it while unmoved since; scrolled, it reads elsewhere.
        if self.mode == ViewMode::Vertical {
            let still = |at: Vec2| (at - self.offset).length() < 2.0;
            return self
                .place
                .filter(|_| self.placed_at.is_some_and(still))
                .map(|(a, _)| a);
        }
        self.place
            .filter(|(_, page)| *page == self.current)
            .map(|(a, _)| a)
    }

    /// A strip's reading line: the page at the view's top, and how far down.
    pub fn reading_line(&self) -> Option<(usize, f32)> {
        let layout = self
            .layout
            .as_ref()
            .filter(|_| self.mode == ViewMode::Vertical)?;
        let top = vec2(self.viewport.x / 2.0, layout.inputs.inset);
        let (page, at) = layout.locate(Pos2::ZERO + self.offset + top)?;
        Some((page, at.y.max(0.0)))
    }

    /// Shows height `y` of `page` at a strip's top, `air` below its edge.
    pub fn land(&mut self, page: usize, y: f32, air: f32) {
        self.land = (self.mode == ViewMode::Vertical).then_some((page, y, air));
    }

    /// Keeps `place` as the page being read's, as a re-set does.
    pub fn hold_place(&mut self, place: Option<mareader_session::Anchor>) {
        self.place = place.map(|a| (a, self.current));
    }

    /// Shows the book set again as `doc`, from `page`; mode and zoom stay.
    pub fn retypeset(
        &mut self,
        doc: DocId,
        page: usize,
        count: usize,
        place: Option<mareader_session::Anchor>,
    ) {
        let page = page.min(count.saturating_sub(1));
        self.doc = doc;
        // Page rects, selection and motion belong to the old pages.
        self.layout = None;
        self.selection = None;
        self.sheets.clear();
        self.visible = page..page + 1;
        (self.jump, self.aim, self.hold, self.nudge) = (None, None, None, None);
        self.scroll_to = None;
        self.resize = None;
        self.zoom.fit_page = page;
        self.place = place.map(|a| (a, page));
        (self.placed_at, self.land) = (None, None);
        self.go(page);
    }

    /// Moves to `doc`, results laid in or out; false if the page went.
    pub fn respliced(
        &mut self,
        doc: DocId,
        count: usize,
        map: impl Fn(usize) -> Option<usize>,
    ) -> bool {
        let Some(page) = map(self.current) else {
            return false;
        };
        let centre = Pos2::ZERO + self.offset + self.viewport / 2.0;
        let spot = self.layout.as_ref().and_then(|l| l.locate(centre));
        let keep = spot.and_then(|(p, at)| Some((map(p)?, at)));
        let place = self.place.and_then(|(a, p)| Some((a, map(p)?)));
        self.retypeset(doc, page, count, None);
        self.place = place;
        self.keep = keep.filter(|_| !self.mode.paged());
        true
    }

    pub fn command(&mut self, command: ZoomCommand, now: f64) {
        let shown = self.layout.as_ref().map(Layout::zoom);
        let shown = shown.filter(|_| self.ease);
        self.zoom.command(command, shown, self.current, now);
    }

    /// Switches mode on the page being read; `fit` fits the paged modes.
    pub fn set_mode(&mut self, mode: ViewMode, now: f64) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        self.hold = None;
        // Single pages and spreads fit their width; the strip never.
        if mode.paged() {
            self.zoom
                .command(ZoomCommand::Fit(Fit::Width), None, self.current, now);
        } else if mode == ViewMode::Horizontal {
            self.zoom.unfit();
            self.zoom.set_basis(Fit::Page);
        } else if self.zoom.fit.is_none() {
            self.zoom.set_basis(Fit::Width);
        }
        self.go(self.current);
    }

    /// Shift+A: auto-scroll on or off; false in a paged mode.
    pub fn toggle_drift(&mut self, now: f64) -> bool {
        if self.mode.paged() {
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
    pub fn arrow(&mut self, arrow: Arrow, count: usize, now: f64) {
        if let Some(forward) = arrow.turns(self.mode, self.rtl) {
            self.go(self.mode.turn(self.current, forward, (count, self.lead)));
            return;
        }
        // Up and down across the strip do nothing, as in mareader.
        let sideways = matches!(arrow, Arrow::Left | Arrow::Right);
        if sideways != (self.mode == ViewMode::Horizontal) {
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
    pub fn page_key(&mut self, forward: bool, count: usize) {
        if self.mode.paged() {
            self.turn(forward, count);
        } else {
            self.nudge = Some(Nudge::Screen(if forward { 1.0 } else { -1.0 }));
        }
    }

    /// Where the backdrop reads its paper: pages weighted by their share.
    pub fn paper_at(&self) -> f64 {
        self.paper_at.unwrap_or(self.current as f64)
    }

    /// Last frame's scroll, in content points; narrower pages sit centred.
    pub fn offset(&self) -> Vec2 {
        self.offset
    }

    /// A page turn from a button: it turns in every mode.
    pub fn turn(&mut self, forward: bool, count: usize) {
        self.go(self.mode.turn(self.current, forward, (count, self.lead)));
    }

    /// Jumps to `page`, kept within the document's `count`.
    pub fn go_to(&mut self, page: usize, count: usize) {
        self.go(page.min(count.saturating_sub(1)));
    }

    /// True while a page turn or reveal still glides.
    pub fn gliding(&self) -> bool {
        self.jump.is_some() || self.reveal.is_some() || self.aim.is_some()
    }

    /// Shows `spot` on `page`, `clear` above the foot; false if unknown.
    pub fn seek(
        &mut self,
        page: usize,
        spot: Option<Bounds>,
        band: Vec2,
        count: usize,
        clear: f32,
    ) -> bool {
        // Already heading there: going again would restart the glide.
        let heading = self.jump.is_some() || self.reveal.is_some();
        let shows = self.visible.contains(&page);
        if self.mode.paged() || spot.is_none() || self.layout.is_none() {
            if !shows && !heading {
                self.go_to(page, count);
            }
            return self.mode.paged();
        }
        let (Some(layout), Some(b)) = (&self.layout, spot) else {
            return false;
        };
        let Some(rect) = layout.rect(page) else {
            return false;
        };
        let zoom = layout.zoom();
        // Only the vertical strip cuts pages to their bands.
        let band = if self.mode == ViewMode::Vertical {
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
    fn follow_size(&mut self, sizes: &[PageSize], keep: bool, now: f64) -> Option<f64> {
        let page = self.current;
        let fit_box = |p| layout::fit_box(self.mode, sizes, (p, self.lead));
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
        if self.mode.paged() {
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

    /// Shows `page` from its start.
    fn go(&mut self, page: usize) {
        self.focus = self.mode.start(page, self.lead);
        self.current = page;
        self.reveal = Some(if self.mode.paged() { self.focus } else { page });
    }
}

/// Set anew, a strip starts at the place read, in the pages' first frame.
fn land(view: &mut PdfView) {
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
fn hold(old: &Layout, new: &Layout, offset: Vec2, pivots: (Vec2, Vec2)) -> Option<Vec2> {
    let (page, at) = old.locate(Pos2::ZERO + offset + pivots.0)?;
    let p = new.place(page, at)?;
    Some((p - pivots.1).to_vec2().max(Vec2::ZERO))
}

/// What the pane needs from the app this frame.
pub struct ViewContext {
    pub active: bool,
    /// The pane shares the window, so a hold can lift it.
    pub lift: bool,
    /// The pane's look: the window's, or its own while split.
    pub tokens: Tokens,
    /// Space a pinned title bar takes from the pane's top.
    pub top_inset: f32,
    pub page: PageLook,
    /// Blend's paper behind the pages, raw; none draws no backdrop.
    pub paper: Option<Rgb>,
    /// The pane's corner radius, which the backdrop keeps.
    pub corner: f32,
    /// Read this pane's paper: blend is on and the pane is focused.
    pub sample: bool,
    /// The book's marks, and how marks and the selection bar look.
    pub marks: Arc<[study::marks::Shown]>,
    /// Pages with a bookmark, which hang a ribbon.
    pub bookmarks: Vec<usize>,
    pub highlights: HighlightSettings,
    /// The mark whose card is open here, to keep the card beside it.
    pub card: Option<MarkId>,
    /// The marks picked here, while picking several.
    pub picked: Option<Vec<MarkId>>,
    /// How hard words are shaded; none while shading is off.
    pub shading: Option<crate::vocab::Look>,
    /// The find bar's hits, in the pane it searches.
    pub found: Option<crate::search::Lit>,
    /// The phrase read aloud, in the pane reading.
    pub aloud: Option<crate::read_aloud::Lit>,
    /// Shots: opens the newest drawn mark's menu.
    pub menu_shot: bool,
    /// The page AI's bubbles and results on the pages shown.
    pub ai: page_ai::PaneAi,
}

/// The Layout settings the pages follow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageLook {
    pub gap: f32,
    /// Air beside the pages; the sideways strip has none to give.
    pub margin: f32,
    pub shadow: bool,
    /// A fit follows the page read when its size differs.
    pub auto_resize: bool,
    /// The fit a document opens at.
    pub fit: Fit,
    /// Where a book's vertical stream sits across the pane.
    pub column: egui::Align,
    /// How much of a book's ink stays, 0 to 1: the ink dial.
    pub ink: f32,
    /// A book's pictures take the page's theme, not their own colours.
    pub theme_pictures: bool,
    /// So do its emoji, drawn from colour fonts.
    pub theme_emojis: bool,
}

impl PageLook {
    pub fn new(layout: &LayoutSettings, text: &TextSettings) -> Self {
        Self {
            gap: if layout.no_gap { 0.0 } else { GAP },
            margin: layout.page_margin,
            shadow: layout.page_shadow,
            auto_resize: layout.auto_resize,
            fit: match layout.startup_fit {
                StartupFit::Page => Fit::Page,
                StartupFit::Width => Fit::Width,
            },
            column: match text.column_align {
                ColumnAlign::Left => egui::Align::Min,
                ColumnAlign::Center => egui::Align::Center,
                ColumnAlign::Right => egui::Align::Max,
            },
            ink: (text.ink_contrast / 100.0).clamp(0.0, 1.0),
            theme_pictures: text.theme_pictures,
            theme_emojis: text.theme_emojis,
        }
    }
}

impl Default for PageLook {
    fn default() -> Self {
        Self::new(&LayoutSettings::default(), &TextSettings::default())
    }
}

/// What drawing a pane reads and fills, shared across panes.
pub struct Sinks<'a> {
    pub pages: &'a mut PageCache,
    pub demand: &'a mut Demand,
    pub texts: &'a mut TextLayers,
    /// GPU pages drawn this frame, for the glass to blur.
    pub drawn: &'a mut Vec<Quad>,
    pub pass: Option<&'a std::sync::Arc<PagePass>>,
    /// What the pages tell the study tools.
    pub gloss: &'a mut Vec<Gloss>,
    /// Which words are hard, for the pages that shade them.
    pub vocab: Option<&'a mut crate::vocab::Vocab>,
}

/// How long a resized pane holds still before its book is set anew.
const SHEET_SETTLE: f64 = 0.3;

/// A paged book's page: what the pane leaves each page, at its zoom.
fn sheet_for(view: &PdfView, doc: &Document, room: Vec2) -> Option<(f32, f32)> {
    if !doc.is_reflowable() || !view.mode.paged() {
        return None;
    }
    let count = if view.mode == ViewMode::Spread {
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

mod input;
mod overlay;
mod paint;
mod scroll;

/// Frame context passed through the four show stages.
pub(super) struct ShowContext<'a> {
    ui: Option<&'a mut Ui>,
    view: &'a mut PdfView,
    doc: &'a Arc<Document>,
    view_cx: &'a ViewContext,
    pages: &'a mut PageCache,
    demand: &'a mut Demand,
    texts: &'a mut TextLayers,
    drawn: &'a mut Vec<Quad>,
    pass: Option<&'a Arc<PagePass>>,
    gloss: &'a mut Vec<Gloss>,
    vocab: Option<&'a mut vocab::Vocab>,
    ctx: egui::Context,
    now: f64,
    frame: Rect,
    sizes: &'a [PageSize],
    bands: Option<&'a [Band]>,
    gliding: bool,
    shown: Option<f32>,
    margin: f32,
    room: Vec2,
    rebuilt: bool,
    viewport: Vec2,
    input: Option<InputState>,
    scroll: Option<ScrollState>,
    paint: PaintState,
}

pub(super) struct InputState {
    viewport: Vec2,
    keyed: Option<f32>,
    sideways: bool,
    down: bool,
    ppp: f32,
    scale: Scale,
    max_side: f32,
    resting: bool,
    urgency: Urgency,
    t: Tokens,
    reflow: bool,
    picture_tone: PageTone,
    emoji_tone: PageTone,
}

pub(super) struct ScrollState {
    area: Option<ScrollArea>,
    landing: Option<std::ops::Range<usize>>,
    jump_started: bool,
}

#[derive(Default)]
pub(super) struct PaintState {
    out: Option<ScrollOutput<()>>,
    shown_pages: std::ops::Range<usize>,
    sheets: Vec<Sheet>,
    paper_at: Option<f64>,
    follow: Option<LinkTarget>,
    quads: Vec<Quad>,
    covered: Vec<usize>,
    open: Vec<Sheet>,
    placed: Vec<study::marks::Placed>,
    shaded: vocab::Shading,
    on_word: Option<usize>,
    did: selection::Outcome,
}

pub fn show(ui: &mut Ui, view: &mut PdfView, doc: &Arc<Document>, io: Sinks<'_>, cx: &ViewContext) {
    let Sinks {
        pages,
        demand,
        texts,
        drawn,
        pass,
        gloss,
        vocab,
    } = io;
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let frame = ui.max_rect();
    let bands = doc.bands().filter(|_| view.mode == ViewMode::Vertical);
    let mut context = ShowContext {
        ui: Some(ui),
        view,
        doc,
        view_cx: cx,
        pages,
        demand,
        texts,
        drawn,
        pass,
        gloss,
        vocab,
        ctx,
        now,
        frame,
        sizes: doc.page_sizes(),
        bands,
        gliding: false,
        shown: None,
        margin: 0.0,
        room: Vec2::ZERO,
        rebuilt: false,
        viewport: Vec2::ZERO,
        input: None,
        scroll: None,
        paint: PaintState::default(),
    };
    input::run(&mut context);
    if context.view.layout.is_none() {
        return;
    }
    scroll::run(&mut context);
    paint::run(&mut context);
    overlay::run(&mut context);
}

/// A book's picture, or a PDF's figure, under a double click at `p`.
fn open_picture(
    ctx: &egui::Context,
    (sheets, texts, id): (&[Sheet], &TextLayers, DocId),
    doc: &Arc<Document>,
    p: Pos2,
) -> Option<crate::image_viewer::Viewer> {
    use crate::image_viewer::Viewer;
    let sheet = sheets.iter().find(|s| s.shown.contains(p))?;
    let (x, y) = sheet.to_page(p);
    if let Some(picture) = doc.picture_at(sheet.page, (x, y)) {
        return Some(Viewer::picture(ctx, picture));
    }
    let layer = texts.get(id, sheet.page)?;
    let inside = |b: &&Bounds| (b.x0..=b.x1).contains(&x) && (b.y0..=b.y1).contains(&y);
    // The smallest is the figure, not the scan's page behind it.
    let area = |b: &&Bounds| (b.x1 - b.x0) * (b.y1 - b.y0);
    let at = *layer
        .figures
        .iter()
        .filter(inside)
        .min_by(|a, b| area(a).total_cmp(&area(b)))?;
    Some(Viewer::figure(ctx, Arc::clone(doc), sheet.page, at))
}

/// The open footnote's card; a press outside or Escape closes it.
fn footnote(ctx: &egui::Context, view: &mut PdfView, room: Rect, count: usize, t: &Tokens) {
    use crate::footnote::{self, Press};
    let Some(note) = &view.footnote else {
        return;
    };
    // Its page scrolled away, the card has nothing to point at.
    let Some(at) = footnote::anchor(note, &view.sheets) else {
        view.footnote = None;
        return;
    };
    let id = egui::Id::new(("footnote", view.doc, note.page, note.text.as_str()));
    let (card, press) = footnote::show(ctx, id, (note, at, room), t);
    // The press that opened it lands on its mark, maybe this very frame.
    let outside = ctx.input(|i| {
        let origin = i.pointer.press_origin();
        let away = |p: Pos2| !card.contains(p) && !at.expand(4.0).contains(p);
        i.pointer.any_pressed() && origin.is_some_and(away)
    });
    let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    match press {
        Some(Press::Go(page)) => {
            view.footnote = None;
            view.go_to(page, count);
        }
        Some(Press::Close) => view.footnote = None,
        None if outside || escape => view.footnote = None,
        None => {}
    }
}

/// The page and box of the shown link to `target` nearest `near`.
fn mark_of(
    sheets: &[Sheet],
    texts: &TextLayers,
    doc: DocId,
    target: &LinkTarget,
    near: Option<Pos2>,
) -> Option<(usize, Bounds)> {
    let mut best: Option<(f32, usize, Bounds)> = None;
    for sheet in sheets {
        let Some(layer) = texts.get(doc, sheet.page) else {
            continue;
        };
        for link in layer.links.iter().filter(|l| l.target == *target) {
            let rect = sheet.to_screen(link.bounds);
            let d = near.map_or(0.0, |p| rect.distance_to_pos(p));
            if best.is_none_or(|(b, ..)| d < b) {
                best = Some((d, sheet.page, link.bounds));
            }
        }
    }
    best.map(|(_, page, at)| (page, at))
}

/// The AI's bubbles, over everything else on their pages.
fn ai_bubbles(
    ui: &Ui,
    sheets: &[Sheet],
    ai: &[page_ai::Overlay],
    t: &Tokens,
    gloss: &mut Vec<Gloss>,
) {
    for o in ai.iter().filter(|o| !o.bubbles.is_empty()) {
        let Some(s) = sheets.iter().find(|s| s.page == o.page) else {
            continue;
        };
        let page = o.page;
        match page_ai::paint::bubbles(ui, (s.rect, s.shown), page, &o.bubbles, t) {
            Some(page_ai::paint::Press::Pick(index, at)) => {
                gloss.push(Gloss::Bubble { page, index, at });
            }
            Some(page_ai::paint::Press::Menu(index, at)) => {
                gloss.push(Gloss::BubbleMenu { page, index, at });
            }
            None => {}
        }
    }
}

/// A right-click on selected text belongs to the selection.
fn on_selection(pages: &Pages<'_>, selection: Option<Selection>, at: Pos2) -> bool {
    let Some(sel) = selection.filter(|s| !s.is_empty()) else {
        return false;
    };
    let Some(s) = pages.sheets.iter().find(|s| s.shown.contains(at)) else {
        return false;
    };
    let Some(layer) = pages.texts.get(pages.doc, s.page) else {
        return false;
    };
    let (x, y) = s.to_page(at);
    let hit = layer.text.char_at(x, y);
    let range = sel.on_page(s.page, layer.text.len());
    hit.zip(range).is_some_and(|(i, r)| r.contains(&i))
}

/// A key at a focused stop does what a click there would.
fn at_stop(
    press: Option<Press>,
    placed: &[study::marks::Placed],
    shaded: &[vocab::Shaded],
    picking: bool,
    gloss: &mut Vec<Gloss>,
) -> Option<LinkTarget> {
    let mark = |i: usize| placed.get(i);
    match press? {
        Press::Go(Stop::Link(target)) => return Some(target),
        Press::Go(Stop::Mark(i)) => {
            let m = mark(i)?;
            gloss.push(match picking {
                true => Gloss::Toggle(m.id),
                false => Gloss::Open {
                    mark: m.id,
                    at: m.bounds(),
                },
            });
        }
        Press::Go(Stop::Word(i)) => {
            let w = shaded.get(i).filter(|_| !picking)?;
            gloss.push(Gloss::Shaded {
                page: w.page,
                chars: w.chars.clone(),
                at: w.bounds(),
            });
        }
        Press::Menu(i) => {
            let m = mark(i).filter(|_| !picking)?;
            let at = m.bounds().left_bottom();
            let words = m.bounds();
            gloss.push(Gloss::Menu {
                mark: m.id,
                at,
                words,
            });
        }
    }
    None
}

/// A key's glide along the strip. The view runs it, so quick presses add up.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Glide {
    from: f32,
    to: f32,
    start: f64,
    secs: f64,
    /// Where it put the strip last frame.
    at: f32,
}

impl Glide {
    fn new(from: f32, to: f32, now: f64, a: ScrollAnimation) -> Self {
        let secs = ((to - from).abs() / a.points_per_second).clamp(a.duration.min, a.duration.max);
        Self {
            from,
            to,
            start: now,
            secs: f64::from(secs),
            at: from,
        }
    }

    /// Moves the whole glide `d` along, as auto-scroll moves the strip.
    fn shift(&mut self, d: f32) {
        self.from += d;
        self.to += d;
        self.at += d;
    }

    /// This frame's position, and whether the glide is over.
    fn step(&mut self, now: f64) -> (f32, bool) {
        let t = ((now - self.start) / self.secs.max(1e-6)).clamp(0.0, 1.0) as f32;
        self.at = self.from + (self.to - self.from) * egui::emath::ease_in_ease_out(t);
        (self.at, t >= 1.0)
    }
}

/// This frame's move along the strip: the keys' and auto-scroll's together.
fn keyed_scroll(view: &mut PdfView, ctx: &egui::Context, len: f32, now: f64) -> Option<f32> {
    let keyed = steered(view, ctx, len, now);
    let drift = drifted(view, ctx, now);
    // A key's glide rides along, or it would read the drift as a wheel.
    if let (Some(d), Some(g)) = (drift, &mut view.aim) {
        g.shift(d);
    }
    match (keyed, drift) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
    }
}

/// Auto-scroll's step, at the same pace whatever the frame rate.
fn drifted(view: &mut PdfView, ctx: &egui::Context, now: f64) -> Option<f32> {
    let last = view.drift?;
    let size = view.layout.as_ref()?.size;
    let (here, end) = match view.mode {
        ViewMode::Horizontal => (view.offset.x, size.x - view.viewport.x),
        _ => (view.offset.y, size.y - view.viewport.y),
    };
    // Paged modes have nowhere to drift; at the strip's end it is done.
    if view.mode.paged() || here >= end - 0.5 {
        view.drift = None;
        return None;
    }
    view.drift = Some(now);
    ctx.request_repaint();
    // A held press, as a selection's drag, holds it still; so does a jump.
    if view.jump.is_some() || ctx.input(|i| i.pointer.any_down()) {
        return None;
    }
    let dt = (now - last).clamp(0.0, MAX_FRAME) as f32;
    Some((AUTO_SPEED * dt).min(end - here))
}

/// The keys' scroll along the strip, in points.
fn steered(view: &mut PdfView, ctx: &egui::Context, len: f32, now: f64) -> Option<f32> {
    if view.mode.paged() {
        view.hold = None;
        view.nudge = None;
        view.aim = None;
        return None;
    }
    let here = if view.mode == ViewMode::Horizontal {
        view.offset.x
    } else {
        view.offset.y
    };
    // A wheel or a drag moved the strip meanwhile: the reader wins.
    if view.aim.is_some_and(|g| (g.at - here).abs() > 1.0) {
        view.aim = None;
    }
    let mut keyed = None;
    if let Some(n) = view.nudge.take() {
        // A key pressed mid-glide adds to where that glide was going.
        let to = view.aim.map_or(here, |g| g.to) + n.points(len);
        let a = motion::jump(ctx);
        view.aim = (a.duration.max > 0.0).then(|| Glide::new(here, to, now, a));
        keyed = Some(to - here);
    }
    if let Some(g) = &mut view.aim {
        let (at, done) = g.step(now);
        keyed = Some(at - here);
        if done {
            view.aim = None;
        } else {
            ctx.request_repaint();
        }
    }
    let Some(mut hold) = view.hold else {
        return keyed;
    };
    if !ctx.input(|i| keymap::held(i, hold.arrow)) {
        view.hold = None;
        return keyed;
    }
    if now - hold.since >= HOLD_DELAY {
        view.aim = None;
        let dt = (now - hold.last).clamp(0.0, MAX_FRAME) as f32;
        keyed = Some(hold.arrow.sign() * HOLD_SPEED * dt);
    }
    hold.last = now;
    view.hold = Some(hold);
    ctx.request_repaint();
    keyed
}

/// ⌘C or Ctrl+C copies the selection, unless a text field has focus.
fn copy(ctx: &egui::Context, view: &PdfView, doc: &Document, texts: &TextLayers) {
    let asked = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
    if !asked || ctx.egui_wants_keyboard_input() {
        return;
    }
    if let Some(text) = selected_text(view, doc, texts) {
        ctx.copy_text(text);
    }
}

/// The selection's text; none if nothing is selected.
fn selected_text(view: &PdfView, doc: &Document, texts: &TextLayers) -> Option<String> {
    let sel = view.selection.filter(|s| !s.is_empty())?;
    // Pages scrolled out of the cache are read again, here.
    Some(sel.text(|page| {
        texts
            .get(view.doc, page)
            .or_else(|| doc.text_layer(page).ok().map(Arc::new))
    }))
}

/// The bar under a finished selection, while any of it is on screen.
fn selection_bar(
    ctx: &egui::Context,
    view: &PdfView,
    texts: &TextLayers,
    room: Rect,
    look: &HighlightSettings,
) -> Option<BarPress> {
    let sel = view.selection.filter(|s| !s.is_empty())?;
    let id = egui::Id::new(("study-bar", view.doc));
    // Hidden while a press elsewhere may be selecting; a press on it stays.
    let bar = egui::LayerId::new(egui::Order::Foreground, id);
    let (down, from) = ctx.input(|i| (i.pointer.any_down(), i.pointer.press_origin()));
    if down && from.and_then(|p| ctx.layer_id_at(p)) != Some(bar) {
        return None;
    }
    let mut shown = Rect::NOTHING;
    for sheet in &view.sheets {
        let Some(layer) = texts.get(view.doc, sheet.page) else {
            continue;
        };
        let Some(range) = sel.on_page(sheet.page, layer.text.len()) else {
            continue;
        };
        for b in layer.text.line_boxes(range) {
            let r = sheet.to_screen(b).intersect(sheet.shown).intersect(room);
            if r.is_positive() {
                shown = shown.union(r);
            }
        }
    }
    if !shown.is_positive() {
        return None;
    }
    study::show_bar(ctx, id, shown, room, look)
}

/// The strips' scroll along their axis; the paged modes' page.
fn progress(view: &PdfView, layout: &Layout, out: &ScrollOutput<()>, count: usize) -> f32 {
    let room = (out.content_size - out.inner_rect.size()).max(Vec2::ZERO);
    let (done, total) = match view.mode {
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

/// A tiled page's grid, drawn over its backdrop where tiles have arrived.
struct Tiles {
    /// What of the page shows: a stream cuts it to its band.
    clip: Rect,
    page: Rect,
    anchor: Anchor,
    tiling: Tiling,
    /// The tiles' scale; its part is set per tile.
    key: PageKey,
}

impl Tiles {
    /// Draws the tiles on screen, asking for missing ones if `ask`.
    fn paint(
        &self,
        ui: &Ui,
        pages: &mut PageCache,
        mut ask: Option<(&mut Demand, Urgency)>,
        (quads, tone): (&mut Vec<Quad>, PageTone),
    ) {
        let t = &self.tiling;
        let size = vec2(t.width as f32, t.height as f32);
        let to_screen = self.page.size() / size;
        let clip = self.clip;
        for row in 0..t.rows() {
            for col in 0..t.cols() {
                let Some([x, y, w, h]) = t.rect(col, row) else {
                    continue;
                };
                let at = vec2(x as f32, y as f32) * to_screen;
                let rect =
                    Rect::from_min_size(self.page.min + at, vec2(w as f32, h as f32) * to_screen);
                if !rect.intersects(clip) {
                    continue;
                }
                let (Ok(col), Ok(row)) = (u8::try_from(col), u8::try_from(row)) else {
                    continue;
                };
                let key = PageKey {
                    part: Part::Tile { col, row },
                    ..self.key
                };
                match pages.lookup_tile(key) {
                    Some(PageTexture::Gpu(page)) => {
                        let quad = Quad::page((rect, clip), Some(page), tone, self.anchor);
                        quads.push(quad);
                    }
                    Some(PageTexture::Egui(h)) => {
                        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                        let painter = ui.painter().with_clip_rect(clip);
                        painter.image(h.id(), rect, uv, Color32::WHITE);
                    }
                    None => {
                        if let Some((demand, urgency)) = ask.as_mut()
                            && !pages.touch(key)
                        {
                            demand.push((*urgency, key.into()));
                        }
                    }
                }
            }
        }
    }
}

/// Glyphs may overhang the column a little: a hung hyphen, an italic.
const STREAM_SLACK: f32 = 24.0;
/// What a revealed hit keeps clear at the top: the find bar and its air.
pub(crate) const FIND_CLEARANCE: f32 = 72.0;
/// Air a revealed hit keeps from the other edges.
const SEEK_MARGIN: f32 = 24.0;

/// The whole page placed so `band` fills `cut`, and a clip to the band.
fn streamed(cut: Rect, band: &Band, size: Option<&PageSize>, zoom: f32, ppp: f32) -> (Rect, Rect) {
    let size = size.map_or(vec2(band.width, band.height), |s| vec2(s.width, s.height));
    let min = cut.min - vec2(band.x, band.y) * zoom;
    let page = Rect::from_min_size(min, size * zoom).round_to_pixels(ppp);
    (page, cut.expand2(vec2(STREAM_SLACK * zoom, 0.0)))
}

/// What a page shows before it is drawn: a blank page, themed.
pub(crate) fn blank(t: &Tokens) -> Color32 {
    let [r, g, b] = t.page.blank().map(|c| (c * 255.0).round() as u8);
    Color32::from_rgb(r, g, b)
}

/// Painters for the parts of `clip` clear of `frame`'s round corners.
fn clear_of_corners(ui: &Ui, clip: Rect, (frame, radius): (Rect, f32)) -> Vec<egui::Painter> {
    if radius <= 0.0 {
        return vec![ui.painter().with_clip_rect(clip)];
    }
    let (f, r) = (frame, radius);
    let (top, bottom) = (f.top() + r, f.bottom() - r);
    let (left, right) = (f.left() + r, f.right() - r);
    [
        Rect::from_min_max(pos2(left, f.top()), pos2(right, top)),
        Rect::from_min_max(pos2(f.left(), top), pos2(f.right(), bottom)),
        Rect::from_min_max(pos2(left, bottom), pos2(right, f.bottom())),
    ]
    .into_iter()
    .map(|band| band.intersect(clip))
    .filter(|band| band.is_positive())
    .map(|band| ui.painter().with_clip_rect(band))
    .collect()
}

/// Draws `quads` from `first` again over `pictures` in `tone`, undimmed.
pub(crate) fn keep_pictures(
    quads: &mut Vec<Quad>,
    first: usize,
    pictures: impl Iterator<Item = Rect>,
    tone: PageTone,
) {
    let page: Vec<Quad> = quads.get(first..).unwrap_or_default().to_vec();
    for picture in pictures {
        for q in page.iter().filter(|q| q.texture.is_some()) {
            let clip = q.clip.intersect(picture);
            if clip.is_positive() {
                quads.push(Quad {
                    clip,
                    tone,
                    ..q.clone()
                });
            }
        }
    }
}

/// Paints a page, or queues it in `quads`; nothing crosses `round`'s
/// corners.
pub(crate) fn paint_page(
    ui: &Ui,
    ((rect, clip), anchor, round): ((Rect, Rect), Anchor, (Rect, f32)),
    look: Lookup,
    (t, shadow): (&Tokens, bool),
    quads: &mut Vec<Quad>,
) {
    // Shadows reach past the page; a cut page casts none.
    let painter = ui.painter().with_clip_rect(clip);
    let shade = clear_of_corners(ui, ui.clip_rect(), round);
    let bands = clear_of_corners(ui, clip, round);
    // Themed pages match the gutter's paper; only this outlines them.
    if shadow {
        let lift = |offset, blur, alpha| egui::Shadow {
            offset: [0, offset],
            blur,
            spread: 0,
            color: Color32::from_black_alpha(alpha),
        };
        // mareader's `0 1px 2px` and `0 8px 24px -6px` box shadows.
        for band in &shade {
            band.add(lift(1, 2, 31).as_shape(rect, CornerRadius::ZERO));
            band.add(lift(8, 24, 56).as_shape(rect.shrink(6.0), CornerRadius::ZERO));
        }
    }
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    match look {
        Lookup::Sharp(PageTexture::Gpu(page)) | Lookup::Stale(PageTexture::Gpu(page)) => {
            quads.push(Quad::page((rect, clip), Some(page), t.page, anchor));
        }
        // Without a GPU there is no page pass, so pages stay unthemed.
        Lookup::Sharp(PageTexture::Egui(h)) | Lookup::Stale(PageTexture::Egui(h)) => {
            painter.image(h.id(), rect, uv, Color32::WHITE);
        }
        // A blank page, themed and textured; the fill is the no-GPU fallback.
        Lookup::Missing => {
            for band in &bands {
                band.rect_filled(rect, 0.0, blank(t));
            }
            quads.push(Quad::page((rect, clip), None, t.page, anchor));
        }
        Lookup::Failed => {
            for band in &bands {
                band.rect_filled(rect, 0.0, blank(t));
            }
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "This page couldn't be drawn",
                egui::FontId::proportional(13.0),
                Color32::from_gray(110),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: PageSize = PageSize {
        width: 612.0,
        height: 792.0,
    };

    fn column(zoom: f32) -> Layout {
        let inputs = Inputs {
            mode: ViewMode::Vertical,
            frame: vec2(644.0, 800.0),
            inset: 0.0,
            zoom,
            focus: 0,
            gap: GAP,
            margin: MARGIN,
            align: egui::Align::Center,
            rtl: false,
            lead: 0,
        };
        Layout::new(&[LETTER; 20], inputs)
    }

    #[test]
    fn zooming_holds_the_spot_under_the_pivot() {
        let (old, new) = (column(1.0), column(2.0));
        let (offset, viewport) = (vec2(0.0, 5_000.0), vec2(644.0, 800.0));
        let centre = viewport / 2.0;
        let before = old.locate(Pos2::ZERO + offset + centre).unwrap_or_default();
        let to = hold(&old, &new, offset, (centre, centre)).unwrap_or_default();
        let after = new.locate(Pos2::ZERO + to + centre).unwrap_or_default();
        assert_eq!(after.0, before.0);
        assert!(
            (after.1 - before.1).length() < 1e-3,
            "{after:?} vs {before:?}"
        );
    }

    #[test]
    fn a_resized_pane_resets_its_book_once_it_holds_still() -> Result<(), &'static str> {
        let mut view = PdfView::new(crate::testing::doc()?);
        // The first page size sets at once.
        assert_eq!(view.settle_sheet(Some((400.0, 600.0)), 0.0), None);
        assert!(std::mem::take(&mut view.sheet_moved));
        // A resize waits for the pane to hold still.
        let wider = Some((500.0, 600.0));
        assert!(view.settle_sheet(wider, 1.0).is_some());
        assert!(view.settle_sheet(wider, 1.2).is_some());
        assert_eq!(view.sheet(), Some((400.0, 600.0)));
        assert_eq!(view.settle_sheet(wider, 1.31), None);
        assert_eq!(view.sheet(), wider);
        assert!(std::mem::take(&mut view.sheet_moved));
        // A strip goes back to the card at once.
        assert_eq!(view.settle_sheet(None, 1.4), None);
        assert_eq!((view.sheet(), view.sheet_moved), (None, true));
        Ok(())
    }

    #[test]
    fn a_mode_switch_keeps_the_page_and_a_turn_moves_it() -> Result<(), &'static str> {
        let mut view = PdfView::new(crate::testing::doc()?);
        view.current = 7;
        view.set_mode(ViewMode::Spread, 0.0);
        assert_eq!((view.focus, view.current, view.reveal), (6, 7, Some(6)));
        assert_eq!(view.zoom.fit, Some(Fit::Width));
        view.arrow(Arrow::Right, 20, 0.0);
        assert_eq!((view.focus, view.current), (8, 8));
        view.set_mode(ViewMode::Horizontal, 0.0);
        assert_eq!((view.zoom.fit, view.reveal), (None, Some(8)));
        // Along the strip, the arrow is the scroll's, not a turn.
        view.arrow(Arrow::Right, 20, 0.0);
        assert_eq!(view.current, 8);
        view.arrow(Arrow::Up, 20, 0.0);
        assert_eq!(view.current, 8);
        // Across the column, left and right still turn.
        view.set_mode(ViewMode::Vertical, 0.0);
        view.arrow(Arrow::Left, 20, 0.0);
        assert_eq!((view.current, view.reveal), (7, Some(7)));
        Ok(())
    }
}
