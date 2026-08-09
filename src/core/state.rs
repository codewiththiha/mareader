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
pub enum SidebarMode {
    None,
    Outline,
    Search,
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
    /// Rendered CSS-px heights per page, 0-based, filled lazily by on_geometry.
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
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: RwSignal::new(String::new()),
            total: RwSignal::new(0),
            results: RwSignal::new(Vec::new()),
            active: RwSignal::new(None),
            index_built: RwSignal::new(false),
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
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: RwSignal::new(Settings::default()),
            doc: DocumentState::default(),
            viewer: ViewerState::default(),
            search: SearchState::default(),
            sidebar: RwSignal::new(SidebarMode::None),
        }
    }
}

