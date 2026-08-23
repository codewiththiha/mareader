//! Reader-level reactive state: the document, the viewer signals and the
//! search state. Pure UI chrome (sidebar, toast) lives in `state/ui` +
//! `state/app`; pure domain logic in `pdf-core`.

use leptos::prelude::{Memo, RwSignal, Set};

use pdf_core::appearance::TextureMode;
use pdf_core::layout::ViewMode;
use pdf_core::math::FitMode;
use pdf_core::search::SearchMatch;
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
        self.page1_size.set(None);
        self.metrics.reset();
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
            page1_size: RwSignal::new(None),
            metrics: PageMetrics::default(),
        }
    }
}

/// The five zoom-pipeline scales, one newtype so they cannot drift apart
/// across modules (was a data clump of loose `f64` signals).
#[derive(Clone, Copy)]
pub struct ZoomState {
    /// Committed scale (what the last render used after settle).
    pub scale: RwSignal<f64>,
    /// Scale the layout is painted at right now; drives CSS size, never render.
    pub display: RwSignal<f64>,
    /// Scale actually used for rasterising (equals `scale` after fit resolves).
    pub render: RwSignal<f64>,
    /// The zoom the READER asked for, independent of whether it currently fits.
    pub desired: RwSignal<f64>,
    /// `(target_scale, animate, token)` — token makes every request unique.
    pub request: RwSignal<Option<(f64, bool, u64)>>,
}

impl Default for ZoomState {
    fn default() -> Self {
        Self {
            scale: RwSignal::new(1.0),
            display: RwSignal::new(1.0),
            render: RwSignal::new(1.0),
            desired: RwSignal::new(1.0),
            request: RwSignal::new(None),
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
    /// True while a zoom animation is in flight; renders/geometry are suspended.
    pub zoom_animating: RwSignal<bool>,
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
}

impl ViewerSignals {
    /// Reset the reading position (page + scroll) on document close. Kept
    /// separate from a full reset: fit/zoom state is the reader's, not the
    /// document's.
    pub fn reset_position(&self) {
        self.page.set(1);
        self.scroll_top.set(0.0);
    }
}

impl Default for ViewerSignals {
    fn default() -> Self {
        Self {
            mode: RwSignal::new(ViewMode::Continuous),
            page: RwSignal::new(1),
            fit: RwSignal::new(FitMode::None),
            scroll_top: RwSignal::new(0.0),
            zoom: ZoomState::default(),
            container_size: RwSignal::new((800.0, 600.0)),
            zoom_animating: RwSignal::new(false),
            selected_pages: RwSignal::new(None),
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

/// The reader's slice of app state: everything the PDF components and the
/// reader effects read/write. Sidebar/UI chrome is deliberately NOT here —
/// it is app chrome state, passed in explicitly where the reader needs it.
#[derive(Clone, Copy)]
pub struct ReaderState {
    pub document: DocumentState,
    pub viewer: ViewerSignals,
    pub search: SearchState,
}

impl Default for ReaderState {
    fn default() -> Self {
        Self {
            document: DocumentState::default(),
            viewer: ViewerSignals::default(),
            search: SearchState::default(),
        }
    }
}
