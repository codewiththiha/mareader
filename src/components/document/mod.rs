//! Components whose purpose is displaying PDF documents: the page canvas,
//! the four view layouts, and the shells they share. Pure PDF math stays in
//! `pdf-core`; the pdf.js bridge in `pdf-engine`; generic virtualization math
//! in `virtual-list`. Shared DOM lookups live in
//! [`crate::components::primitives::hooks::dom`].
//!
//! [`Viewer`] is the dispatch point; the layouts under `layouts/` arrange
//! pages, the shells under `shells/` own the shared scroller chrome, and
//! [`PageStrip`] is the axis-generic virtualized strip both scroll modes use.

pub mod layouts;
pub mod page_canvas;
pub mod page_strip;
pub mod shells;
pub mod viewer;

pub(crate) use page_canvas::PageCanvas;
pub(crate) use page_strip::PageStrip;
pub(crate) use viewer::Viewer;
