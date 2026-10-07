//! The open document: its identity, its outline, and the pages both
//! pipelines publish.

pub mod page_metrics;
pub mod reflow;

use std::sync::Arc;

use leptos::prelude::*;

use pdf_core::outline::OutlineEntry;
use reader_core::document::{DocStatus, PageSize};
use reader_core::format::Format;
use reader_core::outline::OutlineNode;

pub use page_metrics::PageMetrics;
pub use reflow::ReflowContent;

#[derive(Clone, Copy)]
pub struct DocumentState {
    pub status: RwSignal<DocStatus>,
    /// Which pipeline renders the document; PDF while nothing is open.
    pub format: RwSignal<Format>,
    pub error: RwSignal<Option<String>>,
    pub path: RwSignal<Option<String>>,
    /// The library row the open named, when it named one.
    pub book_id: RwSignal<Option<String>>,
    pub title: RwSignal<Option<String>>,
    pub author: RwSignal<Option<String>>,
    /// Pages the reader is navigating; both pipelines publish it.
    pub num_pages: RwSignal<u32>,
    /// The chapter tree behind a shared handle; Leptos clones per reader.
    pub outline: RwSignal<Arc<Vec<OutlineNode>>>,
    /// True while the lazy outline resolution is in flight.
    pub outline_pending: RwSignal<bool>,
    /// The pages, per format. Exactly one half belongs to the open document.
    pub content: DocumentContent,
}

/// The pages of the open document: one half belongs to the open file.
#[derive(Clone, Copy, Default)]
pub struct DocumentContent {
    /// Page sizes at scale 1 and as laid out; both pipelines fill these.
    pub metrics: PageMetrics,
    /// The reflowable pipeline's blocks, heights and current page cut.
    pub reflow: ReflowContent,
}

/// What a re-cut tells the document: the count, the size, the landing
/// page.
#[derive(Clone, Debug, PartialEq)]
pub struct ReflowCut {
    /// Pages the cut produced.
    pub num_pages: u32,
    /// Intrinsic (scale-1) size of every page.
    pub page_size: PageSize,
    /// Laid-out CSS-px height of every page, at the scale the cut was made at.
    pub css_height: f64,
    /// The page the reader lands on: the PREVIOUS cut's current page.
    pub page: u32,
}

impl Default for DocumentState {
    fn default() -> Self {
        Self {
            status: RwSignal::new(DocStatus::Idle),
            format: RwSignal::new(Format::default()),
            error: RwSignal::new(None),
            path: RwSignal::new(None),
            book_id: RwSignal::new(None),
            title: RwSignal::new(None),
            author: RwSignal::new(None),
            num_pages: RwSignal::new(0),
            outline: RwSignal::new(Arc::new(Vec::new())),
            outline_pending: RwSignal::new(false),
            content: DocumentContent::default(),
        }
    }
}

impl DocumentState {
    /// Height-over-width aspect of page 1, with one fallback policy.
    pub fn page1_aspect(&self) -> f64 {
        page_aspect(self.content.metrics.page1_size.get())
    }

    /// Same, read untracked — for rAF/scroll callbacks that must not
    /// subscribe to geometry.
    pub fn page1_aspect_now(&self) -> f64 {
        page_aspect(
            self.content
                .metrics
                .page1_size
                .try_get_untracked()
                .flatten(),
        )
    }

    /// The document's name: usable title, else file stem, else "No
    /// document".
    pub fn display_name(&self) -> String {
        reader_core::filename::display_name(self.title.get().as_deref(), self.path.get().as_deref())
            .unwrap_or_else(|| NO_DOCUMENT.to_string())
    }

    /// File the engine's flattened entries into the reader's outline, with
    /// the page-count clamp.
    pub fn set_pdf_outline(&self, entries: Vec<OutlineEntry>, page_count: u32) {
        self.outline
            .set(Arc::new(pdf_core::outline::to_nodes(entries, page_count)));
    }

    /// Publish a reflowable cut's count and sizes, exactly as a PDF feeds
    /// them.
    pub fn publish_cut(&self, cut: &ReflowCut) {
        self.num_pages.set(cut.num_pages);
        self.content
            .metrics
            .publish_uniform(cut.num_pages, &cut.page_size, cut.css_height);
    }
}

// The 3:4 aspect lives in `runtime-contract`, which both runtimes may
// import.
pub use runtime_contract::covers::DEFAULT_PAGE_ASPECT;

/// Name shown when there is neither a title nor a path.
pub const NO_DOCUMENT: &str = "No document";

/// Height-over-width aspect, falling back when unmeasured or degenerate.
fn page_aspect(size: Option<PageSize>) -> f64 {
    match size {
        Some(s) if s.width > 0.0 => s.height / s.width,
        _ => DEFAULT_PAGE_ASPECT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_aspect_passes_through_measured_sizes() {
        // US Letter at scale 1: 792/612 ≈ 1.294.
        assert!(
            (page_aspect(Some(PageSize {
                width: 612.0,
                height: 792.0
            })) - 792.0 / 612.0)
                .abs()
                < 1e-12
        );
        // A landscape sheet inverts below 1.
        assert!(
            page_aspect(Some(PageSize {
                width: 1000.0,
                height: 500.0
            })) < 1.0
        );
    }

    #[test]
    fn page_aspect_falls_back_to_portrait_when_unmeasured_or_degenerate() {
        assert_eq!(page_aspect(None), DEFAULT_PAGE_ASPECT);
        assert_eq!(
            page_aspect(Some(PageSize {
                width: 0.0,
                height: 792.0
            })),
            DEFAULT_PAGE_ASPECT
        );
        // A negative width is just as degenerate: never divide by it.
        assert_eq!(
            page_aspect(Some(PageSize {
                width: -612.0,
                height: 792.0
            })),
            DEFAULT_PAGE_ASPECT
        );
    }
}
