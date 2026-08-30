//! Reader-level reactive state: the document, the viewer signals, the
//! search state and the AI text-selection state. Pure UI chrome (sidebar,
//! toast) lives in `state/ui` + `state/app`; pure domain logic in `pdf-core`.

use leptos::prelude::{Get, GetUntracked, Memo, RwSignal, Set, Signal};
use serde::Deserialize;

use pdf_core::appearance::TextureMode;
use pdf_core::gloss::{GlossMark, PageAnchor};
use pdf_core::layout::{PAGE_GAP, ViewMode};
use pdf_core::math::FitMode;
use pdf_core::search::SearchMatch;
use pdf_core::settings::AnimationSettings;
use pdf_engine::types::{DocStatus, OutlineNode, PageSize};

/// Page-host texture, provided via Leptos context by the app shell (derived
/// from settings). `PageCanvas` reads it to pick the `texture-*` class; the
/// reader never touches settings.
pub type TextureSignal = Memo<TextureMode>;

#[derive(Clone, Copy)]
pub struct DocumentState {
    pub status: RwSignal<DocStatus>,
    pub error: RwSignal<Option<String>>,
    pub path: RwSignal<Option<String>>,
    pub num_pages: RwSignal<u32>,
    pub title: RwSignal<Option<String>>,
    pub author: RwSignal<Option<String>>,
    pub outline: RwSignal<Vec<OutlineNode>>,
    /// True while the (lazy) outline resolution is in flight — the panel
    /// shows "resolving" instead of a definitive "No outline" for a book
    /// whose chapters are merely not back yet.
    pub outline_pending: RwSignal<bool>,
    /// CSS-px size of page 1 at scale 1 (used for fit modes before any render).
    pub page1_size: RwSignal<Option<PageSize>>,
    /// Intrinsic + laid-out page geometry (one source of truth).
    pub metrics: PageMetrics,
}

impl DocumentState {
    /// Back to the no-document state. Every field the open flow writes is
    /// reset here, so a field added to the struct cannot be silently
    /// forgotten by close_document.
    pub fn reset(&self) {
        self.status.set(DocStatus::Idle);
        self.error.set(None);
        self.path.set(None);
        self.num_pages.set(0);
        self.title.set(None);
        self.author.set(None);
        self.outline.set(Vec::new());
        self.outline_pending.set(false);
        self.page1_size.set(None);
        self.metrics.reset();
    }

    /// Height-over-width aspect of page 1 (tracked read: subscribes the
    /// caller to `page1_size`). Every fixed-geometry surface that sizes
    /// itself against the first sheet — the thumbnail grid's row height,
    /// the auto-center target — goes through here, so the fallback policy
    /// lives in exactly one place.
    pub fn page1_aspect(&self) -> f64 {
        page_aspect(self.page1_size.get())
    }

    /// Same, read untracked — for rAF/scroll callbacks that must not
    /// subscribe to geometry.
    pub fn page1_aspect_untracked(&self) -> f64 {
        page_aspect(self.page1_size.get_untracked())
    }

    /// The document's human-facing name (tracked read: subscribes the
    /// caller to title and path): its usable title, else the file stem,
    /// else "No document". The three surfaces that show the name — the
    /// toolbar title, the sidebar's document card, the floating label —
    /// used to each hand-roll this with three different fallbacks; the
    /// policy lives here now.
    pub fn display_name(&self) -> String {
        pdf_core::filename::display_name(
            self.title.get().as_deref(),
            self.path.get().as_deref(),
        )
        .unwrap_or_else(|| NO_DOCUMENT.to_string())
    }
}

/// Aspect used while page 1 is unmeasured or degenerate: a 3:4 portrait,
/// the default every fixed-geometry surface historically fell back to.
pub const DEFAULT_PAGE_ASPECT: f64 = 0.75;

/// Name shown when a document has neither a usable title nor a path (the
/// reader shell with nothing open).
pub const NO_DOCUMENT: &str = "No document";

/// Height-over-width aspect of a page size, falling back to
/// [`DEFAULT_PAGE_ASPECT`] when the size is missing or its width is not
/// positive (a zero-width sheet has no meaningful aspect, and dividing by
/// it would poison every height derived from it).
pub fn page_aspect(size: Option<PageSize>) -> f64 {
    match size {
        Some(s) if s.width > 0.0 => s.height / s.width,
        _ => DEFAULT_PAGE_ASPECT,
    }
}

/// Packed page geometry: one `PageSize` per page plus the CSS-px column.
#[derive(Clone, Copy)]
pub struct PageMetrics {
    /// Intrinsic (scale-1) width/height of every page, 0-based.
    pub intrinsic: RwSignal<Vec<PageSize>>,
    /// Rendered CSS-px heights per page, seeded from `intrinsic` and refined
    /// by `on_geometry` as pages actually render.
    pub css_heights: RwSignal<Vec<f64>>,
}

impl PageMetrics {
    pub fn reset(&self) {
        self.intrinsic.set(Vec::new());
        self.css_heights.set(Vec::new());
    }
}

impl Default for PageMetrics {
    fn default() -> Self {
        Self {
            intrinsic: RwSignal::new(Vec::new()),
            css_heights: RwSignal::new(Vec::new()),
        }
    }
}

impl Default for DocumentState {
    fn default() -> Self {
        Self {
            status: RwSignal::new(DocStatus::Idle),
            error: RwSignal::new(None),
            path: RwSignal::new(None),
            num_pages: RwSignal::new(0),
            title: RwSignal::new(None),
            author: RwSignal::new(None),
            outline: RwSignal::new(Vec::new()),
            outline_pending: RwSignal::new(false),
            page1_size: RwSignal::new(None),
            metrics: PageMetrics::default(),
        }
    }
}

/// One zoom intent, posted by whichever surface wants the zoom to change
/// (toolbar buttons, keyboard steps, the fit watcher, the follow watcher).
///
/// The [`crate::viewer::zoom::ZoomController`] is the only consumer: a
/// command is resolved against the current window, mode and page, and lands
/// through the one transition pipeline. Nobody executes a zoom by writing
/// the scale signals directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoomCommand {
    /// One step along the preset ladder: `+1` zooms in, `-1` zooms out.
    Step(i32),
    /// Re-resolve the active fit mode (width / page) against the current
    /// window, view mode and page. Stands down when no fit mode is active.
    Refit,
    /// Re-resolve a manual zoom against the current window. The reader's
    /// chosen `desired` is authoritative up to the clamp: a manual zoom is
    /// never shrunk back to the fit width, so a page the reader zoomed in on
    /// stays at that scale (and overflows with a scroll affordance, rather
    /// than being cropped by a size the app chose).
    Constrain,
    /// The space around the page moved — a sidebar slide or a window drag.
    /// Resolves to whichever of the two above owns the scale (a fit mode when
    /// one is active, the reader's own `desired` when they zoomed by hand) and
    /// is posted on EVERY frame of the burst, because a scale that
    /// waits for the burst to end leaves the host wider than the box it now
    /// has to fit in and the flex engine squishes the paper. Its geometry
    /// lands in the frame it was asked for; its crisp render is held until the
    /// container has been quiet, so the burst costs one raster pass.
    Follow,
}

/// A live zoom transaction: what is animating, from where, to where. Exists
/// for exactly the duration of the transition; `None` means idle.
///
/// There is deliberately no position in here. The layout is rescaled on
/// every frame of the tween and the engine holds the document point under
/// the viewport centre exactly where it is, so a transaction has nothing to
/// remember about where the reader was looking.
#[derive(Debug, Clone, Copy)]
pub struct ZoomTransition {
    /// Visual scale the tween started from, so a retarget continues from
    /// wherever the eye currently is instead of teleporting.
    pub from: f64,
    /// Resolved target scale.
    pub to: f64,
    /// `Date::now()` at (re)targeting; a retarget restarts the clock.
    pub start_ms: f64,
    /// Whether the visual scale should tween; `false` lands on the first frame.
    pub animate: bool,
    /// True while this is a container [`ZoomCommand::Follow`] transaction. The
    /// distinction is load-bearing twice over: a follow's commit is HELD (the
    /// controller lands its geometry in the frame the size was reported and the
    /// settle deadline renders once the burst stops), and a watcher may only
    /// retarget a transaction of this kind — never a gesture's tween.
    pub following: bool,
}

/// The zoom pipeline scales, one type so they cannot drift apart across
/// modules. Three absolute scales, no ratios:
///
/// - `desired` is what the reader asked for, independent of whether it
///   currently fits (it is the ceiling a manual zoom resolves to, and the
///   readout tooltip explains it).
/// - `display` is the live visual scale — the scale the reader is looking at
///   right now. It moves on every frame of a zoom, and it is what the readout,
///   the fit maths, the page hosts and the overlays read.
/// - `committed` is the scale the mounted rasters are crisp at. It jumps
///   exactly once per zoom transaction, when the transition commits, and it
///   is the only scale a page render is issued at.
///
/// A fourth signal, `transition`, carries the in-flight transaction (and its
/// absence is what "not zooming" means). Commands queue on `commands`; the
/// controller is their only consumer.
#[derive(Clone, Copy)]
pub struct ZoomState {
    /// The zoom the reader asked for, independent of whether it currently
    /// fits the window.
    pub desired: RwSignal<f64>,
    /// The live visual scale. Moves every frame of a zoom, and the layout
    /// relayouts to it as it moves.
    pub display: RwSignal<f64>,
    /// The scale the mounted rasters are crisp at (page renders). Changes
    /// once per zoom transaction.
    pub committed: RwSignal<f64>,
    /// The in-flight transition, if any. While present, page/scroll
    /// synchronisation and geometry feedback are frozen.
    pub transition: RwSignal<Option<ZoomTransition>>,
    /// `(command, animate, token)` — the token makes every post unique, so
    /// two identical steps in a row both land.
    pub commands: RwSignal<Option<(ZoomCommand, bool, u64)>>,
    /// Monotonic command counter backing the token above.
    pub seq: RwSignal<u64>,
}

impl ZoomState {
    /// Post a zoom intent to the controller. `animate` asks for the eased
    /// tween; callers passing `false` get a first-frame landing.
    pub fn post(&self, cmd: ZoomCommand, animate: bool) {
        let token = self.seq.get_untracked() + 1;
        self.seq.set(token);
        self.commands.set(Some((cmd, animate, token)));
    }

    /// The scale the in-flight transition is heading to, if any. Manual
    /// steps chain from this so a fast `+ +` advances two presets.
    pub fn in_flight_target(&self) -> Option<f64> {
        self.transition.get_untracked().map(|t| t.to)
    }

    /// Seed every scale for a freshly opened document: no transition, no
    /// layout to animate from, the live scale and the rasters already in
    /// agreement.
    pub fn initialize(&self, scale: f64) {
        self.desired.set(scale);
        self.display.set(scale);
        self.committed.set(scale);
        self.transition.set(None);
    }
}

impl Default for ZoomState {
    fn default() -> Self {
        Self {
            desired: RwSignal::new(1.0),
            display: RwSignal::new(1.0),
            committed: RwSignal::new(1.0),
            transition: RwSignal::new(None),
            commands: RwSignal::new(None),
            seq: RwSignal::new(0),
        }
    }
}

/// Which of the reader's motions animate. Projected from the persisted
/// [`AnimationSettings`] by the app root (`effects::app::motion::publish_motion`)
/// and read by everything that moves a page, so no consumer has to know that a
/// master switch exists: the projection already applied it.
///
/// Read TRACKED by views (the rail's transition class has to change when the
/// reader flips a switch) and UNTRACKED by effects and scroll calls: a flag
/// that stops something animating must not be what triggers the animation.
///
/// Nothing here skips a change. Off renders the end frame in the frame the
/// change arrives, which is why freezing the reader loses no fit, no follow
/// and no scroll target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Motion {
    /// The rail animates its open/close: the docked rail tweens its width
    /// (`SIDEBAR_SLIDE_MS`), the floating rail fades (`SIDEBAR_FADE_MS`).
    pub sidebar_slide: bool,
    /// The page rides a window drag: the canvas flexes on every frame of it.
    /// Riding the RAIL is not in here on purpose — a container that was
    /// measured is answered in the same frame, animation or no animation, and
    /// deferring it is what cropped the page for a visible instant.
    pub canvas_resize: bool,
    /// A zoom eases to its target over the profile's duration.
    pub zoom: bool,
    /// A jump to a page glides the column (or the thumbnail rail) over it.
    pub scroll_glide: bool,
}

impl Motion {
    /// The one place the master switch is honoured. Off, no detail can bring
    /// an animation back on; the Animations tab hides itself for the same
    /// reason, so the detail switches are never shown lying.
    pub const fn from_prefs(p: &AnimationSettings) -> Self {
        Self {
            sidebar_slide: p.enabled && p.sidebar_slide,
            canvas_resize: p.enabled && p.canvas_resize,
            zoom: p.enabled && p.zoom,
            scroll_glide: p.enabled && p.scroll_jumps,
        }
    }
}

impl Default for Motion {
    /// Everything moves. The shell publishes the reader's prefs before
    /// anything can act on them, and a reader that has not been published to
    /// yet (a document opening, a test) must not look broken.
    fn default() -> Self {
        Self {
            sidebar_slide: true,
            canvas_resize: true,
            zoom: true,
            scroll_glide: true,
        }
    }
}

/// The zoom pipeline signals (see `crate::effects`).
#[derive(Clone, Copy)]
pub struct ViewerSignals {
    pub mode: RwSignal<ViewMode>,
    /// 1-based current page.
    pub page: RwSignal<u32>,
    pub fit: RwSignal<FitMode>,
    pub scroll_top: RwSignal<f64>,
    pub zoom: ZoomState,
    /// (width, height) of the viewer content area in CSS px.
    pub container_size: RwSignal<(f64, f64)>,
    /// Inclusive `(first, last)` 1-based page range of the reader's current
    /// text selection, or `None` when no text is selected.
    ///
    /// The `pdfEngine.ts` selectionchange listener walks the DOM from the
    /// selection's anchor and focus up to the nearest `.pdf-page` host, parses
    /// the page index from its id (`cont-{i}-pg`), and dispatches a
    /// `pdfreader:selection-pages` CustomEvent with `{ first, last }` (or
    /// `null` to clear). This effect listens for that event and writes the
    /// range here so `PageList` can PIN those pages in the virtualization
    /// window — otherwise scrolling evicts them, orphaning the selection's
    /// DOM nodes and breaking copy of multi-page selections.
    pub selected_pages: RwSignal<Option<(u32, u32)>>,
    /// Continuous auto-scroll along the active strip (Continuous / Horizontal).
    pub auto_scroll: RwSignal<bool>,
    /// Inter-page gap in the continuous strip (0 when No Gap is on).
    pub page_gap: RwSignal<f64>,
    /// Horizontal inset around pages (CSS px). `0` removes the margin.
    pub page_margin: RwSignal<f64>,
    /// Which motions animate. Written only by the shell, from the settings
    /// (`Motion::from_prefs`); see the type's contract.
    pub motion: RwSignal<Motion>,
}

impl ViewerSignals {
    /// Reset the reading position (page + scroll) on document close. Kept
    /// separate from a full reset: fit/zoom state is the reader's, not the
    /// document's.
    pub fn reset_position(&self) {
        self.page.set(1);
        self.scroll_top.set(0.0);
        self.auto_scroll.set(false);
        self.page_gap.set(PAGE_GAP);
        self.page_margin.set(0.0);
    }

    /// True while a zoom transaction is in flight: renders are suspended,
    /// page/scroll synchronisation and geometry feedback are frozen, and the
    /// mounted window is pinned around the dominant page.
    pub fn zooming(&self) -> Signal<bool> {
        let transition = self.zoom.transition;
        Signal::derive(move || transition.get().is_some())
    }

    /// Untracked variant of [`Self::zooming`] for rAF/scroll callbacks and
    /// effect guards that must not subscribe to the transition.
    pub fn zooming_now(&self) -> bool {
        self.zoom.transition.get_untracked().is_some()
    }

    /// True only while a manual zoom animation is in flight (fit is `None`,
    /// so the reader is zooming by hand rather than re-fitting). When set,
    /// the layouts hand the canvas to the gesture so a fit-driven refit can
    /// never fight the pinch.
    ///
    /// A container follow is deliberately excluded even though it opens a
    /// transition too: a window drag with a hand-picked zoom is not a gesture,
    /// and pages must not start rasterising at a display scale that is already
    /// obsolete two frames later.
    pub fn gesture_owns(&self) -> Signal<bool> {
        let transition = self.zoom.transition;
        let fit = self.fit;
        Signal::derive(move || {
            fit.get_untracked() == FitMode::None
                && transition.get().is_some_and(|t| !t.following)
        })
    }
}

impl Default for ViewerSignals {
    fn default() -> Self {
        Self {
            mode: RwSignal::new(ViewMode::ScrollVertical),
            page: RwSignal::new(1),
            fit: RwSignal::new(FitMode::None),
            scroll_top: RwSignal::new(0.0),
            zoom: ZoomState::default(),
            container_size: RwSignal::new((800.0, 600.0)),
            selected_pages: RwSignal::new(None),
            auto_scroll: RwSignal::new(false),
            page_gap: RwSignal::new(PAGE_GAP),
            page_margin: RwSignal::new(0.0),
            motion: RwSignal::new(Motion::default()),
        }
    }
}

#[derive(Clone, Copy)]
pub struct SearchState {
    pub query: RwSignal<String>,
    pub total: RwSignal<u32>,
    /// Every occurrence of the query, in document order — one entry per match.
    pub matches: RwSignal<Vec<SearchMatch>>,
    /// Index into `matches` of the one the reader is currently on.
    pub active: RwSignal<Option<usize>>,
    pub index_built: RwSignal<bool>,
    /// Floating-search overlay visibility; read+written by shortcuts.
    pub visible: RwSignal<bool>,
    /// The bar has been dismissed but its highlights are still on screen,
    /// muted; the next real interaction ends the grace period.
    pub dismissed: RwSignal<bool>,
}

impl SearchState {
    /// Back to the no-search state (fresh document or close). The floating
    /// overlay must not linger after opening/closing a document.
    pub fn reset(&self) {
        self.query.set(String::new());
        self.total.set(0);
        self.matches.set(Vec::new());
        self.active.set(None);
        self.index_built.set(false);
        self.visible.set(false);
        self.dismissed.set(false);
    }
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: RwSignal::new(String::new()),
            total: RwSignal::new(0),
            matches: RwSignal::new(Vec::new()),
            active: RwSignal::new(None),
            index_built: RwSignal::new(false),
            visible: RwSignal::new(false),
            dismissed: RwSignal::new(false),
        }
    }
}

/// Bounding rectangle of the selected text, in viewport CSS pixels — the
/// "warp window" the AI selection menu anchors to.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct SelectionRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Everything the AI feature needs about the current text selection, as
/// dispatched by the engine's `pdfreader:selection-detail` event.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SelectionDetail {
    /// The exact text the user highlighted.
    pub text: String,
    /// Surrounding sentence (~120 chars from the same text layer) so the
    /// model can disambiguate the word.
    pub context: String,
    /// Tight bounding box around the selection (the "warp window").
    pub rect: SelectionRect,
}

/// Reactive state for the AI text-selection feature: what is selected and
/// whether the explanation popover is open.
#[derive(Clone, Copy)]
pub struct AiSelectionState {
    /// The current selection details, or `None` if nothing is selected.
    pub detail: RwSignal<Option<SelectionDetail>>,
    /// The selection's origin in page space, so the Info pill can follow
    /// scroll and die when it leaves the viewport.
    pub anchor: RwSignal<Option<PageAnchor>>,
    /// Whether the "Info" popover is currently open.
    pub popover_open: RwSignal<bool>,
}

impl Default for AiSelectionState {
    fn default() -> Self {
        Self {
            detail: RwSignal::new(None),
            anchor: RwSignal::new(None),
            popover_open: RwSignal::new(false),
        }
    }
}

impl AiSelectionState {
    /// Clear selection detail, page anchor and the open flag. Called on
    /// document close so a card left open on PDF A cannot poison PDF B
    /// (a stale `popover_open = true` would hide the Info button and make
    /// the next open a no-op).
    pub fn reset(&self) {
        self.detail.set(None);
        self.anchor.set(None);
        self.popover_open.set(false);
    }
}

/// The persisted gloss highlights of the OPEN document.
///
/// One flat list rather than a per-page map: a document has a handful of
/// marks, every page host filters the list itself, and a `Vec` is what both
/// localStorage and the `<For>` in the mark layer want.
#[derive(Clone, Copy, Default)]
pub struct GlossState {
    pub marks: RwSignal<Vec<GlossMark>>,
    /// Gloss multi-select mode (long-press initiated on a mark).
    pub selection_active: RwSignal<bool>,
    /// Ids of the marks currently selected while in multi-select mode.
    pub selected_marks: RwSignal<std::collections::HashSet<String>>,
    /// id of the mark whose "processing" highlighter animation is live, if any.
    ///
    /// Lives here, not in the popover, because the animation is painted by the
    /// in-page mark layer: while the model is working there is NO surface at
    /// all, so the stroke itself has to carry the thinking state.
    pub processing_id: RwSignal<Option<String>>,
}

/// The reader's slice of app state: everything the PDF components and the
/// reader effects read/write. Sidebar/UI chrome is deliberately NOT here —
/// it is app chrome state, passed in explicitly where the reader needs it.
#[derive(Clone, Copy, Default)]
pub struct ReaderState {
    pub document: DocumentState,
    pub viewer: ViewerSignals,
    pub search: SearchState,
    pub ai_selection: AiSelectionState,
    pub gloss: GlossState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_aspect_passes_through_measured_sizes() {
        // US Letter at scale 1: 792/612 ≈ 1.294.
        assert!((page_aspect(Some(PageSize { width: 612.0, height: 792.0 })) - 792.0 / 612.0).abs() < 1e-12);
        // A landscape sheet inverts below 1.
        assert!(page_aspect(Some(PageSize { width: 1000.0, height: 500.0 })) < 1.0);
    }

    #[test]
    fn the_master_switch_freezes_every_detail() {
        let all_on = AnimationSettings::default();
        assert!(all_on.enabled);
        let m = Motion::from_prefs(&all_on);
        assert!(m.sidebar_slide && m.canvas_resize && m.zoom && m.scroll_glide);

        // With the master off, no detail can bring an animation back.
        let mut frozen = AnimationSettings::default();
        frozen.enabled = false;
        assert!(frozen.zoom && frozen.sidebar_slide);
        let m = Motion::from_prefs(&frozen);
        assert!(!m.sidebar_slide);
        assert!(!m.canvas_resize);
        assert!(!m.zoom);
        assert!(!m.scroll_glide);
    }

    #[test]
    fn a_detail_switch_moves_exactly_one_motion() {
        // The tab offers one switch per motion, so each has to own precisely
        // the one it names — and the master has to stay out of their way.
        let mut p = AnimationSettings::default();
        p.zoom = false;
        let m = Motion::from_prefs(&p);
        assert!(!m.zoom);
        assert!(m.sidebar_slide && m.canvas_resize);
        assert!(m.scroll_glide);
    }

    #[test]
    fn page_aspect_falls_back_to_portrait_when_unmeasured_or_degenerate() {
        assert_eq!(page_aspect(None), DEFAULT_PAGE_ASPECT);
        assert_eq!(
            page_aspect(Some(PageSize {
                width: 0.0,
                height: 792.0
            })),
            DEFAULT_PAGE_ASPECT
        );
        // A negative width is just as degenerate: never divide by it.
        assert_eq!(
            page_aspect(Some(PageSize {
                width: -612.0,
                height: 792.0
            })),
            DEFAULT_PAGE_ASPECT
        );
    }
}
