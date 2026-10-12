//! Red ink over hard words; a click hands each one to the AI card.

use leptos::prelude::*;
use reader_core::settings::Settings;

use crate::state::ReaderState;

pub mod layer;
pub mod pdf;
pub mod reflow;

/// One text scan per row, shared by the search painter and the ink.
pub(crate) mod scan;

/// Range measurement, batched into single animation frames.
pub(crate) mod measure;

/// A walk's memo, its sink and the fingerprints it is keyed by.
pub(crate) mod walk;

pub(crate) use walk::{
    Sink, WalkKey, WalkMemo, marks_fingerprint, publish, publish_checked, text_fingerprint,
};

/// Hand the cache's misses to the backend, one ask per key.
pub(crate) fn ask(state: ReaderState, misses: Vec<String>) {
    let keys = state.cefr.claim(misses);
    if keys.is_empty() {
        return;
    }
    let asked = keys.clone();
    crate::services::cefr::fetch_levels(keys, move |levels| state.cefr.ingest(asked, levels));
}

/// The pane realm's settings; both painters read their knobs from it.
pub(crate) fn pane_settings() -> RwSignal<Settings> {
    use_context::<RwSignal<Settings>>().expect("the pane realm provides settings")
}
