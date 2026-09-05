//! The raster appearance pipeline: the canvas filter chain, the blend mode
//! and the tinted UI-token overrides.
//!
//! Raster-specific by construction, and "raster" is the honest name for what
//! it keys on: a page that arrives as a bitmap (today, every PDF page) is
//! always light, so base mode and tint reach it through CSS filters over the
//! pixels, and the seven `--color-*` UI tokens are overridden from the same
//! maths. A page painted as DOM type does not import this module — it derives
//! its own palette directly (see [`crate::appearance::reflowable`]). The shared
//! kernel both pipelines build on lives in [`crate::appearance::shared`].
//!
//!   * [`filter`] — `canvas_filter()` / `canvas_blend()`: the raster pipeline
//!   * [`tint`]   — the tinted UI-token overrides that ride along with it

pub mod filter;
pub mod tint;
