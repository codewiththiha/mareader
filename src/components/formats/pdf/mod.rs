//! The PDF format's views: the rasterised page and the strip that lays the
//! pages out.
//!
//! This module is the only place the reader's UI is allowed to name pdf.js.
//! Everything above it — the layouts, the shells, the chrome — goes through
//! [`viewer::page_host`](crate::components::viewer::page_host), which decides
//! per page whether a raster or real type is the right answer. The two
//! components here take the same `page`/`class`/`texture` surface as their
//! reflowable siblings precisely so that the host can stay a `match` on the
//! format and nothing else.
//!
//! Three tiers of page, one per component: [`PdfPageCanvas`] rasterises (at
//! full resolution or, for a strip's preview ring, at a fraction of it), and
//! `placeholder::PdfPagePlaceholder` is the box a mounted page that is owed no
//! raster at all occupies — no canvas, no engine registration, and one shared
//! miniature between every placeholder in the document.

pub mod canvas;
pub mod canvas_host;
pub mod placeholder;
pub mod strip;

pub use canvas::PdfPageCanvas;
pub use canvas::GlossOverlayProps;
pub use strip::PdfPageStrip;
