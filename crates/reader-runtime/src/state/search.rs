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
    /// A PDF index build is in flight. Every keystroke fires a search run,
    /// and the first search of a big book takes seconds to build its index;
    /// the runs that arrive mid-build skip the build instead of stacking a
    /// second extraction — the building task queries the latest text when it
    /// lands. Cleared when the build ends, when the overlay is dismissed
    /// (which cancels the owner-scoped task), and on reset.
    pub building: RwSignal<bool>,
    /// Floating-search overlay visibility; read+written by shortcuts.
    pub visible: RwSignal<bool>,
    /// The bar has been dismissed but its highlights are still on screen,
    /// muted; the next real interaction ends the grace period.
    pub dismissed: RwSignal<bool>,
}

impl SearchState {
    /// Back to the no-search state (fresh document or close). The floating
    /// overlay must not linger after opening/closing a document.
    ///
    /// Every field is bound with no `..` rest, so a field added to the struct is
    /// a compile error here rather than a value carried over from the document
    /// just closed. The handles are `Copy`, so this binds the signals the
    /// struct already holds; `Self::default()` would allocate a fresh arena node
    /// per field on every close and leak them.
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
