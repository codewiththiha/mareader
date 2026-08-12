//! The per-page canvas: one `.pdf-page` host, its bitmap and its text layer.
//!
//!   * `host`      — DOM-level helpers that resize/mask/clean a host element.
//!   * `component` — the reactive `PageCanvas` component that drives them.

mod host;
pub mod component;

pub use component::PageCanvas;
