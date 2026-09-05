//! The PDF page frame: the constants that only mean something once there is
//! a page of paper on screen.
//!
//! Everything the four view modes share — the mode enum, the axis, the page
//! gap, the render budget, the spread arithmetic and the rescale anchor — is
//! format-agnostic and lives in `reader_core::view`, because a plain-text
//! document is scrolled, spread and zoomed through the very same model. What is
//! left here is the band the raster has to clear.

/// Height of the glass toolbar, in CSS px. The viewer scrollport spans the
/// full window height and no view reserves this band: the bar is an overlay
/// that reveals on hover, so the pages start at the very top and travel under
/// it. It remains the reference for anything that must clear the bar while it
/// IS shown (the search reveal's dead zone, the traffic-light centring).
///
/// MUST stay in sync with Tailwind `h-12` on the title bar.
pub const TOOLBAR_H: f64 = 48.0;

