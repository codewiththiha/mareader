//! Window-level gloss interactions: Escape, outside press, origin exit.

use ai_core::gloss::{GlossBox, boxes_close};
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::ai::anchor::{AnchorWatch, origin_outside_band};
use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::phase::GlossPhase;
use app_chrome::floating::dismiss::{DismissPolicy, DismissTrigger, use_dismiss};
use app_chrome::hooks::use_viewport::viewport_size;

/// Escape collapses the card; a second gives up on the gloss.
pub fn use_dismiss_interactions(ctrl: GlossController) {
    // Two-step Escape is domain policy; the primitive handles the press
    // half.
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
                // The first Escape closes the card with its outro.
                GlossPhase::Expanded => ctrl.commands.collapse_to_mark.run(()),
                _ => ctrl.commands.reset.run(()),
            }
        });
        on_cleanup(move || key.remove());
    });

    // A press inside is the card's; the mark stroke counts as inside.
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

/// Whether the origin left the viewport entirely.
fn origin_gone(origin: Option<GlossBox>, vh: f64) -> bool {
    origin_outside_band(origin, vh)
}

/// Scrolling does not kill the card while part of its mark is on
/// screen.
pub fn use_origin_exit_collapse(watch: AnchorWatch, ctrl: GlossController) {
    Effect::new(move |_| {
        if !ctrl.geometry.surface_visible.get()
            || ctrl.geometry.gphase.get() != GlossPhase::Expanded
        {
            return;
        }
        let (_, vh) = viewport_size();
        if origin_gone(watch.screen.get(), vh) {
            ctrl.commands.collapse_to_mark.run(());
        }
    });
}

/// The outro's hand-off: unmount on SETTLE, so stroke and surface
/// never both show.
pub fn use_settle_unmount(
    ctrl: GlossController,
    anchor: Signal<Option<GlossBox>>,
    sprung: Signal<Option<GlossBox>>,
) {
    Effect::new(move |_| {
        if !ctrl.geometry.surface_visible.get() || ctrl.geometry.gphase.get() != GlossPhase::Compact
        {
            return;
        }
        let Some(a) = anchor.get() else {
            // The page unmounted mid-morph: nothing left to land on.
            ctrl.geometry.surface_visible.set(false);
            return;
        };
        if sprung.get().is_some_and(|b| boxes_close(b, a, 0.5)) {
            ctrl.geometry.surface_visible.set(false);
        }
    });
}

/// A zoom re-renders the textLayer; close the card, keep the
/// highlight.
pub fn use_zoom_reset(state: crate::context::ReaderContext, ctrl: GlossController) {
    Effect::new(move |_| {
        // TRACKED: the untracked form never fired again after mount.
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
