//! Thumbnail grid, split by responsibility:
//!
//!   * `geometry`    — the grid's fixed dimensions and timings, and the pure
//!     maths derived from them.
//!   * `cell`        — one thumbnail: its engine render, cache fast-path and
//!     reveal animation.
//!   * `auto_center` — the glide / grace / debounce machinery that follows the
//!     reader's page (and the reveal-active listener).
//!   * `panel`       — the scroll container: virtualization window and
//!     document-change invalidation; wires in the auto-center effects.

pub mod auto_center;
pub mod cell;
pub mod geometry;
pub mod panel;

pub use panel::ThumbnailsPanel;
