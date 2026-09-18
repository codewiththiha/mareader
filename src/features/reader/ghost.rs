//! The ghost: one real page of the open document, rendered once at ~140px,
//! desaturated, and stretched across every mounted-but-unpainted page as
//! its placeholder. Cost: one tiny render (a few KB of WebP, one blob URL)
//! for the whole session, versus ~22 MB of canvas per full-resolution page
//! — which is exactly what a fling used to churn.
//!
//! The engine owns the render (`public/engine/ghost.ts`, through
//! `pdf_engine::api::render_ghost`); this module owns which page it is —
//! the document's MODAL page size, the size most pages wear, so the
//! placeholder reads as a preview of THIS book rather than a generic sheet.

use std::collections::HashMap;

use pdf_engine::types::PageSize;

/// (0-based index, width, height) of the FIRST page whose size is the
/// document's most common one. 16px bucketing tolerates scan noise — a page
/// off by a hair from its siblings still lands in the same bucket — and
/// `max_by_key` keeps the FIRST page of the winning bucket, the one the
/// reader meets first.
pub fn modal_page(intrinsic: &[PageSize]) -> Option<(usize, f64, f64)> {
    let mut hist: HashMap<(u32, u32), (usize, usize)> = HashMap::new();
    for (i, s) in intrinsic.iter().enumerate() {
        if s.width <= 0.0 || s.height <= 0.0 {
            continue;
        }
        let key = ((s.width / 16.0).round() as u32, (s.height / 16.0).round() as u32);
        hist.entry(key).or_insert((0, i)).0 += 1;
    }
    let first = hist
        .iter()
        .max_by_key(|entry| entry.1 .0)
        .map(|entry| entry.1.1)?;
    let size = intrinsic.get(first)?;
    Some((first, size.width, size.height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(w: f64, h: f64) -> PageSize {
        PageSize { width: w, height: h }
    }

    #[test]
    fn the_modal_bucket_wins_and_its_first_page_wins_the_tie() {
        // Ten pages: seven A4-ish (one of them off by a hair — scan noise,
        // still the same bucket), one square-ish, two wide. The A4 bucket is
        // the modal, and the FIRST page of it (index 1) represents it, not
        // the last.
        let sizes = vec![
            page(600.0, 800.0),
            page(595.0, 842.0),
            page(594.0, 843.0),
            page(601.0, 841.0),
            page(595.0, 842.0),
            page(595.0, 842.0),
            page(595.0, 842.0),
            page(595.0, 842.0),
            page(900.0, 842.0),
            page(901.0, 842.0),
        ];
        assert_eq!(modal_page(&sizes), Some((1, 595.0, 842.0)));
    }

    #[test]
    fn empty_and_unmeasured_pages_contribute_nothing() {
        assert_eq!(modal_page(&[]), None);
        assert_eq!(modal_page(&[page(0.0, 0.0), page(0.0, 842.0)]), None);
    }
}
