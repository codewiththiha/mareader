//! The open document: what it is, how big its pages are, and the one
//! fallback policy every fixed-geometry surface shares.

use std::sync::Arc;

use leptos::prelude::*;

use pdf_engine::types::{DocStatus, OutlineNode, PageSize};

#[derive(Clone, Copy)]
pub struct DocumentState {
    pub status: RwSignal<DocStatus>,
    pub error: RwSignal<Option<String>>,
    pub path: RwSignal<Option<String>>,
    pub num_pages: RwSignal<u32>,
    pub title: RwSignal<Option<String>>,
    pub author: RwSignal<Option<String>>,
    /// The document's flattened chapter tree, behind a shared handle.
    ///
    /// `Arc` rather than a plain `Vec` because Leptos hands every reader its
    /// own clone of a signal's value: a textbook outline is several hundred
    /// `OutlineNode`s, each with an owned `String` title, and the panel reads
    /// the list on every page turn (the active-entry memo, the reveal effect,
    /// the row list) as well as the floating label. Cloning the handle is a
    /// refcount bump; cloning the list was several hundred allocations per
    /// notify.
    pub outline: RwSignal<Arc<Vec<OutlineNode>>>,
    /// True while the (lazy) outline resolution is in flight — the panel
    /// shows "resolving" instead of a definitive "No outline" for a book
    /// whose chapters are merely not back yet.
    pub outline_pending: RwSignal<bool>,
    /// CSS-px size of page 1 at scale 1 (used for fit modes before any render).
    pub page1_size: RwSignal<Option<PageSize>>,
    /// Intrinsic + laid-out page geometry (one source of truth).
    pub metrics: PageMetrics,
}

impl DocumentState {
    /// Back to the no-document state. Every field the open flow writes is
    /// reset here, so a field added to the struct cannot be silently
    /// forgotten by close_document.
    pub fn reset(&self) {
        self.status.set(DocStatus::Idle);
        self.error.set(None);
        self.path.set(None);
        self.num_pages.set(0);
        self.title.set(None);
        self.author.set(None);
        self.outline.set(Arc::new(Vec::new()));
        self.outline_pending.set(false);
        self.page1_size.set(None);
        self.metrics.reset();
    }

    /// Height-over-width aspect of page 1 (tracked read: subscribes the
    /// caller to `page1_size`). Every fixed-geometry surface that sizes
    /// itself against the first sheet — the thumbnail grid's row height,
    /// the auto-center target — goes through here, so the fallback policy
    /// lives in exactly one place.
    pub fn page1_aspect(&self) -> f64 {
        page_aspect(self.page1_size.get())
    }

    /// Same, read untracked — for rAF/scroll callbacks that must not
    /// subscribe to geometry.
    pub fn page1_aspect_untracked(&self) -> f64 {
        page_aspect(self.page1_size.get_untracked())
    }

    /// The document's human-facing name (tracked read: subscribes the
    /// caller to title and path): its usable title, else the file stem,
    /// else "No document". The three surfaces that show the name — the
    /// toolbar title, the sidebar's document card, the floating label —
    /// used to each hand-roll this with three different fallbacks; the
    /// policy lives here now.
    pub fn display_name(&self) -> String {
        pdf_core::filename::display_name(
            self.title.get().as_deref(),
            self.path.get().as_deref(),
        )
        .unwrap_or_else(|| NO_DOCUMENT.to_string())
    }
}

/// Aspect used while page 1 is unmeasured or degenerate: a 3:4 portrait,
/// the default every fixed-geometry surface historically fell back to.
pub const DEFAULT_PAGE_ASPECT: f64 = 0.75;

/// Name shown when a document has neither a usable title nor a path (the
/// reader shell with nothing open).
pub const NO_DOCUMENT: &str = "No document";

/// Height-over-width aspect of a page size, falling back to
/// [`DEFAULT_PAGE_ASPECT`] when the size is missing or its width is not
/// positive (a zero-width sheet has no meaningful aspect, and dividing by
/// it would poison every height derived from it).
pub(crate) fn page_aspect(size: Option<PageSize>) -> f64 {
    match size {
        Some(s) if s.width > 0.0 => s.height / s.width,
        _ => DEFAULT_PAGE_ASPECT,
    }
}

/// Packed page geometry: one `PageSize` per page plus the CSS-px column.
#[derive(Clone, Copy)]
pub struct PageMetrics {
    /// Intrinsic (scale-1) width/height of every page, 0-based.
    pub intrinsic: RwSignal<Vec<PageSize>>,
    /// Rendered CSS-px heights per page, seeded from `intrinsic` and refined
    /// by `on_geometry` as pages actually render.
    pub css_heights: RwSignal<Vec<f64>>,
}

impl PageMetrics {
    pub fn reset(&self) {
        self.intrinsic.set(Vec::new());
        self.css_heights.set(Vec::new());
    }
}

impl Default for PageMetrics {
    fn default() -> Self {
        Self {
            intrinsic: RwSignal::new(Vec::new()),
            css_heights: RwSignal::new(Vec::new()),
        }
    }
}

impl Default for DocumentState {
    fn default() -> Self {
        Self {
            status: RwSignal::new(DocStatus::Idle),
            error: RwSignal::new(None),
            path: RwSignal::new(None),
            num_pages: RwSignal::new(0),
            title: RwSignal::new(None),
            author: RwSignal::new(None),
            outline: RwSignal::new(Arc::new(Vec::new())),
            outline_pending: RwSignal::new(false),
            page1_size: RwSignal::new(None),
            metrics: PageMetrics::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_aspect_passes_through_measured_sizes() {
        // US Letter at scale 1: 792/612 ≈ 1.294.
        assert!((page_aspect(Some(PageSize { width: 612.0, height: 792.0 })) - 792.0 / 612.0).abs() < 1e-12);
        // A landscape sheet inverts below 1.
        assert!(page_aspect(Some(PageSize { width: 1000.0, height: 500.0 })) < 1.0);
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
