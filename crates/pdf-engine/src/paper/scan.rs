//! The fixed-mode background scan: sample pages one at a time, pool every
//! frame into the document detector, settle the book's colour — resumably.

use pdf_paper::{PaperMode, Rgb, PAPER_SHARE};

use crate::api;

use super::{feed_state, publish, publish_with, with, Session};

/// The colour worth banking when a book closes: the pooled dominant of the
/// pages an interrupted scan reached, or the interim when it reached none
/// (a one-page answer — still this book's paper, not the theme's). `None`
/// when there is nothing to bank: no book, blend off, continuous mode, or a
/// colour that was already settled — and therefore already persisted — by
/// a completed scan or a cache hit.
pub(super) fn bankable_partial(s: &Session) -> Option<Rgb> {
    if !s.blend_on
        || s.config.mode != PaperMode::Fixed
        || s.doc_path.is_none()
        || s.fixed_final.is_some()
    {
        return None;
    }
    s.document.dominant(PAPER_SHARE).or(s.interim)
}

/// Would a scan be useful right now? (Fixed mode live, a book open, no
/// final colour, no scan running — and the reader has painted at least one
/// page: the scan's offscreen renders share the pdf.js worker with the
/// first visible render, and that render wins the worker by right.)
pub(super) fn scan_should_start(s: &Session) -> bool {
    s.blend_on
        && s.config.mode == PaperMode::Fixed
        && s.doc_path.is_some()
        && s.fixed_final.is_none()
        && s.interim.is_some()
        && !s.scanning
}

pub(super) fn start_scan_if_needed() {
    if !super::with(|s| scan_should_start(s)) {
        return;
    }
    start_scan();
}

/// Spawn the background scan: sample pages `scan_cursor..=cap` one at a
/// time (the engine yields between pages so live renders never queue
/// behind it), pooling every frame into the document detector, then settle
/// the book's colour. The cursor makes the scan RESUMABLE: cancelling it
/// (blend off, mode flip, book change) and starting again later continues
/// from the page after the last one fed.
fn start_scan() {
    let epoch = with(|s| {
        s.scanning = true;
        if s.scan_cursor == 0 {
            s.scan_cursor = 1;
        }
        s.epoch
    });
    super::spawn_engine(move || scan_task(epoch));
}

async fn scan_task(epoch: u64) {
    loop {
        // The next page to sample, or None when the scan is done/cancelled.
        let next = with(|s| {
            if s.epoch != epoch || !s.scanning {
                return None; // cancelled or superseded
            }
            let last = s.num_pages.min(s.config.scan_pages);
            if s.scan_cursor > last {
                return Some(0); // done
            }
            Some(s.scan_cursor)
        });
        let Some(page) = next else { return };
        if page == 0 {
            finish_scan(epoch);
            return;
        }
        let frame = api::sample_paper_page(page).await.ok().flatten();
        let changed = with(|s| {
            if s.epoch != epoch || !s.scanning {
                return false; // the book changed under the sample
            }
            let mut changed = false;
            if let Some(f) = &frame {
                s.document.feed(
                    s.config.area,
                    f.width as usize,
                    f.height as usize,
                    &f.data,
                    s.config.edge_width as usize,
                );
                changed = feed_state(s, f); // the scan fills the palette for free
            }
            s.scan_cursor = page + 1;
            changed
        });
        if changed {
            // A scan page can be the first frame the book ever offered (no
            // live render yet): the interim it establishes must publish.
            publish();
        }
    }
}

/// The scan covered its budget: settle the book's colour from the pooled
/// histogram (falling back to the interim for photo books with no paper
/// majority) and persist it — this is the ONE publish that writes the
/// per-document cache.
pub(super) fn finish_scan(epoch: u64) {
    let done = with(|s| {
        if s.epoch != epoch {
            return false;
        }
        s.scanning = false;
        s.fixed_final = s.document.dominant(PAPER_SHARE).or(s.interim);
        s.fixed_final.is_some()
    });
    if done {
        publish_with(true);
    }
}
