//! The Markdown format's views.
//!
//! `md-core` decides where a document's blocks are; this module decides what one
//! looks like once it has been laid out — a rendered construct, in a wrapper the
//! stylesheet owns. Everything else about a Markdown file is the machinery in
//! [`super::reflow`].
//!
//! A note on the boundary with the parser: the blocks still hold their
//! Markdown SOURCE and this module runs them through `leptos-md`, one
//! construct at a time. Rendering to HTML in `md-core` would mean a second
//! Markdown implementation, and the paginator only needs the boundaries and the
//! kind.

mod block;

pub use block::MdBlockView;
