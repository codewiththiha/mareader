//! Thumbnail grid, split by responsibility:
//!
//!   * `geometry`    — the grid's fixed dimensions and timings, and the pure
//!     maths derived from them.
//!   * `thumbnail_cell` — one thumbnail: its engine render, cache fast-path and
//!     reveal animation.
//!   * `auto_center` — the glide / grace / debounce machinery that follows the
//!     reader's page (and the reveal-active listener): `math` holds the pure
//!     timing rules, `wiring` the effect installation.
//!   * `thumbnails_panel` — the scroll container: virtualization window and
//!     document-change invalidation; wires in the auto-center effects.

pub mod auto_center;
pub mod thumbnail_cell;
pub mod geometry;
pub mod thumbnails_panel;

pub(crate) use thumbnails_panel::ThumbnailsPanel;
