//! The plain-text format's views.
//!
//! One component today: the block. Everything else about a text document —
//! the page box and the stream — is the shared reflowable
//! machinery in [`super::reflow`], which asks this module only how to paint the
//! inside of a block.

mod block;

pub use block::TxtBlockView;
