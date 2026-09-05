//! Seeding the app state for a freshly opened document.
//!
//! One synchronous batch, in a deliberate order, run while the status is
//! still `Opening` and nothing is mounted. The order is the interesting part
//! and each step says why it is where it is.

use leptos::prelude::*;

use reader_core::format::Format;
use reader_core::filename::display_name;
use pdf_engine::types::{OpenResult, PageSize};

use super::enter;
use crate::state::AppState;

/// What the rest of the flow needs to know once the state is seeded.
pub(super) struct Seeded {
    /// The document's display name, for the shelf record.
    pub name: Option<String>,
    /// The page to resume at, clamped to the book that actually opened.
    pub resume: u32,
    pub num_pages: u32,
}

/// Write everything the fresh mount will read, the resume page included;
/// the strip anchors itself to it on mount.
pub(super) fn seed(state: AppState, path: &str, open: OpenResult, saved_page: u32) -> Seeded {
    let page1 = open.page1_size;
    let num_pages = open.num_pages;
    let name = display_name(open.title.as_deref(), Some(path));

    // Document identity, through the step both open tails share (see
    // [`super::enter`]). The format flips BACK here: a PDF opening over a text
    // document must shed the reflowable gates (blend, thumbnails, the Fonts
    // tab) the same way a text open claims them. The chapter tree is `None`
    // because a PDF's outline is resolved AFTER the open, and that is what
    // keeps `outline_pending` true (see `super::outline`).
    enter::identity(
        state,
        enter::DocumentIdentity {
            format: Format::Pdf,
            path: path.to_string(),
            title: open.title,
            author: open.author,
            page1_size: page1.clone(),
            outline: None,
        },
    );

    // A text document's model must not survive the PDF that opens over it
    // (the measure column mounts while `reflow.blocks` is a document).
    state.reader.document.content.reflow.reset();
    state.reader.document.num_pages.set(num_pages);
    // The paper session resets for the new book — synchronously, while the
    // status is still `Opening` and nothing is mounted, so the previous
    // book's backdrop colour is gone before the reader's first frame.
    pdf_engine::paper::document_open(path, num_pages);
    state
        .reader
        .document
        .content
        .metrics
        .intrinsic
        .set(intrinsic_sizes(&open.page_widths, &open.page_heights, &page1, num_pages));

    // Gloss highlights for THIS document, loaded where the reflowable tail
    // loads them: before anything mounts, so the first painted page already
    // carries them. For a PDF they are page-space rects, not DOM state.
    enter::load_marks(state, path);

    let resume = enter::resume_page(saved_page, num_pages);

    // The reading position is authored HERE, once, and the strip anchors
    // itself to it when it mounts (`ScrollShell`). Until that anchor has
    // landed the strip's own dominant page is whatever offset it last held,
    // so the scroll→page sync is told to stand down first — before the page
    // is written, so no effect can ever observe the new page against the
    // old strip. Every other reader of `page` (the indicator, reading
    // progress, the thumbnails) simply sees the resume point from the start;
    // nothing passes through a transient page 1 any more.
    //
    // ALL of this lands BEFORE `status = Ready` flips the route to the
    // reader, so the fresh mount reads a fully seeded state.
    state.reader.viewer.awaiting_anchor.set(true);
    state.reader.viewer.page.set(resume);
    state.reader.viewer.scroll_top.set(0.0);
    // Heights belong to the document that was just closed; leaving them would
    // have the zoom coordinator anchor against a stale column on the first
    // gesture. ReaderPage re-seeds them from the intrinsic page sizes at the
    // current scale.
    state.reader.document.content.metrics.css_heights.set(Vec::new());
    // The seed scale comes from the step both open tails share: the same
    // geometry the first live refit will use, including the stream exception
    // that has no page to fit.
    let (startup_fit, scale) = enter::startup_scale(state, (page1.width, page1.height));
    state.reader.viewer.fit.set(startup_fit);
    // Seeding the zoom state is correct HERE and nowhere else: this is the
    // initial scale for a brand-new document, so there is no layout to
    // animate from and nothing to anchor to. All three scales start in
    // agreement, with no transition in flight.
    state.reader.viewer.zoom.initialize(scale);

    Seeded {
        name,
        resume,
        num_pages,
    }
}

/// Intrinsic (scale-1) size of every page, packed one `PageSize` each.
///
/// The engine sends the widths and heights as two parallel arrays; a book
/// whose arrays do not both match the page count is not trustworthy per-page,
/// so every page falls back to page 1's size rather than being read off by
/// one.
fn intrinsic_sizes(
    widths: &[f64],
    heights: &[f64],
    page1: &PageSize,
    num_pages: u32,
) -> Vec<PageSize> {
    let n = num_pages as usize;
    if widths.len() == n && heights.len() == n {
        widths
            .iter()
            .zip(heights.iter())
            .map(|(&width, &height)| PageSize { width, height })
            .collect()
    } else {
        vec![page1.clone(); n]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(w: f64, h: f64) -> PageSize {
        PageSize {
            width: w,
            height: h,
        }
    }

    #[test]
    fn per_page_sizes_are_used_when_both_arrays_match_the_book() {
        let sizes = intrinsic_sizes(&[10.0, 20.0], &[100.0, 200.0], &size(1.0, 1.0), 2);
        assert_eq!(sizes.len(), 2);
        assert_eq!(sizes[1].width, 20.0);
        assert_eq!(sizes[1].height, 200.0);
    }

    #[test]
    fn a_mismatched_array_falls_back_to_page_one_for_every_page() {
        // Reading a short array off by one would give later pages the wrong
        // geometry, which the virtualizer would then lay out against.
        let sizes = intrinsic_sizes(&[10.0], &[100.0, 200.0], &size(612.0, 792.0), 2);
        assert_eq!(sizes.len(), 2);
        assert!(sizes.iter().all(|s| s.width == 612.0 && s.height == 792.0));
    }

    #[test]
    fn a_book_with_no_pages_has_no_sizes() {
        assert!(intrinsic_sizes(&[], &[], &size(612.0, 792.0), 0).is_empty());
    }
}
