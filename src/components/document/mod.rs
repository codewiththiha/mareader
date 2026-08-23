//! Components whose purpose is displaying PDF documents: the page canvas
//! and the single/continuous layouts. Pure PDF math stays in `pdf-core`;
//! the pdf.js bridge in `pdf-engine`; generic virtualization math in
//! `virtual-list`.

pub mod continuous_view;
pub mod dom_helpers;
pub mod page_canvas;
pub mod page_list;
pub mod single_page_view;

pub(crate) use continuous_view::ContinuousView;
pub(crate) use page_canvas::PageCanvas;
pub(crate) use page_list::PageList;
pub(crate) use single_page_view::SinglePageView;
