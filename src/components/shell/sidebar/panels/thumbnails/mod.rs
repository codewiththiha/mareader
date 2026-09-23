//! Thumbnail grid, split by responsibility:
//!
//!   * `geometry` — the grid's fixed dimensions and the pure maths derived
//!     from them.
//!   * `thumbnail_cell` — one thumbnail: its engine render, cache fast-path and
//!     reveal animation.
//!   * `auto_center` — the glide / grace / debounce machinery that follows the
//!     reader's page (and the reveal-active listener): the pure timing rules
//!     first, then the effect installation that wires them up.
//!   * `panel` — the scroll container: virtualization window and
//!     document-change invalidation; wires in the auto-center effects.
//!   * `view` — the rail's host around `panel`.

#[cfg(all(feature = "format-pdf", target_arch = "wasm32"))]
pub mod auto_center;
pub mod geometry;
#[cfg(all(feature = "format-pdf", target_arch = "wasm32"))]
pub mod panel;
#[cfg(all(feature = "format-pdf", target_arch = "wasm32"))]
pub mod thumbnail_cell;
pub mod view;
