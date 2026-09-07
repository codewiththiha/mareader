//! The raster appearance pipeline: the canvas filter chain, the blend mode
//! and the tinted UI-token overrides.
//!
//! "Raster" is what it keys on: a page that arrives as a bitmap (today, every
//! PDF page) is always light, so base mode and tint reach it through CSS
//! filters over the pixels, and the seven `--color-*` UI tokens are overridden
//! from the same maths. A page painted as DOM type does not import this
//! module — it derives its own palette ([`crate::appearance::reflowable`]).
//! The shared kernel both build on is [`crate::appearance::shared`].
//!
//!   * [`filter`] — `canvas_filter()` / `canvas_blend()`
//!   * [`tint`]   — the tinted UI-token overrides that ride along

pub mod filter;
pub mod tint;
