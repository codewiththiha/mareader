//! The document lifecycle: opening (dialog, path, OS file events, a library
//! row) and closing. Driven by the toolbar button, Ctrl+O, drag-and-drop, the
//! library shelf, and the OS "Open with" handoff — all through the same entry
//! points, none of which depend on UI.
//!
//! [`gloss_key`] is the one fact the lifecycle owns that is not about the
//! engine: which book the open document is. The address alone cannot say —
//! the library may hold two rows of one file — and every reader of the
//! highlights and writer of a resume point needs the answer. It is the row's
//! id, so a conversion or a move keeps the marks without carrying them
//! anywhere.

pub mod close;
pub mod open;
pub(crate) mod session;

pub use close::close_document;
pub use open::{init_open_file_handling, open_dialog, open_path, open_row};

use leptos::prelude::*;

use crate::state::AppState;

/// The key the open document's highlights are stored under: the id of the
/// row the library holds for it.
///
/// An id rather than a string derived from the address: an id does not move
/// when the bytes do, so a conversion or a merge keeps the marks where they
/// are, and two rows of one file keep their lists apart because each has its
/// own id.
///
/// Every writer of the marks asks here rather than reading the document's
/// path — the load at open (`crate::services::document::open::enter`), the
/// save per stroke (`crate::components::ai::gloss::controller`) and the
/// sweep on a removal (`crate::services::library::arrange`) — so the three
/// cannot disagree about which list they mean.
///
/// Empty when nothing is open or the open has no row the library can name;
/// every caller reads that as "nowhere to put it" rather than as a key. An
/// address-only open settles onto its row before any tail loads the marks
/// (`crate::services::document::open`), so the empty window is the window
/// where there is no book to have marks about.
pub(crate) fn gloss_key(state: AppState) -> String {
    state
        .reader
        .document
        .book_id
        .get_untracked()
        .unwrap_or_default()
}
