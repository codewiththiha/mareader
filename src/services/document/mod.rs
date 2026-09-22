//! The document lifecycle: opening (dialog, path, OS file events, a library
//! row) and closing. Driven by the toolbar button, Ctrl+O, drag-and-drop, the
//! library shelf, and the OS "Open with" handoff — all through the same entry
//! points, none of which depend on UI.
//!
//! [`flush_read_point`] is what a session ending owes the library: the resume
//! point the progress effect is still debouncing, written now. Both exits call
//! it — the close, and the reload in [`crate::services::reload`].
//!
//! Which book the open document is — [`gloss_key`], re-exported from the
//! reader's state — is the one fact the lifecycle asks that is not about the
//! engine. The address alone cannot say it: the library may hold two rows of
//! one file, and every reader of the highlights and writer of a resume point
//! needs the answer. It is the row's id, so a conversion or a move keeps the
//! marks without carrying them anywhere.

pub mod close;
pub(crate) mod flush;
pub mod open;
pub(crate) mod session;

pub use close::close_document;
pub(crate) use flush::flush_read_point;
pub use open::{init_open_file_handling, open_dialog, open_path, open_row};
// The key the open document's marks are stored under is the reader's fact —
// it reads the open document's row id — so it lives with the reader's state
// and the lifecycle re-exports the question it keeps asking.
pub use reader_app::state::gloss_key;

