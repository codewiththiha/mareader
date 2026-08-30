//! Closing a document: flush the reading position, tear the engine
//! document down, and reset the document/viewer/search state via the
//! explicit reset methods on each state struct (so a new field added to a
//! state struct cannot be silently forgotten here).

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use pdf_engine::api as engine;
use pdf_engine::types::DocStatus;
use crate::state::SidebarMode;
use crate::state::AppState;
use crate::storage::save_library;

/// Close the current document and return to the library shelf.
///
/// Tears the engine's document state down and resets the document / viewer /
/// search signals so the reader lands back on the empty-state bookshelf (which
/// renders whenever `doc.status != Ready`). The library itself is untouched:
/// the just-closed book keeps its saved page so reopening resumes there.
pub fn close_document(state: AppState) {
    // Flush the current reading position NOW, before the signals are reset.
    // The reading-progress effect writes the library signal synchronously but
    // debounces the localStorage save; closing (and then possibly quitting)
    // must not lose the last position to that debounce.
    if state.reader.document.status.get_untracked() == DocStatus::Ready
        && let Some(path) = state.reader.document.path.get_untracked()
    {
        let page = state.reader.viewer.page.get_untracked();
        let mut changed = false;
        state.library.books.update(|books| {
            if let Some(b) = books.iter_mut().find(|b| b.path == path)
                && b.page != page
            {
                b.page = page;
                changed = true;
            }
        });
        if changed {
            if let Err(e) = save_library(&state.library.books.get_untracked()) {
                e.report();
            }
        }
    }

    // Tear the engine document down while the reader is idle on the shelf.
    // destroy() is non-blocking (it drops the loading-task reference
    // synchronously and lets the worker die in the background), so this can
    // never hang, and a fast "close → reopen" is safe because the reopen's
    // own destroy() is idempotent.
    spawn_local(async move {
        _ = engine::destroy().await;
    });

    state.reader.document.reset();
    state.reader.viewer.reset_position();
    state.reader.search.reset();
    // The marks stay on disk under this path; only the in-memory copy goes,
    // so the next open of this book paints them again.
    state.reader.gloss.marks.set(Vec::new());
    state.reader.gloss.processing_id.set(None);
    state.reader.gloss.selection_active.set(false);
    state.reader.gloss.selected_marks.set(std::collections::HashSet::new());
    // Drop any in-flight AI selection/card so a stale popover_open cannot
    // hide the Info button or swallow the first open on the next document.
    state.reader.ai_selection.reset();
    state.ui.sidebar.set(SidebarMode::None);
    // The paper session forgets the book and drops the backdrop back to the
    // theme paper; in-flight samples/scans die with the generation bump.
    pdf_engine::paper::document_close();
}
