//! Not a question — an answer: a folder the library already reads in place is named,
//! and closing the note lights its shelf up.

use crate::state::AppState;
use crate::state::library::{AlreadyNote, NoteKind};

/// Not a question: a folder the library reads in place cannot be imported twice — the
/// second import would either duplicate every book in it or silently do nothing. The
/// ground is reconciled instead (the covering tree's walk, on the reader's own ask).
pub fn raise_note(state: AppState, shelf_id: String, name: String, kind: NoteKind) {
    state.library.already_imported.raise(AlreadyNote {
        shelf_id,
        name,
        kind,
    });
}

/// The highlight is the modal's own close effect's job, so every way out ends on the shelf being lit.
/// [`Sheet::lower`] rather than a bare `open.set(false)`: the ask STAYS for the effect that
/// consumes it and reveals — the type says "closed, question still held", which is the whole
/// difference between this sheet and a dismissed one.
pub fn close_already_imported(state: AppState) {
    state.library.already_imported.lower();
}
