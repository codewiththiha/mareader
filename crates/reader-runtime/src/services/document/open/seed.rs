//! Seeding the app state for a freshly opened document, in a
//! deliberate order.

use leptos::prelude::*;

use pdf_engine::types::{OpenResult, PageSize};
use reader_core::format::Format;

use super::enter;

/// What the rest of the flow needs to know once the state is seeded.
pub(super) struct Seeded {
    /// The page to resume at, clamped to the book that actually opened.
    pub resume: u32,
    pub num_pages: u32,
}

/// Write everything the fresh mount reads, the resume page included.
pub(super) fn seed(
    state: &crate::context::ReaderContext,
    path: &str,
    open: OpenResult,
    saved_page: u32,
) -> Seeded {
    let page1 = open.page1_size;
    let num_pages = open.num_pages;

    // Identity through the shared step; the format flips BACK here.
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

    // A text document's blocks must not survive the PDF that opens over
    // it.
    state.reader.document.content.reflow.reset();
    state.reader.document.num_pages.set(num_pages);
    // Configure the new paper session, then open it.
    crate::effects::reader::blend_backdrop::configure_session(state);
    state.pane.pdf().paper_document_open(path, num_pages);
    state.reader.document.content.metrics.clear_rendered();
    state
        .reader
        .document
        .content
        .metrics
        .intrinsic
        .set(intrinsic_sizes(
            &open.page_widths,
            &open.page_heights,
            &page1,
            num_pages,
        ));

    // Gloss highlights for THIS document, loaded before anything mounts.
    enter::load_marks(state);

    let resume = enter::resume_page(saved_page, num_pages);

    // The reading position is authored HERE, once; the strip anchors to
    // it.
    state.reader.viewer.awaiting_anchor.set(true);
    state.reader.viewer.page.set(resume);
    state.reader.viewer.scroll_top.set(0.0);
    // Stale heights would anchor the first gesture wrongly.
    state
        .reader
        .document
        .content
        .metrics
        .css_heights
        .set(Vec::new());
    // The seed scale comes from the shared step.
    let (startup_fit, scale) = enter::startup_scale(state, (page1.width, page1.height));
    state.reader.viewer.fit.set(startup_fit);
    // Seed the zoom state HERE: all three scales agree, no transition.
    state.reader.viewer.zoom.initialize(scale);

    Seeded { resume, num_pages }
}

/// Intrinsic size of every page, packed one `PageSize` each.
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
        // A short array read off by one would misplace later pages.
        let sizes = intrinsic_sizes(&[10.0], &[100.0, 200.0], &size(612.0, 792.0), 2);
        assert_eq!(sizes.len(), 2);
        assert!(sizes.iter().all(|s| s.width == 612.0 && s.height == 792.0));
    }

    #[test]
    fn a_book_with_no_pages_has_no_sizes() {
        assert!(intrinsic_sizes(&[], &[], &size(612.0, 792.0), 0).is_empty());
    }
}
