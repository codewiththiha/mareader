//! Search pipeline: build index once, run search on submit, jump to results,
//! force visible-page re-render to re-apply highlights. OWNED BY branch C
//! (panels/sidebar).

use crate::core::state::AppState;

/// Must be called once from the app root.
pub fn search_effects(_state: AppState) {
    // TODO(branch C): watch search.query submit -> buildSearchIndex (once) +
    // engine.search + clearHighlights + result jump + re-render for highlights.
}
