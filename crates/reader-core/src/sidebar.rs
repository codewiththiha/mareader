//! Which panel the reading rail is showing.
//!
//! UI chrome state, not viewer state: nothing here renders, and nothing here
//! knows what a document is. It lives in the reader's format-agnostic core
//! because both halves of the app hold it — the shell's layout controller
//! decides what the rail's presence does to the page, and the rail itself
//! (which the reader owns) decides which of its panels is mounted.

/// Which sidebar panel is open. UI chrome state, not viewer state:
/// reader-side rendering receives it as a plain signal when it needs to know
/// and never owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    None,
    Outline,
    Thumbs,
}
