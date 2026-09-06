//! The reusable "glued to the page, dies when the origin leaves" behaviour.
//!
//! One watcher serves both floating surfaces — the selection pill and the
//! gloss card — and both die by one rule: the origin is out when NO PART of
//! it is on screen (or its host unmounted), identically above the top edge
//! and below the bottom edge, identically for PDF and reflow.

use ai_core::gloss::{GlossBox, PageAnchor};
use leptos::prelude::*;

use app_chrome::hooks::use_raf::raf_coalesce;
use app_chrome::hooks::use_viewport::viewport_size;
use app_chrome::hooks::use_window_event::{add_window_capture_listener, use_window_event};

use super::MarkResolver;

/// One exit rule for every anchored surface (pill and card, PDF and reflow):
/// the surface stays open while any part of its mark is on screen, and exits
/// only once the mark has fully left the viewport — above the top edge or
/// below the bottom edge. `None` (the mark's host is unmounted) always counts
/// as out.
///
/// The old rule mixed two bands: fully-out at the top but `y > vh * 0.8` at
/// the bottom. Scrolling up moves the mark toward the bottom edge, so the
/// card collapsed while the mark was still visible; scrolling down looked
/// correct only because the top edge used the strict rule.
pub fn origin_outside_band(origin: Option<GlossBox>, vh: f64) -> bool {
    match origin {
        None => true,
        Some(b) => (b.y + b.h) <= 0.0 || b.y >= vh,
    }
}

#[derive(Clone, Copy)]
pub struct AnchorWatch {
    /// Live viewport-space box of the anchor (None = page not mounted).
    pub screen: RwSignal<Option<GlossBox>>,
    /// Origin left the viewport: fully above the top edge or fully below
    /// the bottom edge (or its host unmounted). See [`origin_outside_band`].
    pub exited: RwSignal<bool>,
    /// Synchronous re-derive (reads the DOM now). Call before using `screen`
    /// inside the same tick that the mark changed.
    pub refresh: Callback<()>,
}

/// Reusable "glued to the page, dies when the origin leaves" behaviour.
///
/// The screen box is re-derived whenever scroll / zoom / view mode / page /
/// container size change (plus a capture-phase scroll listener so *any*
/// scroller is caught, and window resize), and `exited` follows the one
/// symmetric rule ([`origin_outside_band`]).
///
/// `resolve` is the format's answer to "where is this anchor in the viewport
/// right now" — [`super::anchor_resolver`] builds the right one for whichever
/// document is open. `invalidate` is the format's answer to "something moved that scroll
/// and zoom do not cover": a reflowable document re-cuts its pages when the
/// typography or the column width changes, and a mark that stayed put through
/// that would be pointing at the wrong words. A PDF has nothing to add, so it
/// passes [`super::no_invalidation`].
pub fn watch_page_anchor(
    anchor: Signal<Option<PageAnchor>>,
    resolve: MarkResolver,
    scale: Signal<f64>,
    scroll_top: Signal<f64>,
    page: Signal<u32>,
    invalidate: Signal<u64>,
) -> AnchorWatch {
    let screen = RwSignal::new(None::<GlossBox>);
    let exited = RwSignal::new(false);
    let tick = RwSignal::new(0u32);

    let refresh = Callback::new(move |_| {
        let b = anchor
            .get_untracked()
            .and_then(|a| resolve.run((a, scale.get_untracked())));
        if screen.get_untracked() != b {
            screen.set(b);
        }
        let (_, vh) = viewport_size();
        let out = origin_outside_band(b, vh);
        if exited.get_untracked() != out {
            exited.set(out);
        }
    });

    Effect::new(move |_| {
        let _ = anchor.get();
        let _ = scale.get();
        let _ = scroll_top.get();
        let _ = page.get();
        let _ = invalidate.get();
        let _ = tick.get();
        refresh.run(());
    });

    // Scroll and resize both fire faster than the screen updates, and each
    // re-derive reads layout twice (the page host's rect, the viewport size).
    // Coalescing to one recompute per frame drops the passes whose results
    // were overwritten before anything was painted; the card is spring-driven
    // at frame rate anyway, so it cannot tell the difference. Anything that
    // needs the anchor NOW (an open, mid-tick) calls `refresh` directly.
    let queue_refresh = raf_coalesce(move || tick.update(|n| *n += 1));
    let on_scroll = queue_refresh.clone();
    add_window_capture_listener("scroll", move |_| on_scroll());
    use_window_event("resize", move |_| queue_refresh());

    AnchorWatch {
        screen,
        exited,
        refresh,
    }
}

#[cfg(test)]
mod tests {
    use super::origin_outside_band;
    use crate::components::ai::fixture::origin;

    #[test]
    fn an_unmounted_page_is_outside_every_band() {
        assert!(origin_outside_band(None, 900.0));
    }

    #[test]
    fn only_fully_out_of_view_counts_as_gone() {
        let vh = 900.0;
        assert!(!origin_outside_band(origin(300.0, 100.0), vh));
        // Overlapping either edge is still visible.
        assert!(!origin_outside_band(origin(-50.0, 100.0), vh));
        assert!(!origin_outside_band(origin(850.0, 100.0), vh));
        // Fully above / fully below.
        assert!(origin_outside_band(origin(-150.0, 100.0), vh));
        assert!(origin_outside_band(origin(901.0, 100.0), vh));
    }

    #[test]
    fn touching_a_viewport_edge_without_overlapping_is_outside() {
        let vh = 900.0;
        assert!(origin_outside_band(origin(-100.0, 100.0), vh));
        assert!(origin_outside_band(origin(vh, 100.0), vh));
    }

    #[test]
    fn a_mark_near_the_bottom_edge_stays_open_while_scrolling_up() {
        let vh = 900.0;
        assert!(!origin_outside_band(origin(760.0, 20.0), vh));
        assert!(!origin_outside_band(origin(880.0, 200.0), vh));
    }
}
