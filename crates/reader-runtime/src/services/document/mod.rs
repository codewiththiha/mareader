//! The document lifecycle: open and close, through entry points with no UI.

pub mod close;
pub(crate) mod flush;
pub mod open;
pub(crate) mod session;

pub use close::prepare_leave;
pub(crate) use flush::flush_read_point;
pub use open::{init_open_file_handling, open_dialog, open_path};

use leptos::prelude::*;

/// The key a document's highlights are stored under: its library row id.
pub(crate) fn gloss_key(state: crate::context::ReaderContext) -> String {
    state
        .reader
        .document
        .book_id
        .get_untracked()
        .unwrap_or_default()
}
