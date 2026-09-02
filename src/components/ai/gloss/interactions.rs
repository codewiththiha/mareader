//! Window-level interactions: Escape handling, outside-press dismissal,
//! origin-exit collapse, page-flip/zoom guards, and the outro's
//! settle-unmount. Each is a small self-contained hook so the popover stays
//! wiring + view; the generic part of dismissal (outside press, topmost
//! Escape, exclusion selectors) is the primitive `use_dismiss`, and only the
//! two-step gloss semantics (first Escape collapses, second gives up) stay
//! here.

use ai_core::gloss::{boxes_close, GlossBox};
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::ai::anchor::{origin_outside_band, AnchorWatch, MENU_EXIT_FRAC};
use crate::components::ai::gloss::controller::GlossController;
use crate::components::primitives::floating::dismiss::{use_dismiss, DismissPolicy, DismissTrigger};
use app_chrome::hooks::use_viewport::viewport_size;
use crate::components::ai::types::GlossPhase;
use crate::state::AppState;

/// Escape collapses the expanded card; a second Escape on the bare chip gives
/// up on the gloss entirely. Outside presses collapse too (only while
/// expanded — a bare chip is reachable by design).
pub fn use_dismiss_interactions(ctrl: GlossController) {
    // Two-step Escape: the gloss owns its own meaning (collapse → reset), so
    // the primitive is used for the outside-press half and the Escape half
    // stays here as domain policy.
    Effect::new(move |_| {
        if !ctrl.geometry.surface_visible.get() {
            return;
        }
        let key = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            let ke = ev.unchecked_ref::<web_sys::KeyboardEvent>();
            if ke.key() != "Escape" {
                return;
            }
            match ctrl.geometry.gphase.get_untracked() {
                // First Escape closes the card (with the outro); a second one
                // on the bare chip gives up on the gloss entirely.
                GlossPhase::Expanded => ctrl.commands.collapse_to_mark.run(()),
                _ => ctrl.commands.reset.run(()),
            }
        });
        on_cleanup(move || key.remove());
    });

    // A press inside the surface is the card's own interaction; anywhere else
    // collapses an expanded card (compact chips stay put). A press on the
    // mark stroke is the SAME request as the card (it is what reopens it), so
    // it is excluded too: without this it fired the outside-collapse on
    // pointerdown and then REOPENED on the follow-up click — the card would
    // fold and immediately pop back rather than toggle closed.
    use_dismiss(
        ctrl.geometry.surface_visible.into(),
        ctrl.commands.collapse_to_mark,
        DismissPolicy {
            escape: false,
            outside: Some(DismissTrigger::PointerDown),
            exclude_selectors: vec![".gloss-surface", ".gloss-mark"],
            enabled: None,
            topmost_only: false,
        },
        |_| false,
    );
}

/// Whether the origin has left the viewport entirely — above the top or
/// below the bottom. A `None` box (the mark's page unmounted) counts as gone:
/// the hard exit fires no matter how the card opened.
///
/// The hard exit is the full-viewport band, the same shape the watcher applies
/// to its softer `CARD_EXIT_FRAC` one.
fn origin_gone(origin: Option<GlossBox>, vh: f64) -> bool {
    origin_outside_band(origin, vh, MENU_EXIT_FRAC)
}

/// The soft band's verdict for an origin still inside the viewport: arm the
/// band while the origin is inside it, collapse once it has left — but only
/// if it was armed while inside. A card opened near the bottom edge starts
/// past the band and unarmed; collapsing it on spawn is what made low opens
/// read as "the card can't decide where to spawn", so it survives until it
/// has been inside the band at least once.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum BandVerdict {
    /// Origin inside the band: arm the one-shot.
    Arm,
    /// Origin past the band and it was armed: collapse.
    Collapse,
    /// Origin past the band but never armed: leave it (the hard exit owns it).
    Keep,
}

fn band_verdict(exited: bool, armed: bool) -> BandVerdict {
    match (exited, armed) {
        (false, _) => BandVerdict::Arm,
        (true, true) => BandVerdict::Collapse,
        (true, false) => BandVerdict::Keep,
    }
}

/// Scrolling does not kill the card instantly: it tracks its anchor until the
/// origin crosses CARD_EXIT_FRAC of the viewport height, leaves the top, or
/// its page is virtualized away — then it collapses back onto the mark.
pub fn use_origin_exit_collapse(watch: AnchorWatch, ctrl: GlossController) {
    Effect::new(move |_| {
        if !ctrl.geometry.surface_visible.get() || ctrl.geometry.gphase.get() != GlossPhase::Expanded {
            return;
        }

        // Hard exit: the mark's page unmounted, or the origin fully left the
        // viewport (top or bottom). Collapses no matter how the card opened.
        let (_, vh) = viewport_size();
        if origin_gone(watch.screen.get(), vh) {
            ctrl.commands.collapse_to_mark.run(());
            return;
        }

        // Soft band: arm while the origin is inside; only an armed card is
        // collapsed by the band. Opened low → unarmed → survives; scroll up
        // (arms) then back down past the band → collapses, as designed.
        match band_verdict(watch.exited.get(), ctrl.geometry.exit_armed.get_untracked()) {
            BandVerdict::Arm => ctrl.geometry.exit_armed.set(true),
            BandVerdict::Collapse => ctrl.commands.collapse_to_mark.run(()),
            BandVerdict::Keep => {}
        }
    });
}

/// The outro's hand-off: once the collapsing surface has morphed down onto
/// the anchor, unmount it and let the in-page stroke take over. Doing this on
/// SETTLE rather than on a timer is what keeps the two from being visible at
/// once (the stroke is drawn on the same exact-fit box the surface lands on).
pub fn use_settle_unmount(
    ctrl: GlossController,
    anchor: Signal<Option<GlossBox>>,
    sprung: Signal<Option<GlossBox>>,
) {
    Effect::new(move |_| {
        if !ctrl.geometry.surface_visible.get() || ctrl.geometry.gphase.get() != GlossPhase::Compact {
            return;
        }
        let Some(a) = anchor.get() else {
            // The mark's page unmounted mid-morph: there is nothing left to
            // land on, so drop the surface now.
            ctrl.geometry.surface_visible.set(false);
            return;
        };
        if sprung.get().is_some_and(|b| boxes_close(b, a, 0.5)) {
            ctrl.geometry.surface_visible.set(false);
        }
    });
}

/// A page flip collapses an expanded card back onto its mark (which may now
/// be off screen — the anchor still knows where it is).
pub fn use_page_flip_collapse(state: AppState, ctrl: GlossController) {
    Effect::new(move |_| {
        let _ = state.reader.viewer.page.get();
        if ctrl.geometry.surface_visible.get_untracked() {
            ctrl.commands.collapse_to_mark.run(());
        }
    });
}

/// A zoom re-renders the textLayer; the mark survives it, but the open card
/// would slide under the reader's hands mid-gesture — close it and leave the
/// highlight behind.
pub fn use_zoom_reset(state: AppState, ctrl: GlossController) {
    Effect::new(move |_| {
        // TRACKED: this must re-run when a transaction opens. The untracked
        // form left the effect with no reactive dependency at all — it ran
        // once at mount, found no zoom, and never fired again, so the card
        // never actually closed on a zoom.
        if !state.reader.viewer.zooming().get() {
            return;
        }
        if state.reader.ai_selection.popover_open.get_untracked() {
            ctrl.commands.reset.run(());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(y: f64, h: f64) -> Option<GlossBox> {
        Some(GlossBox {
            x: 100.0,
            y,
            w: 40.0,
            h,
            r: 6.0,
        })
    }

    #[test]
    fn an_unmounted_page_counts_as_gone() {
        assert!(origin_gone(None, 900.0));
    }

    #[test]
    fn the_hard_exit_fires_only_fully_outside_the_viewport() {
        let vh = 900.0;
        // Comfortably inside.
        assert!(!origin_gone(origin(300.0, 100.0), vh));
        // Overlapping the top edge is still visible.
        assert!(!origin_gone(origin(-50.0, 100.0), vh));
        // Overlapping the bottom edge is still visible.
        assert!(!origin_gone(origin(850.0, 100.0), vh));
        // Fully above: (y + h) < 0.
        assert!(origin_gone(origin(-150.0, 100.0), vh));
        // Fully below: y > vh.
        assert!(origin_gone(origin(901.0, 100.0), vh));
    }

    #[test]
    fn the_band_arms_inside_and_collapses_only_once_armed() {
        // Inside the band: arm.
        assert_eq!(band_verdict(false, false), BandVerdict::Arm);
        assert_eq!(band_verdict(false, true), BandVerdict::Arm);
        // Past the band, armed at some point: collapse.
        assert_eq!(band_verdict(true, true), BandVerdict::Collapse);
        // Past the band, never armed (opened low): the hard exit owns it.
        assert_eq!(band_verdict(true, false), BandVerdict::Keep);
    }
}
