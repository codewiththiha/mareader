//! The open handshake both pipelines share.
//!
//! A PDF and a reflowable document are opened by different tails — one waits
//! on the engine, the other on a file read and a parser — but they end in the
//! same place: the same identity fields written, the same gloss marks loaded,
//! the same resume clamp, startup scale, route flip and shelf record. Those
//! steps live here once.
//!
//! A contract, not a convenience: the tails used to spell the handshake out
//! separately and drifted — loading gloss marks at open (so the first painted
//! page carries them) was added to the PDF seed and had to be added to the
//! reflow tail by hand. A step added here reaches every format at once.
//!
//! Deliberately NOT here: the order in which a tail seeds its own content.
//! Both tails depend on an order only they can see — heights published at the
//! seed scale, the anchor guard up before the page is written, the route
//! flipping last — and a shared function would hide the sequence that makes it
//! correct.

use std::sync::Arc;

use leptos::prelude::*;

use ai_core::gloss::GlossMark;
use pdf_engine::types::{DocStatus, PageSize};
use reader_core::format::Format;
use reader_core::outline::OutlineNode;
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;

use crate::state::AppState;
use crate::zoom::target::FitDims;

/// Which document is open, in the fields both formats have. `outline` is
/// `None` for a format whose chapter tree resolves after the open (a PDF's
/// comes from the engine, asynchronously); a format that already has its
/// headings passes the empty tree and says so, which clears `outline_pending`
/// without a second race to lose.
pub(super) struct DocumentIdentity {
    pub format: Format,
    pub path: String,
    pub title: Option<String>,
    pub author: Option<String>,
    /// The size every fixed-geometry surface uses before a page has rendered.
    /// A PDF's comes from the file; a reflowable document's is the A4 sheet it
    /// is cut into.
    pub page1_size: PageSize,
    pub outline: Option<Arc<Vec<OutlineNode>>>,
}

/// Write the document's identity.
///
/// The format flips here rather than in the tails: a PDF opening over a text
/// document has to shed the reflowable gates (blend, thumbnails, the Fonts
/// tab) in the same breath a text open claims them, and one write is the only
/// way the two cannot disagree about when that happens.
///
/// The previous book's chapters are cleared with the identity — a mid-read open
/// never passes through `close_document`'s reset, so the old tree would
/// otherwise show while the new one resolves.
pub(super) fn identity(state: AppState, doc: DocumentIdentity) {
    let document = &state.reader.document;
    document.format.set(doc.format);
    document.path.set(Some(doc.path));
    // The title the reader's surfaces show. The document's own when it
    // supplied one worth showing; otherwise the LIBRARY's name for the book
    // this open belongs to, when it belongs to one — and that name is never
    // the address's stem for a stored book, because the address a copy opens
    // at is the store's own `source.pdf`, a layout artifact rather than a
    // name (`library_core::book::Book::title` falls back to the source the
    // bytes came from). A title bar reading "source" for every book the
    // library copied is the sentence this write exists to prevent.
    let title = match doc.title.as_deref().map(str::trim) {
        Some(own) if reader_core::filename::is_usable_title(own) => doc.title.clone(),
        _ => state
            .reader
            .document
            .book_id
            .get_untracked()
            .and_then(|id| state.library.row(&id))
            .map(|row| row.display_name())
            .or(doc.title.clone()),
    };
    document.title.set(title);
    document.author.set(doc.author);
    document.outline.set(doc.outline.clone().unwrap_or_else(|| Arc::new(Vec::new())));
    document.outline_pending.set(doc.outline.is_none());
    document.content.metrics.page1_size.set(Some(doc.page1_size));
}

/// This document's gloss highlights, into a freshly reset gloss state. Loaded
/// during the open rather than lazily by the mark layer so the very first page
/// mount already paints them — page-space rects for a PDF, a block and
/// character range for a reflowable document, which the page cut makes
/// projectable. `reset` runs first so a field added to the gloss state cannot
/// be missed here.
///
/// Which list "this document's" is, is [`crate::services::document::gloss_key`]'s
/// answer: the id of the row the library holds for it. Two rows of one file are
/// two lists, and a book of its own reads the marks its own reader made — which
/// falls out of the key being an id rather than out of a rule about privacy.
///
/// There is deliberately no fallback to the address. Marks are keyed by row id
/// and the storage migrates an address-keyed list onto the row that was reading
/// it (`crate::storage::migrate_gloss_keys`), so an address read here would only
/// ever find a list the migration had already claimed — and finding it would put
/// one file's marks on a different book at the same address. An open with no row
/// to name has no marks to load, which is the honest answer rather than a guess.
pub(super) fn load_marks(state: AppState) {
    state.reader.gloss.reset();
    let key = crate::services::document::gloss_key(state);
    if key.is_empty() {
        return;
    }
    let marks: Vec<GlossMark> = crate::storage::load_gloss()
        .remove(&key)
        .unwrap_or_default();
    state.reader.gloss.marks.set(marks);
}

/// The page to resume at, clamped to the book that actually opened. Both
/// bounds are real: a re-edited document may have fewer pages than the shelf
/// remembers, and a stale or transient saved 0 must never resume before the
/// book.
pub(super) fn resume_page(saved_page: u32, num_pages: u32) -> u32 {
    saved_page.clamp(1, num_pages.max(1))
}

/// The scale to seed a fresh document at, and the fit mode it belongs to.
///
/// Resolved through the same geometry the first live refit will use, so the
/// first frame already sits where the fit is going to land instead of jumping
/// to it a moment later. `page_size` is the sheet being fitted: page 1's for a
/// PDF, the dialled card (A4 unless the column-width dial grew it) for a
/// reflowable document.
///
/// A document opening straight into the continuous stream is the exception —
/// and the reason this returns the fit mode, not only the scale: there is no
/// page to fit, the window IS the page, type size belongs to the typography
/// settings, and the zoom starts at 1 with no fit to remember.
pub(super) fn startup_scale(state: AppState, page_size: (f64, f64)) -> (FitMode, f64) {
    let streaming = state.reader.viewer.mode.get_untracked() == ViewMode::ScrollVertical;
    // The startup fit mode is a user setting (Fit Page / Fit Width), not a
    // hard-coded fit-width, and `sanitize` has already replaced a persisted
    // `None` with the default — so this is always a real fit mode here.
    let startup_fit = if streaming {
        FitMode::None
    } else {
        state.settings.with_untracked(|s| s.layout.default_fit)
    };
    let scale = if streaming {
        1.0
    } else {
        // The container CANNOT be asked at seed time: `container_size` is
        // what the mounted scroller reports and nothing is mounted yet —
        // seeding from it fits the first page against the previous document's
        // box, and the post-mount refit then commits a zoom over the first
        // renders. The window is alive already, so measure IT: the title bar
        // overlays the content, so only a DOCKED rail gives width up (`w-72`,
        // border-box), and the fit maths gets the same container the mounted
        // viewer will report a moment later — which turns the post-mount refit
        // into a no-op.
        //
        // The column-width dial is a reflowable-only setting and stays out
        // of this budget: a reflowable page box already carries it through
        // the geometry it was cut with, and a PDF page IS the column — the
        // same contract the live fit maths holds (`crate::zoom::target`).
        // The format is settled by the identity step that runs before this
        // one.
        const DOCKED_RAIL_W: f64 = 288.0;
        let (vw, vh) = app_chrome::hooks::use_viewport::viewport_size();
        let docked = !state.settings.with_untracked(|s| s.layout.sidebar_overlay)
            && state.ui.sidebar.get_untracked() != crate::state::SidebarMode::None;
        let cw = vw - if docked { DOCKED_RAIL_W } else { 0.0 };
        FitDims::from_geometry(
            state.reader.viewer.mode.get_untracked(),
            (cw.max(1.0), vh.max(1.0)),
            state.reader.viewer.page_margin.get_untracked(),
            page_size,
        )
        .map_or(1.0, |dims| dims.fit(startup_fit, 1.0))
    };
    (startup_fit, scale)
}

/// The document is open: flip the route. LAST, and after every signal the
/// fresh mount reads is in its new-document state — `status = Ready` is what
/// mounts the reader, so anything written after this is written under a live
/// view. A successful open also dismisses a stale error toast and drops the
/// previous document's search: the floating overlay and its highlights belong
/// to the book that was open and must not linger into this one.
pub(super) fn enter_ready(state: AppState) {
    state.reader.document.error.set(None);
    state.reader.document.status.set(DocStatus::Ready);
    state.ui.toast.set(None);
    state.reader.search.reset();
}
