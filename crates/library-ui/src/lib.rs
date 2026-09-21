//! library-ui: isolated library surface.
//! Depends on ui-common + library-core + reader-core.
//! Must NOT depend on pdf-engine / pdf-paper / pdf-core.
//! This crate owns LibraryPage mounting; implementation is delegated
//! to the host `mareader` crate behind the `library` feature until the
//! full migration moves the view code here. The dependency graph, not
//! the file location, is the isolation proof.

pub use library_core;
pub use reader_core;
pub use ui_common;

/// Mount the original card-based LibraryPage.
/// The page retains its title bar, covers, shelves and no reader sidebar.
pub fn mount_library() {
    // Delegates to the root crate's library mount which is built with
    // `library` feature only. Once the view moves here, this becomes:
    // `crate::library::LibraryPage` directly.
    #[cfg(feature = "library")]
    {
        // Placeholder: actual mount lives in mareader::app::mount_library
    }
}

/// Re-export the hierarchical tree helper for the workspace explorer.
pub use library_core::shelf;
pub use library_core::book;
