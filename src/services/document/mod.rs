//! The document lifecycle: opening (dialog, path, OS file events) and
//! closing. Driven by the toolbar button, Ctrl+O, drag-and-drop, the
//! library shelf, and the OS "Open with" handoff — all through the same
//! two entry points, none of which depend on UI.

pub mod close;
pub mod open;

pub use close::close_document;
pub use open::{init_open_file_handling, open_dialog, open_path};
