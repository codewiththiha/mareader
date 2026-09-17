//! Closing a document: flush the reading position, tear the engine
//! document down, and reset the document/viewer/search state via the
//! explicit reset methods on each state struct (so a new field added to a
//! state struct cannot be silently forgotten here).

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use pdf_engine::api as engine;
use crate::state::{AppState, SidebarMode};

/// Close the current document and return to the library shelf.
///
/// Tears the engine's document state down and resets the document / viewer /
/// search signals, so the reader lands back on the empty-state bookshelf
/// (which renders whenever `doc.status != Ready`). The library is untouched:
/// the just-closed book keeps its saved page, so reopening resumes there.
pub fn close_document(state: AppState) {
    // Take the document state over from whatever open may still be resolving:
    // an open's tail (its `Ready` flip, its cover, its outline) lands frames
    // after the engine answers, and without this claim a close arriving in
    // that window would be undone by the book it just closed.
    let _ = super::session::claim();

    // Flush the current reading position NOW, before the signals are reset:
    // the reading-progress effect writes the library signal synchronously but
    // debounces the localStorage save, and closing (then possibly quitting)
    // must not lose the last position to that debounce. One function, shared
    // with the reload that ends a session just as finally
    // (`crate::services::reload`).
    super::flush::flush_read_point(state);

    // Tear the engine document down while the reader is idle on the shelf.
    // destroy() is non-blocking — it drops the loading-task reference
    // synchronously and lets the worker die in the background — so this can
    // never hang, and a fast close → reopen is safe: the reopen's own
    // destroy() is idempotent.
    spawn_local(async move {
        _ = engine::destroy().await;
        // Finish the job: destroy() runs the advisory worker cleanup itself
        // now (only it can — later the document is gone), and this tail sweep
        // is the idempotent one the shelf deserves, catching anything that
        // re-landed while the teardown was resolving (a fast close → reopen
        // re-registers hosts before this future wakes).
        engine::sweep();
        engine::sweep_snapshots();
        // The heap probe's other half: what the session left behind on the
        // wasm side once the shelf is as empty as it gets — the retained
        // index (kept for a reopen's adoption), the covers, the library.
        crate::memory::log_heap("close");
    });

    // One call sheds everything the open flow wrote — the identity, the outline
    // and BOTH formats' pages — because `DocumentState::reset` owns that list
    // (including the reflowable half it delegates to). Resetting anything here
    // as well would be a second place to remember.
    state.reader.document.reset();
    state.reader.viewer.reset_position();
    state.reader.search.reset();
    // The marks stay on disk under this path; only the in-memory copy goes,
    // so the next open of this book paints them again.
    state.reader.gloss.reset();
    // Drop any in-flight AI selection/card so a stale popover_open cannot
    // hide the Explain button or swallow the first open on the next document.
    state.reader.ai_selection.reset();
    state.ui.sidebar.set(SidebarMode::None);
    // The paper session forgets the book and drops the backdrop back to the
    // theme paper; in-flight samples die with the generation bump.
    pdf_engine::backdrop::document_close();
}
