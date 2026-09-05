//! The open handshake both pipelines share.
//!
//! A PDF and a reflowable document are opened by different tails — one waits on
//! the engine, the other on a file read and a parser — but they end in the same
//! place: the same identity fields written, the same gloss marks loaded, the
//! same resume clamp, the same startup scale, the same route flip and the same
//! shelf record. Those steps live here once.
//!
//! This is a contract, not a convenience. The two tails used to spell the
//! handshake out separately, and they drifted: loading a document's gloss marks
//! at open (so the first painted page already carries them) was added to the PDF
//! seed and had to be added to the reflowable tail by hand afterwards. A step
//! added here reaches every format at once; a step added to one tail does not.
//!
//! What is deliberately NOT here: the order in which a tail seeds its own
//! content. Both tails depend on an order that only they can see — heights are
//! published at the seed scale, the anchor guard goes up before the page is
//! written, the route flips last — and a shared function would hide the very
//! sequence that makes it correct.

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

/// Which document is open, in the fields both formats have.
///
/// `outline` is `None` for a format whose chapter tree is resolved after the
/// open (a PDF's comes from the engine, asynchronously); a format that already
/// has its headings in hand passes the empty tree and says so, which is what
/// clears `outline_pending` without a second race to lose.
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
/// document has to shed the reflowable gates (blend, thumbnails, the Fonts tab)
/// in the same breath that a text open claims them, and one write is the only
/// way the two cannot disagree about when that happens.
///
/// The previous book's chapters are cleared with the identity — a mid-read open
/// never passes through `close_document`'s reset, so the old tree would
/// otherwise be showing while the new one resolves.
pub(super) fn identity(state: AppState, doc: DocumentIdentity) {
    let document = &state.reader.document;
    document.format.set(doc.format);
    document.path.set(Some(doc.path));
    document.title.set(doc.title);
    document.author.set(doc.author);
    document.outline.set(doc.outline.clone().unwrap_or_else(|| Arc::new(Vec::new())));
    document.outline_pending.set(doc.outline.is_none());
    document.content.metrics.page1_size.set(Some(doc.page1_size));
}

/// This document's gloss highlights, into a freshly reset gloss state.
///
/// Loaded during the open rather than lazily by the mark layer so the very
/// first page mount already paints them — for a PDF they are page-space rects,
/// for a reflowable document a block and a character range that the page cut
/// makes projectable. `reset` runs first so a field added to the gloss state
/// cannot be missed here; the loaded marks then overwrite the empty list.
pub(super) fn load_marks(state: AppState, path: &str) {
    state.reader.gloss.reset();
    let marks: Vec<GlossMark> = crate::storage::load_gloss()
        .remove(path)
        .unwrap_or_default();
    state.reader.gloss.marks.set(marks);
}

/// The page to resume at, clamped to the book that actually opened.
///
/// Both bounds are real: a re-edited document may have fewer pages than the
/// shelf remembers, and a stale or transient saved 0 must never resume before
/// the book.
pub(super) fn resume_page(saved_page: u32, num_pages: u32) -> u32 {
    saved_page.clamp(1, num_pages.max(1))
}

/// The scale to seed a fresh document at, and the fit mode it belongs to.
///
/// Resolved through the same geometry the first live refit will use, so the
/// first frame already sits where the fit is going to land rather than jumping
/// to it a moment later. `page_size` is the sheet being fitted: page 1's for a
/// PDF, the A4 constant for a reflowable document.
///
/// A document that opens straight into the continuous stream is the exception,
/// and the reason this returns the fit mode instead of only the scale: there is
/// no page to fit, the window IS the page, type size belongs to the typography
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
        FitDims::from_geometry(
            state.reader.viewer.mode.get_untracked(),
            state.reader.viewer.container_size.get_untracked(),
            state.reader.viewer.page_margin.get_untracked(),
            page_size,
        )
        .map_or(1.0, |dims| dims.fit(startup_fit, 1.0))
    };
    (startup_fit, scale)
}

/// The document is open: flip the route.
///
/// LAST, and after every signal the fresh mount reads is already in its
/// new-document state — `status = Ready` is what mounts the reader, so anything
/// written after this is written under a live view. A successful open also
/// dismisses a stale error toast and drops the previous document's search: the
/// floating search overlay and its highlights belong to the book that was open,
/// and neither may linger into this one.
pub(super) fn enter_ready(state: AppState) {
    state.reader.document.error.set(None);
    state.reader.document.status.set(DocStatus::Ready);
    state.ui.toast.set(None);
    state.reader.search.reset();
}
