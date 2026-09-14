//! Not a question — an answer: a folder the library already reads in place is
//! named, and closing the note lights its shelf up. One raiser for the note's
//! two sentences; [`NoteKind`] is the difference between them.
//!
//! [`NoteKind`]: crate::state::library::NoteKind

use leptos::prelude::*;

use crate::state::library::{AlreadyNote, NoteKind};
use crate::state::AppState;

/// Tell the reader the folder they picked is already a shelf here, and light
/// that shelf up when they acknowledge it.
///
/// Not a question: a folder the library reads in place cannot be imported
/// twice — the second import would either duplicate every book in it or
/// silently do nothing, and both read as broken. So the ground is reconciled
/// instead (the covering tree's walk, on the reader's own ask), and the note
/// is what a walk that found every book already standing owes the reader: a
/// sentence and a highlight, with the reveal riding the modal's close rather
/// than its open, because a light that burns its 1.6 seconds behind a modal
/// nobody has dismissed is a light nobody sees.
///
/// One raiser for the note's sentences, because they are one act with one
/// difference and [`NoteKind`] is that difference: [`NoteKind::NothingNew`]
/// is the report of a re-import walk that reconciled the tree — its root or
/// any rung of it — and found every book already standing, and
/// [`NoteKind::Returned`] is the fold's report — the import put a shelf back
/// inside the family its directory names, and the note names the shelf that
/// went home. Raisers that differed by a boolean were that many places to
/// keep in step about which sentence the reader was owed.
pub fn raise_note(state: AppState, shelf_id: String, name: String, kind: NoteKind) {
    state.library.already_imported.raise(AlreadyNote {
        shelf_id,
        name,
        kind,
    });
}

/// Acknowledge the "already imported" note. The highlight is the modal's own
/// close effect's job, so every way out — the button, the backdrop, Escape,
/// the lane — ends on the shelf being lit.
pub fn close_already_imported(state: AppState) {
    state.library.already_imported.open.set(false);
}
