//! The reflowable appearance pipeline: page colours derived directly in
//! OKLCH, written as `--tx-*` tokens — no CSS filter involved.
//!
//! A bitmap page is an always-light raster, so its appearance works through a
//! CSS filter chain ([`crate::appearance::raster`]). A reflowable page paints
//! real DOM type, so its paper and ink are computed once and assigned
//! outright — deliberately NOT with the raster maths: such a page wants bright
//! paper in Light (wherever the tint sits), darkish grey in Dark, medium-dark
//! grey with dark ink in Dim, ink mostly black on bright paper and mostly
//! white on dark, always carrying a whisper of the paper's hue.
//!
//!   * [`palette`] — [`palette::TextPalette::compute`]: per-mode lightness
//!     anchors + the tint
//!   * [`preview`] — the preset swatch rendered in this palette
//!   * [`tokens`]  — the `--tx-*` CSS variables the palette flattens to

pub mod palette;
pub mod preview;
pub mod tokens;
