//! Scroll → page: the strip's dominant item names the reader's page.

use std::rc::Rc;

use leptos::prelude::*;

use reader_core::view::ViewMode;
use virtual_list_leptos::Virtualizer;

use super::{Arms, JumpGate};

/// The page a strip's dominant item (0-based) corresponds to, safely.
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
        // The dominant's scope outlives the reader state by a beat.
        let Some(mode_now) = mode.try_get() else {
            return;
        };
        if mode_now != axis {
            return;
        }
        // The text stream owns its own page bookkeeping.
        if axis == ViewMode::ScrollVertical && state.reflowable() {
            return;
        }
        // A just-mounted strip is still being placed on `viewer.page`.
        if state.viewer.awaiting_anchor.get() {
            return;
        }
        let dominant = page_from_dominant(v.dominant().get(), state.document.num_pages.get());
        // Mid-zoom the frozen window can still move the dominant.
        if zooming.get() {
            return;
        }
        // A held navigation replays in this same flush; let it land first.
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

    /// The view-mode-change regression: a sentinel index maps to a real
    /// page, never 0.
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
