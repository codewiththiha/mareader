//! The shared maths of laying reflowable text out: blocks, pages, typography.

#![forbid(unsafe_code)]

pub mod block;
pub mod geometry;
pub mod pager;
pub mod search;
pub mod source;
pub mod typography;

// Only `reflow_core::geometry(book_layout)` is read through the root path.
pub use geometry::geometry;
