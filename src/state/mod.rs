//! App-level state: the persisted settings, the library and its rows, and the
//! UI chrome (which rail panel is open, the toast, the window flags). The
//! open document's own state is NOT here — it is `reader_app::state`, and
//! this module re-exports the two names the shell keeps reaching for so a
//! caller does not have to know which crate owns them. The rest of that
//! module's surface the shell names directly, because a re-export nobody
//! reads is just a second path to the same thing.
//!
//! Pure domain logic lives in the `-core` crates: the reader's in
//! `reader-core`, the library's in `library-core`, the engine bridge in
//! `pdf-engine`, and the document lifecycle operations in `services`.

pub mod app;
pub mod library;

pub use app::{AppearanceSignal, AppState, SidebarMode, Toast};
pub use reader_app::state::{ReaderState, TextureSignal};
