//! Page-paper colour logic for the blend backdrop, as a reusable crate.
//!
//! The reader paints its background with the PDF's own paper colour (the
//! colour the file's pages actually are — white, cream, scanned grey). That
//! sounds like one canvas read; it grew into three interlocking concerns:
//!
//! * **Configuration** — which pages the colour comes from
//!   ([`PaperMode::Fixed`] for one book-wide colour, [`PaperMode::Continuous`]
//!   for a per-page palette that follows the scroll) and which pixels of a
//!   page are trusted to carry it ([`PaperArea::WholePage`] or the margin
//!   strips of [`PaperArea::Edges`]), all plain fields on [`PaperConfig`]
//!   with an adjustable scan-page budget (100 by default).
//! * **Detection** — [`PaperDetector`] turns raw RGBA rasters into one
//!   dominant colour, pooling across pages for a book-wide answer.
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
pub use config::{
    PaperArea, PaperConfig, PaperMode, DEFAULT_EDGE_WIDTH, DEFAULT_SCAN_PAGES, MAX_SCAN_PAGES,
    MIN_SCAN_PAGES,
};
pub use detect::{PaperDetector, PAPER_SHARE};
pub use palette::PagePalette;
