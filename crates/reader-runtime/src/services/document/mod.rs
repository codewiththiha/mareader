//! The document lifecycle: opening (dialog, path, OS file events, a library
//! row) and closing. Driven by the toolbar button, Ctrl+O, drag-and-drop, the
//! library shelf, and the OS "Open with" handoff — all through the same entry
//! points, none of which depend on UI.
//!
//! [`flush_read_point`] is what a session ending owes the library: the resume
//! point the progress effect is still debouncing, written now. Both exits call
//! it — the close, and the reload in [`crate::services::reload`].
//!
//! [`gloss_key`] is the one fact the lifecycle owns that is not about the
//! engine: which book the open document is. The address alone cannot say —
//! the library may hold two rows of one file — and every reader of the
//! highlights and writer of a resume point needs the answer. It is the row's
//! id, so a conversion or a move keeps the marks without carrying them
//! anywhere.

pub mod close;
pub(crate) mod flush;
pub mod open;
pub(crate) mod session;

pub use close::close_document;
pub(crate) use flush::flush_read_point;
pub use open::{init_open_file_handling, open_dialog, open_path};

use leptos::prelude::*;

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
/// sweep on a removal (`library_runtime::services::arrange`) — so the three
/// cannot disagree about which list they mean.
///
/// Empty when nothing is open or the open has no row the library can name;
/// every caller reads that as "nowhere to put it" rather than as a key. An
/// address-only open settles onto its row before any tail loads the marks
/// (`crate::services::document::open`), so the empty window is the window
/// where there is no book to have marks about.
pub(crate) fn gloss_key(state: crate::context::ReaderContext) -> String {
    state
        .reader
        .document
        .book_id
        .get_untracked()
        .unwrap_or_default()
}
