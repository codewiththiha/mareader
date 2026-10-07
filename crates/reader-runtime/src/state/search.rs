//! In-document search: the query, its matches, and the overlay's visibility.

use leptos::prelude::*;

use reader_core::search::SearchMatch;

#[derive(Clone, Copy)]
pub struct SearchState {
    pub query: RwSignal<String>,
    pub total: RwSignal<u32>,
    /// Every occurrence of the query, in document order — one entry per match.
    pub matches: RwSignal<Vec<SearchMatch>>,
    /// Index into `matches` of the one the reader is currently on.
    pub active: RwSignal<Option<usize>>,
    pub index_built: RwSignal<bool>,
    /// A PDF index build is in flight.
    pub building: RwSignal<bool>,
    /// Floating-search overlay visibility; read+written by shortcuts.
    pub visible: RwSignal<bool>,
    /// The bar is dismissed but its highlights linger, muted.
    pub dismissed: RwSignal<bool>,
}

impl SearchState {
    /// Back to the no-search state (fresh document or close).
    pub fn reset(&self) {
        let Self {
            query,
            total,
            matches,
            active,
            index_built,
            building,
            visible,
            dismissed,
        } = *self;
        query.set(String::new());
        total.set(0);
        matches.set(Vec::new());
        active.set(None);
        index_built.set(false);
        building.set(false);
        visible.set(false);
        dismissed.set(false);
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
            building: RwSignal::new(false),
            visible: RwSignal::new(false),
            dismissed: RwSignal::new(false),
        }
    }
}
