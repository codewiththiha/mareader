//! Thumbnail grid, split by responsibility:
//!
//!   * `geometry` — the grid's fixed dimensions and timings, and the pure
//!     maths derived from them.
//!   * `cell`     — one thumbnail: its engine render, cache fast-path and
//!     reveal animation.
//!   * `panel`    — the scroll container: virtualization window, auto-center
//!     glide, and document-change invalidation.

pub mod cell;
pub mod geometry;
pub mod panel;

pub use panel::ThumbnailsPanel;
