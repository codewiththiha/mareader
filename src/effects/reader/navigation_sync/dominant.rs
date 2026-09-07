//! Scroll → page: the strip's dominant item names the page the reader is on.
//! One arm per axis, both the same shape. Each stands down while a freshly
//! mounted strip is still anchoring, during an open zoom transaction, and for
//! a held navigation that has not replayed yet; each guard carries its own
//! reasoning.

use std::rc::Rc;

use leptos::prelude::*;

use reader_core::view::ViewMode;
use virtual_list_leptos::Virtualizer;

use super::{Arms, JumpGate};

/// The page a strip's dominant item (0-based index) corresponds to, SAFELY.
///
/// The naive `dominant as u32 + 1` is a footgun: a strip that has not yet
/// resolved a window (fresh mode flip, mid-fit measure) can report a sentinel
/// or out-of-range index, and `usize::MAX` WRAPS to 0 — a view-mode change
/// would reset the reader to page 0, which reading-progress then persisted
/// over the real position. Clamping to `[1, page_count]` makes a momentary
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
        zooming,
    } = arms;
    let page = state.viewer.page;
    let mode = state.viewer.mode;
    Effect::new(move |_| {
        if mode.get() != axis {
            return;
        }
        // The continuous text stream owns its own page bookkeeping: it
        // virtualizes BLOCKS, so this arm's page-cut dominant would read a
        // virtualizer with no container and no window — and its one honest
        // write (page 1) would clobber the resume position. The stream maps
        // its dominant block to the page cut directly.
        if axis == ViewMode::ScrollVertical && state.reflowable() {
            return;
        }
        // A strip that just mounted (document open, back from the library, a
        // mode switch) is still being placed on `viewer.page` by
        // `ScrollShell`; until then its dominant is not the reader's page —
        // reading it now is what used to reset a resumed book to page 1.
        // TRACKED, so the arm re-runs on the frame the anchor lands and
        // adopts the true dominant.
        if state.viewer.awaiting_anchor.get() {
            return;
        }
        let dominant = page_from_dominant(v.dominant().get(), state.document.num_pages.get());
        // During a zoom transaction the virtualizer's window is frozen, but
        // a mid-zoom wheel can still rewindow and move the dominant item
        // through no fault of the reader; the zoom anchor already knows the
        // page, and syncing here made the number flicker to a neighbour
        // mid-gesture.
        //
        // TRACKED, because a container follow holds a transaction open for a
        // whole sidebar-slide or window-drag burst: shrinking the page fits
        // more of the book on screen, so the dominant legitimately moves
        // while the flag is up, and an untracked guard would drop that page
        // and leave the counter stale until the next scroll. Landing on the
        // commit is the same one write, at the moment the scale is final.
        if zooming.get() {
            return;
        }
        // A held navigation replays in this same flush and the strip has
        // not moved yet — correcting the page from the stale dominant now
        // would lose it. Let the replay land first; the re-run it causes
        // reads the TRUE dominant.
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
