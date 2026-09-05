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

pub mod canvas;
pub mod canvas_host;
pub mod strip;

pub use canvas::PdfPageCanvas;
pub use canvas::GlossOverlayProps;
pub use strip::PdfPageStrip;
