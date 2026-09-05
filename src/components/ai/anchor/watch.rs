//! The reusable "glued to the page, dies when the origin leaves" behaviour, and
//! the two bands it dies in.
//!
//! One watcher serves both floating surfaces — the selection pill and the gloss
//! card — and the only difference between them is how far the origin may travel
//! before `exited` flips, which is what [`PILL_EXIT_FRAC`] and
//! [`CARD_EXIT_FRAC`] say.

use ai_core::gloss::{GlossBox, PageAnchor};
use leptos::prelude::*;

use app_chrome::hooks::use_raf::raf_coalesce;
use app_chrome::hooks::use_viewport::viewport_size;
use app_chrome::hooks::use_window_event::{add_window_capture_listener, use_window_event};

use super::MarkResolver;

/// The selection "Explain" pill lives until its origin fully leaves the viewport.
///
/// `1.0` is deliberate, not an untuned placeholder: the pill is small and
/// passive (it morphs nothing and owns no screen real estate), so it should
/// never vanish while any part of the text it points at is still visible —
/// unlike the gloss card below, which covers content and yields earlier.
pub const PILL_EXIT_FRAC: f64 = 1.0;

/// The expanded gloss card tolerates scroll until its origin passes this
/// fraction of the viewport height (or leaves the top edge).
pub const CARD_EXIT_FRAC: f64 = 0.8;

/// Whether an origin box has left the band it is allowed to live in: above the
/// viewport top, past `exit_frac` of the viewport height, or gone entirely
/// (its page unmounted, which by itself counts as left).
///
/// One definition for both bands — the watcher's soft `CARD_EXIT_FRAC` one and
/// the card's hard `1.0` one — so "off screen" cannot come to mean two
/// slightly different things.
pub fn origin_outside_band(origin: Option<GlossBox>, vh: f64, exit_frac: f64) -> bool {
    match origin {
        None => true,
        Some(b) => (b.y + b.h) < 0.0 || b.y > vh * exit_frac,
    }
}

#[derive(Clone, Copy)]
pub struct AnchorWatch {
    /// Live viewport-space box of the anchor (None = page not mounted).
    pub screen: RwSignal<Option<GlossBox>>,
    /// Origin left the allowed band: above the viewport top, or below
    /// `exit_frac` of the viewport height (or the page unmounted).
    pub exited: RwSignal<bool>,
    /// Synchronous re-derive (reads the DOM now). Call before using `screen`
    /// inside the same tick that the mark changed.
    pub refresh: Callback<()>,
}

/// Reusable "glued to the page, dies when the origin leaves" behaviour.
///
/// The screen box is re-derived whenever scroll / zoom / view mode / page /
/// container size change (plus a capture-phase scroll listener so *any*
/// scroller is caught, and window resize). `exit_frac` is the fraction of the
/// viewport height the origin may reach before `exited` flips: `1.0` means
/// "fully out of the viewport", `0.8` means "past 80% of the height".
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
    exit_frac: f64,
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
        let out = origin_outside_band(b, vh, exit_frac);
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
    use super::{origin_outside_band, CARD_EXIT_FRAC, PILL_EXIT_FRAC};
    use crate::components::ai::fixture::origin;

    #[test]
    fn an_unmounted_page_is_outside_every_band() {
        assert!(origin_outside_band(None, 900.0, PILL_EXIT_FRAC));
        assert!(origin_outside_band(None, 900.0, CARD_EXIT_FRAC));
    }

    #[test]
    fn the_full_band_only_gives_up_off_screen() {
        let vh = 900.0;
        assert!(!origin_outside_band(origin(300.0, 100.0), vh, PILL_EXIT_FRAC));
        // Overlapping either edge is still visible.
        assert!(!origin_outside_band(origin(-50.0, 100.0), vh, PILL_EXIT_FRAC));
        assert!(!origin_outside_band(origin(850.0, 100.0), vh, PILL_EXIT_FRAC));
        // Fully above / fully below.
        assert!(origin_outside_band(origin(-150.0, 100.0), vh, PILL_EXIT_FRAC));
        assert!(origin_outside_band(origin(901.0, 100.0), vh, PILL_EXIT_FRAC));
    }

    #[test]
    fn the_card_band_gives_up_early() {
        let vh = 900.0; // the card's band ends at 720
        assert!(!origin_outside_band(origin(700.0, 20.0), vh, CARD_EXIT_FRAC));
        assert!(origin_outside_band(origin(760.0, 20.0), vh, CARD_EXIT_FRAC));
        // Still visible, but past the band: the pill would stay, the card goes.
        assert!(!origin_outside_band(origin(760.0, 20.0), vh, PILL_EXIT_FRAC));
    }
}
