//! Not a question but an answer: a folder already read in place is named.

use crate::state::library::{AlreadyNote, NoteKind};

/// A folder read in place cannot be imported twice; the ground reconciles.
pub fn raise_note(
    state: crate::context::LibraryContext,
    shelf_id: String,
    name: String,
    kind: NoteKind,
) {
    state.library.already_imported.raise(AlreadyNote {
        shelf_id,
        name,
        kind,
    });
}

/// The close effect lights the shelf, so every way out ends lit. `lower`
pub fn close_already_imported(state: crate::context::LibraryContext) {
    state.library.already_imported.lower();
}
