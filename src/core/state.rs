//! Global application state, provided via Leptos context.
//!
//! CONTRACT: signal names + field names below are referenced by every feature
//! branch. Keep them stable.

use leptos::prelude::RwSignal;

use crate::core::document::{DocStatus, OutlineNode, PageSize};
use crate::core::layout::ViewMode;
use crate::core::math::FitMode;
use crate::core::search::SearchResult;
use crate::core::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    #[allow(dead_code)] // reserved: info toasts (only errors are emitted so far)
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub kind: ToastKind,
    pub message: String,
}

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
    ///
    /// Unlike `page_heights` this is scale-independent and complete from the
    /// moment the document opens, so `PageList` can lay out the whole column
    /// correctly before anything has rendered.
    pub page_sizes: RwSignal<Vec<f64>>,
    /// Intrinsic (scale-1) width of every page, 0-based, from engine.open().
    ///
    /// Fit and shrink-to-fit read the CURRENT page's width from here, not
    /// `page1_size`. A landscape insert in a portrait book would otherwise
    /// be cropped (or a portrait page over-shrunk) because the ceiling was
    /// computed from the wrong sheet.
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

#[derive(Clone, Copy)]
pub struct ViewerState {
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

    // --- zoom pipeline (see `effects::fit`) ----------------------------------
    // Zoom is a LAYOUT animation over bitmaps we already painted, not a
    // render-driven relayout. That requires separating the two scales:
    //
    //   `display_scale` — what the layout is currently drawn at. Written every
    //     animation frame. Pages CSS-stretch to follow it; nothing re-renders.
    //   `render_scale`  — what the bitmaps were rasterised at. Written ONCE per
    //     gesture, when it settles, so there is exactly one crisp re-render.
    //
    // `scale` remains the committed, user-visible zoom (the toolbar %), and is
    // the value presets/fit compare against.
    /// Scale the layout is painted at right now; drives CSS size, never render.
    pub display_scale: RwSignal<f64>,
    /// True while a zoom animation is in flight. Renders and geometry
    /// write-back are suspended so a mid-flight render can't relayout under us.
    pub zoom_animating: RwSignal<bool>,
    /// A zoom asked for by any control: `(target_scale, animate, token)`. The
    /// token makes every request unique, so mashing `+` retargets the SAME
    /// animation instead of being swallowed as a duplicate signal write.
    pub zoom_request: RwSignal<Option<(f64, bool, u64)>>,

    /// The zoom the READER asked for, independent of whether it currently fits.
    ///
    /// `scale` is what the page is shown at; this is what was requested. They
    /// differ whenever the window (or the sidebar) leaves too little room: the
    /// page is then shown shrunk-to-fit while this remembers the choice, so
    /// the exact original zoom can be restored when the space comes back —
    /// and so growing back STOPS there instead of continuing indefinitely.
    ///
    /// Only a real zoom gesture writes this. A resize never does; that is
    /// precisely what makes the memory survive a resize.
    pub desired_scale: RwSignal<f64>,
}

impl Default for ViewerState {
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
    pub results: RwSignal<Vec<SearchResult>>,
    pub active: RwSignal<Option<usize>>,
    pub index_built: RwSignal<bool>,
    /// Floating-search overlay visibility; read+written by shortcuts (Cmd+F / Escape).
    pub visible: RwSignal<bool>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: RwSignal::new(String::new()),
            total: RwSignal::new(0),
            results: RwSignal::new(Vec::new()),
            active: RwSignal::new(None),
            index_built: RwSignal::new(false),
            visible: RwSignal::new(false),
        }
    }
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub settings: RwSignal<Settings>,
    pub doc: DocumentState,
    pub viewer: ViewerState,
    pub search: SearchState,
    pub sidebar: RwSignal<SidebarMode>,
    /// Current toast (if any), rendered by the app-root `ToastHost`.
    pub toast: RwSignal<Option<Toast>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: RwSignal::new(Settings::default()),
            doc: DocumentState::default(),
            viewer: ViewerState::default(),
            search: SearchState::default(),
            sidebar: RwSignal::new(SidebarMode::None),
            toast: RwSignal::new(None),
        }
    }
}

