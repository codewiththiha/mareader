//! Window-level interactions: Escape handling, outside-press dismissal,
//! origin-exit collapse, the zoom guard, and the outro's settle-unmount.
//! Each is a small self-contained hook so the popover stays wiring + view;
//! the generic part of dismissal (outside press, topmost Escape, exclusion
//! selectors) is the primitive `use_dismiss`, and only the two-step gloss
//! semantics (first Escape collapses, second gives up) stay here.

use ai_core::gloss::{GlossBox, boxes_close};
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::ai::anchor::{AnchorWatch, origin_outside_band};
use crate::components::ai::gloss::controller::GlossController;
use app_chrome::floating::dismiss::{DismissPolicy, DismissTrigger, use_dismiss};
use app_chrome::hooks::use_viewport::viewport_size;
use crate::components::ai::gloss::phase::GlossPhase;
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

/// Whether the origin has left the viewport entirely — fully above the top
/// or fully below the bottom, identically in both directions. A `None` box
/// (the mark's host unmounted) counts as gone no matter how the card opened.
fn origin_gone(origin: Option<GlossBox>, vh: f64) -> bool {
    origin_outside_band(origin, vh)
}

/// Scrolling does not kill the card while any part of its mark is on screen:
/// the surface tracks its anchor until the origin fully leaves the viewport
/// (either edge) or its host is virtualized away — then it collapses back
/// onto the mark. Page flips need no rule of their own: in the paginated
/// modes a flip unmounts the host (origin `None`), and in the continuous
/// ones a boundary crossing is just scroll — the watcher re-resolves on the
/// page signal and this rule stays the single source of exits. A collapsed
/// card only reopens on an explicit click, so a mark riding the viewport
/// edge cannot flicker.
pub fn use_origin_exit_collapse(watch: AnchorWatch, ctrl: GlossController) {
    Effect::new(move |_| {
        if !ctrl.geometry.surface_visible.get() || ctrl.geometry.gphase.get() != GlossPhase::Expanded {
            return;
        }
        let (_, vh) = viewport_size();
        if origin_gone(watch.screen.get(), vh) {
            ctrl.commands.collapse_to_mark.run(());
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
    use crate::components::ai::fixture::origin;

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
}
