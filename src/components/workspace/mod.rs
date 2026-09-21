//! The workspace's sidebar surfaces: the library tree, the active panes and
//! the remote thumbnail grid. Everything here is workspace-shaped on
//! purpose — none of it renders a document, because the workspace never owns
//! one. It renders the reader's FILED library and the host's pane registry,
//! both as value-only state this realm already holds.

mod active;
mod thumbnails;
pub mod thumbnail_source;
/// `pub` because the runtime's tree payload is built with the same
/// hierarchy function the view renders.
pub mod tree;

pub use active::WorkspaceActive;
pub use thumbnails::WorkspaceThumbnails;
pub use tree::WorkspaceLibraryTree;
