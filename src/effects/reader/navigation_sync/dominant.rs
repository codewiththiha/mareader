//! Scroll → page: the strip's dominant item names the page the reader is on.
//!
//! One arm per axis, both the same shape — only the view mode they answer for
//! differs. Each stands down for a mode restore, for an open zoom transaction,
//! and for a held navigation that has not replayed yet; the reasoning for each
//! stand-down is at its guard.

use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::Virtualizer;

use super::JumpGate;
use super::Arms;

/// The page a strip's dominant item (0-based index) corresponds to, SAFELY.
///
/// The naive `dominant as u32 + 1` is a real footgun: a strip that has not
/// yet resolved a window (a freshly-mounted mode flip, a mid-fit measure)
/// can report a sentinel or out-of-range index, and if that index is
/// `usize::MAX` the `as u32 + 1` WRAPS to 0 — so a view-mode change would
/// reset the reader to page 0, which reading-progress then persisted over
/// the real position. Clamping to `[1, page_count]` makes a momentary
/// no-window read harmless instead of destructive.
fn page_from_dominant(dominant: usize, num_pages: u32) -> u32 {
    let raw = dominant.saturating_add(1) as u64;
    raw.clamp(1, u64::from(num_pages.max(1))) as u32
}

/// Install the scroll → page arm for one axis.
pub(super) fn install(arms: Arms, axis: ViewMode, v: Virtualizer, gate: Rc<JumpGate>) {
    let Arms {
        state,
        suppress,
        defer,
        zooming,
    } = arms;
    let page = state.viewer.page;
    let mode = state.viewer.mode;
    Effect::new(move |_| {
        if mode.get() != axis {
            return;
        }
        // While on_mode_change is re-anchoring the strip after a mode flip,
        // the strip is still sitting at its initial (unrestored) offset and
        // its dominant would misreport the page — reading it now resets the
        // reader back to (a transient) page 0/1. Stand down; the restored
        // scroll re-runs this effect with the true dominant.
        if defer.get() {
            return;
        }
        let dominant = page_from_dominant(v.dominant().get(), state.document.num_pages.get());
        // During a zoom transaction the virtualizer's window is frozen,
        // but a mid-zoom wheel can still rewindow and move the dominant
        // item through no fault of the reader. The zoom anchor already
        // knows the page; syncing it here is what made the page number
        // flicker to a neighbour mid-gesture.
        //
        // TRACKED, because a container follow holds a transaction open for
        // the whole burst of a sidebar slide or a window drag: shrinking the
        // page fits more of the book on screen, so the dominant item
        // legitimately moves while the flag is up, and an untracked guard
        // would drop that page and leave the counter on the old one until
        // the reader scrolled again. Landing on the commit is the same one
        // write, at the moment the scale is final.
        if zooming.get() {
            return;
        }
        // A held navigation replays in this same flush and the strip has
        // not moved yet — correcting the page from the stale dominant now
        // is exactly how the resume jump used to die. Let the replay land
        // first; the re-run it causes reads the TRUE dominant.
        if gate.pending().is_some() {
            return;
        }
        if page.get_untracked() == dominant {
            return;
        }
        suppress.set(true);
        page.set(dominant);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The view-mode-change regression: a strip that reports a sentinel index
    /// (no window resolved yet) maps to a real page, never to 0.
    #[test]
    fn a_sentinel_dominant_index_never_becomes_page_zero() {
        // usize::MAX used to wrap to 0 through `as u32 + 1`.
        assert_eq!(page_from_dominant(usize::MAX, 300), 300);
        // A truly empty strip reads as page 1 (the first page), not 0.
        assert_eq!(page_from_dominant(0, 300), 1);
        // An index beyond the book clamps to the last page.
        assert_eq!(page_from_dominant(999, 50), 50);
        // With no pages known yet, everything clamps to page 1.
        assert_eq!(page_from_dominant(999, 0), 1);
        // Ordinary indices round-trip.
        assert_eq!(page_from_dominant(41, 300), 42);
    }
}
