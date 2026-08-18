//! App-level state: the viewer pieces come from `pdf_viewer::state`; this adds
//! the app chrome (settings, library, covers, toast). Field paths match the
//! viewer state exactly, so components work unchanged in either context.

use std::collections::HashMap;

use leptos::prelude::RwSignal;

use crate::core::library::{CoverImage, RecentBook};
use pdf_core::settings::Settings;
use pdf_viewer::state::{DocumentState, SearchState, SidebarMode, ViewerSignals};

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

#[derive(Clone, Copy)]
pub struct AppState {
    pub settings: RwSignal<Settings>,
    pub doc: DocumentState,
    pub viewer: ViewerSignals,
    pub search: SearchState,
    pub sidebar: RwSignal<SidebarMode>,
    /// Current toast (if any), rendered by the app-root `ToastHost`.
    pub toast: RwSignal<Option<Toast>>,
    /// The "recent books" library: recently opened documents, most-recent
    /// first, each carrying its last-reached page.
    pub library: RwSignal<Vec<RecentBook>>,
    /// Cover art (page-1 JPEG data URLs) keyed by path.
    pub covers: RwSignal<HashMap<String, CoverImage>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: RwSignal::new(Settings::default()),
            doc: DocumentState::default(),
            viewer: ViewerSignals::default(),
            search: SearchState::default(),
            sidebar: RwSignal::new(SidebarMode::None),
            toast: RwSignal::new(None),
            library: RwSignal::new(Vec::new()),
            covers: RwSignal::new(HashMap::new()),
        }
    }
}
