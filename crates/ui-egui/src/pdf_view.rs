//! A PDF in a pane, in any view mode, drawn as pages arrive.

use std::sync::Arc;

use egui::emath::GuiRounding as _;
use egui::scroll_area::{DragScroll, ScrollAreaOutput as ScrollOutput, ScrollSource};
use egui::style::ScrollAnimation;
use egui::{Color32, CornerRadius, Pos2, Rect, ScrollArea, Ui, Vec2, pos2, vec2};
use mareader_appearance::{PageTone, Rgb};
use mareader_engine::host::Shared;
use mareader_engine::pane::{Arrow, Fit, ViewMode, ZoomCommand};
use mareader_engine::reading::Place;
use mareader_engine::texts::TextLayers;
use mareader_library::MarkId;
use mareader_pdf::{Bounds, LinkTarget, web_address};
use mareader_search::reveal;
use mareader_session::{Band, DocId, Document, PageKey, PageSize, Part, Scale, Tiling, Want};
use mareader_settings::{ColumnAlign, HighlightSettings, LayoutSettings, StartupFit, TextSettings};

use crate::gpu::{Anchor, PagePass, PagesCallback, Quad};
use crate::layout::{self, GAP, Inputs, Layout, MARGIN};
use crate::page_ai;
use crate::pages::{Lookup, PageCache, PageTexture};
use crate::reach::{self, Press, Stop};
use crate::read_aloud::marker::{self, Marker};
use crate::selection::{self, Pages, Sheet};
use crate::study::{self, BarPress, Gloss};
use crate::study_mode::gate::StudySpan;
use crate::theme::Tokens;
use crate::vocab;
use crate::zoom::PaneZoom;
use crate::{keymap, motion};

mod input;
mod interactions;
mod interactive;
mod navigation;
mod overlay;
mod paint;
mod render;
mod scroll;
mod steer;
#[cfg(test)]
mod tests;

use interactions::{
    ai_bubbles, at_stop, copy, footnote, mark_of, open_picture, selected_text, selection_bar,
};
use navigation::{hold, land, progress, sheet_for};
pub(crate) use render::{FIND_CLEARANCE, blank, keep_pictures, paint_page};
use render::{Tiles, streamed};
pub use steer::{AUTO_SPEED, line_step, screen_step};
use steer::{Glide, keyed_scroll};

/// Pages drawn ahead of the viewport, each way, in turns.
const PREFETCH: usize = 1;
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
    pub zoom: PaneZoom,
    /// The paged modes' page; the strips follow the scroll instead.
    focus: usize,
    layout: Option<Layout>,
    /// A page to bring to the start of the view next frame.
    reveal: Option<usize>,
    /// An offset to reach, and the frames left to reach it.
    scroll_to: Option<(Vec2, u8)>,
    offset: Vec2,
    viewport: Vec2,
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
    /// The strip's offset once it landed on the place's page; stale once moved.
    landed_at: Option<Vec2>,
    /// Zoom steps ease, as of last frame's motion settings.
    ease: bool,
    /// Until first drawn: the default fit is still to apply.
    fresh: bool,
    /// A page of another size the zoom waits to follow, and since when.
    resize: Option<(usize, f64)>,
    /// A strip's spot to keep once the pages are laid out anew.
    keep: Option<(usize, Vec2)>,
    /// When the view last moved, for tiles to wait on.
    moved_at: f64,
    /// A book's footnote open in a card on its mark.
    pub footnote: Option<crate::footnote::Footnote>,
    /// Clip transcripts open in the book's pages.
    pub transcripts: mareader_engine::transcript::Transcripts,
    /// What the book's triggers have shown, hidden and played.
    pub triggers: mareader_engine::triggers::Triggers,
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
    /// Stable scroll-area salt across `DocId` swaps of the same open book.
    scroll_id: Option<DocId>,
    /// Last frame's pixel density and texture ceiling, for retypeset priming.
    ppp: f32,
    max_side: f32,
    /// Live `:hover` transitions and `@keyframes` animations over EPUB pages.
    interactive: interactive::InteractiveState,
    /// The pages study mode lets the pane reach, as of this frame.
    study_span: StudySpan,
}

impl Default for PdfView {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfView {
    pub fn new() -> Self {
        Self {
            zoom: PaneZoom::default(),
            focus: 0,
            layout: None,
            reveal: None,
            scroll_to: None,
            offset: Vec2::ZERO,
            viewport: Vec2::ZERO,
            marker: None,
            sheets: Vec::new(),
            paper_at: None,
            fresh: true,
            resize: None,
            keep: None,
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
            landed_at: None,
            ease: true,
            place: None,
            placed_at: None,
            land: None,
            sheet: None,
            sheet_want: None,
            sheet_moved: false,
            scroll_id: None,
            ppp: 1.0,
            max_side: 4096.0,
            interactive: interactive::InteractiveState::default(),
            study_span: StudySpan::default(),
        }
    }
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
    /// Active look transition blending from a prior pane state.
    pub fade: Option<crate::crossfade::PaneFade>,
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
    /// Words recalled in study mode, in their own look.
    pub settled: Vec<crate::vocab::Settled>,
    /// The find bar's hits, in the pane it searches.
    pub found: Option<mareader_engine::search::Lit>,
    /// The phrase read aloud, in the pane reading.
    pub aloud: Option<mareader_engine::read_aloud::lit::Lit>,
    /// Shots: opens the newest drawn mark's menu.
    pub menu_shot: bool,
    /// The page AI's bubbles and results on the pages shown.
    pub ai: page_ai::PaneAi,
    /// The machine, so a clip a page links to is handed to its own opener.
    pub host: Shared,
    /// Study mode's span: the pane stays between its start and its gate.
    pub study_span: StudySpan,
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
    pub vocab: Option<&'a mut (dyn mareader_engine::vocab::ShadingSource + 'static)>,
}

/// Frame context passed through the four show stages.
pub(super) struct ShowContext<'a> {
    ui: Option<&'a mut Ui>,
    view: &'a mut PdfView,
    /// Where this pane is read: the map's, so a turn is the engine's.
    place: &'a mut Place,
    doc: &'a Arc<Document>,
    view_cx: &'a ViewContext,
    pages: &'a mut PageCache,
    demand: &'a mut Demand,
    texts: &'a mut TextLayers,
    drawn: &'a mut Vec<Quad>,
    pass: Option<&'a Arc<PagePass>>,
    gloss: &'a mut Vec<Gloss>,
    vocab: Option<&'a mut (dyn mareader_engine::vocab::ShadingSource + 'static)>,
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
    /// The artwork/PDF page tone kept for fixed-layout EPUB pages.
    fixed_tone: PageTone,
    reflow: bool,
    picture_tone: PageTone,
    emoji_tone: PageTone,
    fade: Option<FadeTones>,
}

#[derive(Clone, Copy)]
pub(super) struct FadeTones {
    t: Tokens,
    fixed_tone: PageTone,
    picture_tone: PageTone,
    emoji_tone: PageTone,
    paper: Option<Rgb>,
    progress: f32,
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

pub fn show(
    ui: &mut Ui,
    view: &mut PdfView,
    place: &mut Place,
    doc: &Arc<Document>,
    io: Sinks<'_>,
    cx: &ViewContext,
) {
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
    let bands = doc.bands().filter(|_| place.mode == ViewMode::Vertical);
    view.study_span = cx.study_span;
    let mut context = ShowContext {
        ui: Some(ui),
        view,
        place,
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
    context.place.zoom = context.view.zoom.sharp();
    if context.view.layout.is_none() {
        return;
    }
    scroll::run(&mut context);
    paint::run(&mut context);
    overlay::run(&mut context);
    context.place.zoom = context.view.zoom.sharp();
}
