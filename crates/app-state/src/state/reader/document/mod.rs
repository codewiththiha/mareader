//! The open document: what it is, which pipeline it renders through, how big
//! its pages are, and the one fallback policy every fixed-geometry surface
//! shares.
//!
//! The struct is three groups, named for what they hold rather than for the
//! format they came from:
//!
//! * the identity — path, the library row it is, title, author, the format,
//!   the load status;
//! * the outline — the chapter tree, whatever produced it;
//! * the content — [`page_metrics::PageMetrics`] (the page sizes BOTH pipelines
//!   publish) and [`reflow::ReflowContent`] (the blocks, heights and page cut
//!   that only a reflowable document has).
//!
//! The page sizes are named for what they measure rather than for the format
//! that used to own them, because a reflowable document publishes A4 into
//! exactly the same fields, through [`DocumentState::publish_cut`]. Each leaf
//! stayed its own signal on purpose: one payload enum inside one signal would
//! notify every reader of the document on every geometry write, including the
//! ones that only wanted its title. The format is already the tag that says
//! which half is live; a second one in the state could only disagree with it.
//!
//! A field added here is reset by [`DocumentState::reset`] and by nothing else,
//! which is the invariant that keeps a close from leaking the last book.

pub mod page_metrics;
pub mod reflow;

use std::sync::Arc;

use leptos::prelude::*;

use pdf_core::outline::OutlineEntry;
use pdf_engine::types::{DocStatus, PageSize};
use reader_core::format::Format;
use reader_core::outline::OutlineNode;

pub use page_metrics::PageMetrics;
pub use reflow::ReflowContent;

#[derive(Clone, Copy)]
pub struct DocumentState {
    pub status: RwSignal<DocStatus>,
    /// Which pipeline the open document renders through. PDF while nothing
    /// is open (the historical default), so chrome that branches on it has
    /// a sane answer on the empty shelf.
    pub format: RwSignal<Format>,
    pub error: RwSignal<Option<String>>,
    pub path: RwSignal<Option<String>>,
    /// The library row the reader opened, when the open named one — a card, a
    /// list row, the context menu's Open all carry an id, while a drop, an
    /// "open with" and a dialog carry nothing but an address.
    ///
    /// The address cannot tell two rows of one file apart, and the library
    /// can hold two. Every reader of a resume point and every writer of a
    /// highlight asks which row this is rather than guessing from the path —
    /// see `reader_runtime::services::document::gloss_key` and
    /// [`library_core::book::rows_for_read`].
    pub book_id: RwSignal<Option<String>>,
    pub title: RwSignal<Option<String>>,
    pub author: RwSignal<Option<String>>,
    /// How many pages the reader is currently navigating. Both pipelines
    /// publish it, because every surface that shows or clamps a page reads
    /// this and none of them may care who counted.
    pub num_pages: RwSignal<u32>,
    /// The document's flattened chapter tree, behind a shared handle. `Arc`
    /// rather than a plain `Vec` because Leptos hands every reader its own
    /// clone of a signal's value, and the panel reads the list on every page
    /// turn: cloning the handle is a refcount bump, cloning the list was
    /// several hundred allocations per notify.
    pub outline: RwSignal<Arc<Vec<OutlineNode>>>,
    /// True while the (lazy) outline resolution is in flight — the panel
    /// shows "resolving" instead of a definitive "No outline" for a book
    /// whose chapters are merely not back yet.
    pub outline_pending: RwSignal<bool>,
    /// The pages, per format. Exactly one half belongs to the open document.
    pub content: DocumentContent,
}

/// The pages of the open document: two grouped signals, not one enum (see
/// the module docs for why the halves stay separate signals).
#[derive(Clone, Copy, Default)]
pub struct DocumentContent {
    /// Page sizes at scale 1 and as laid out. Shared: a PDF fills these from
    /// the file, a reflowable document from its page cut.
    pub metrics: PageMetrics,
    /// The reflowable pipeline's blocks, heights and current page cut.
    pub reflow: ReflowContent,
}

impl DocumentContent {
    /// Back to "no document" for both pipelines. A format's content is
    /// released whenever the OTHER format takes the reader over, which is why
    /// the two halves reset together rather than at their own open.
    pub fn reset(&self) {
        self.metrics.reset();
        self.reflow.reset();
    }
}

/// What a reflowable re-cut tells the document: how many pages it now has,
/// how big each one is, and which page the reader lands on.
///
/// Every page is the same size — A4 is the cut's one fixed point — so this
/// carries ONE size and ONE height rather than two vectors of a repeated
/// value, and [`DocumentState::publish_cut`] expands them. The page count is
/// the cut's, not the reader's: a cut that changes the count must say so
/// before anything clamps a page against it.
#[derive(Clone, Debug, PartialEq)]
pub struct ReflowCut {
    /// Pages the cut produced.
    pub num_pages: u32,
    /// Intrinsic (scale-1) size of every page.
    pub page_size: PageSize,
    /// Laid-out CSS-px height of every page, at the scale the cut was made at.
    pub css_height: f64,
    /// The page the reader lands on: the one holding the block the PREVIOUS
    /// cut's current page started on, so a re-cut never strands the position.
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
    /// Back to the no-document state. Every field the open flow writes is
    /// reset here, so a field added to the struct cannot be silently
    /// forgotten by close_document.
    ///
    /// The handles are `Copy`, so this binds the signals the struct already
    /// holds; `Self::default()` would allocate a fresh arena node per field on
    /// every close and leak them.
    pub fn reset(&self) {
        let Self {
            status,
            format,
            error,
            path,
            book_id,
            title,
            author,
            num_pages,
            outline,
            outline_pending,
            content,
        } = *self;
        status.set(DocStatus::Idle);
        format.set(Format::default());
        error.set(None);
        path.set(None);
        book_id.set(None);
        title.set(None);
        author.set(None);
        num_pages.set(0);
        outline.set(Arc::new(Vec::new()));
        outline_pending.set(false);
        content.reset();
    }

    /// Height-over-width aspect of page 1 (tracked read). Every
    /// fixed-geometry surface that sizes itself against the first sheet goes
    /// through here, so the fallback policy lives in exactly one place.
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

    /// The document's human-facing name (tracked read): its usable title,
    /// else the file stem, else "No document". The three surfaces that show
    /// the name used to hand-roll this with three different fallbacks; the
    /// policy lives here.
    pub fn display_name(&self) -> String {
        reader_core::filename::display_name(self.title.get().as_deref(), self.path.get().as_deref())
            .unwrap_or_else(|| NO_DOCUMENT.to_string())
    }

    /// File the engine's flattened outline entries into the reader's outline.
    /// The PDF pipeline's last format-specific act: from here the panel
    /// cannot tell these chapters from the ones `md_core` derives, and the
    /// `page_count` clamp stops an outline authored against a re-saved file
    /// from jumping past the last sheet.
    pub fn set_pdf_outline(&self, entries: Vec<OutlineEntry>, page_count: u32) {
        self.outline
            .set(Arc::new(pdf_core::outline::to_nodes(entries, page_count)));
    }

    /// Publish a reflowable cut to the shared page machinery: the page count
    /// and per-page sizes, fed exactly as a PDF feeds them.
    ///
    /// The one place the two pipelines meet, and what lets the paged modes,
    /// the zoom ladder and the progress chrome never ask which format is
    /// open. The reflow half decides the numbers
    /// ([`reflow::ReflowContent::recut`]) and the document writes them,
    /// because they are the document's fields.
    pub fn publish_cut(&self, cut: &ReflowCut) {
        self.num_pages.set(cut.num_pages);
        self.content
            .metrics
            .publish_uniform(cut.num_pages, &cut.page_size, cut.css_height);
    }
}

/// Aspect used while page 1 is unmeasured or degenerate: a 3:4 portrait, the
/// default every fixed-geometry surface historically fell back to.
pub const DEFAULT_PAGE_ASPECT: f64 = 0.75;

/// Name shown when a document has neither a usable title nor a path (the
/// reader shell with nothing open).
pub const NO_DOCUMENT: &str = "No document";

/// Height-over-width aspect of a page size, falling back to
/// [`DEFAULT_PAGE_ASPECT`] when the size is missing or its width is not
/// positive — dividing by a zero-width sheet would poison every height
/// derived from it.
pub(crate) fn page_aspect(size: Option<PageSize>) -> f64 {
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

    #[test]
    fn a_reset_releases_both_halves() {
        // One reset, both pipelines: a close cannot leave the previous
        // book's page sizes behind while the next document is measured.
        let state = DocumentState::default();
        state.book_id.set(Some("b1".to_string()));
        state.content.metrics.css_heights.set(vec![792.0]);
        state.content.reflow.heights.set(Arc::new(vec![40.0]));
        state.num_pages.set(3);
        state.reset();
        assert!(state.content.metrics.css_heights.get_untracked().is_empty());
        assert!(state.content.reflow.heights.get_untracked().is_empty());
        assert_eq!(state.num_pages.get_untracked(), 0);
        assert_eq!(
            state.book_id.get_untracked(),
            None,
            "a close cannot leave the last book's row named"
        );
        assert!(state.content.metrics.page1_size.get_untracked().is_none());
    }
}
