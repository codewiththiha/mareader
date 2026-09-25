//! Closing a document: flush the reading position, tear the engine
//! document down, and reset the document/viewer/search state — the runtime's
//! document-close (see `crate::runtime::ReaderRuntime::document_close`).

use crate::state::AppState;

/// Close the current document and return to the library shelf.
///
/// This is the RUNTIME's document-close: the document session tears down
/// (engine destroy awaited, sweeps, reader slices reset) and the reader
/// lands back on the empty-state bookshelf, but the runtime STAYS alive —
/// disposal belongs to the `/reader` route boundary alone. The two
/// operations are distinct by design (Phase 1 §3): a close keeps the
/// runtime ready for the next open; only leaving the reader disposes it.
pub fn close_document(state: AppState) {
    state.runtime.document_close(state);
}
