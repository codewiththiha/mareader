//! Page-paper colour logic for the blend backdrop, as a reusable crate.
//!
//! The reader paints its background with the PDF's own paper colour (the
//! colour the file's pages actually are — white, cream, scanned grey). That
//! sounds like one canvas read; it grew into three interlocking concerns:
//!
//! * **Configuration** — which pixels of a page are trusted to carry the
//!   colour ([`PaperArea::WholePage`] or the margin strips of
//!   [`PaperArea::Edges`]), plain fields on [`PaperConfig`].
//! * **Detection** — [`PaperDetector`] turns raw RGBA rasters into one
//!   dominant colour.
//! * **Resolution** — [`PagePalette`] answers "what colour is the reader
//!   looking at right now" at a fractional page position, blending between
//!   neighbouring pages the way the viewport actually straddles them.
//!
//! The crate is pure: no wasm, no DOM, no leptos. The TS engine stays the
//! eyes (it owns the canvases and pdf.js); this crate is the brain, and it
//! is exercised by ordinary `cargo test` on the host.

mod color;
mod config;
mod detect;
mod palette;

pub use color::{lerp, Rgb};
pub use config::{PaperArea, PaperConfig, DEFAULT_EDGE_WIDTH};
pub use detect::{with_sample_buf, PaperDetector, PAPER_SHARE};
pub use palette::PagePalette;
