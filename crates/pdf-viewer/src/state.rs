//! Viewer-level reactive state, provided to the reusable viewer components.
//! App chrome composes these into its own app-level state.

use leptos::prelude::RwSignal;

use pdf_core::appearance::TextureMode;
use pdf_core::layout::ViewMode;
use pdf_core::math::FitMode;
use pdf_core::search::SearchMatch;
use pdf_engine::types::{DocStatus, OutlineNode, PageSize};

/// Page-host texture, provided via Leptos context by the app shell (derived
/// from settings). `PageCanvas` reads it to pick the `texture-*` class; viewer
/// code never touches settings.
pub type TextureSignal = RwSignal<TextureMode>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    None,
    Outline,
    Thumbs,
}

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
    /// Intrinsic (scale-1) height of every page, 0-based, from engine.open().
    pub page_sizes: RwSignal<Vec<f64>>,
    /// Intrinsic (scale-1) width of every page, 0-based, from engine.open().
    pub page_widths: RwSignal<Vec<f64>>,
    /// Rendered CSS-px heights per page, 0-based, seeded from `page_sizes` and
    /// refined by on_geometry as pages actually render.
    pub page_heights: RwSignal<Vec<f64>>,
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
            page_sizes: RwSignal::new(Vec::new()),
            page_widths: RwSignal::new(Vec::new()),
            page_heights: RwSignal::new(Vec::new()),
        }
    }
}

/// The zoom pipeline signals (see `pdf_viewer::effects`).
#[derive(Clone, Copy)]
pub struct ViewerSignals {
    pub mode: RwSignal<ViewMode>,
    /// 1-based current page.
    pub page: RwSignal<u32>,
    pub scale: RwSignal<f64>,
    pub fit: RwSignal<FitMode>,
    pub scroll_top: RwSignal<f64>,
    /// Scale actually used for rendering (equals `scale` after fit resolves).
    pub render_scale: RwSignal<f64>,
    /// (width, height) of the viewer content area in CSS px.
    pub container_size: RwSignal<(f64, f64)>,
    /// Scale the layout is painted at right now; drives CSS size, never render.
    pub display_scale: RwSignal<f64>,
    /// True while a zoom animation is in flight; renders/geometry are suspended.
    pub zoom_animating: RwSignal<bool>,
    /// `(target_scale, animate, token)` — the token makes every request unique
    /// so mashing `+` retargets the SAME animation instead of being swallowed.
    pub zoom_request: RwSignal<Option<(f64, bool, u64)>>,
    /// The zoom the READER asked for, independent of whether it currently fits
    /// (the ceiling shrink-to-fit grows back to).
    pub desired_scale: RwSignal<f64>,
}

impl Default for ViewerSignals {
    fn default() -> Self {
        Self {
            mode: RwSignal::new(ViewMode::Continuous),
            page: RwSignal::new(1),
            scale: RwSignal::new(1.0),
            fit: RwSignal::new(FitMode::None),
            scroll_top: RwSignal::new(0.0),
            render_scale: RwSignal::new(1.0),
            container_size: RwSignal::new((800.0, 600.0)),
            display_scale: RwSignal::new(1.0),
            zoom_animating: RwSignal::new(false),
            zoom_request: RwSignal::new(None),
            desired_scale: RwSignal::new(1.0),
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

/// The viewer's slice of app state: everything the reusable components and
/// effects read/write. The app shell builds this from its own state and hands
/// it to the viewer; all field paths match the app-level state exactly, so a
/// component works unchanged in either context.
#[derive(Clone, Copy)]
pub struct ViewerState {
    pub doc: DocumentState,
    pub viewer: ViewerSignals,
    pub search: SearchState,
    pub sidebar: RwSignal<SidebarMode>,
}

impl ViewerState {
    /// Compose a viewer state from the individual pieces (used by the app
    /// shell when wiring the reader).
    pub fn new(
        doc: DocumentState,
        viewer: ViewerSignals,
        search: SearchState,
        sidebar: RwSignal<SidebarMode>,
    ) -> Self {
        Self { doc, viewer, search, sidebar }
    }
}
