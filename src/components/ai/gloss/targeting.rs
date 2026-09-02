//! Card targeting: everything that decides where the sprung box wants to be
//! right now — the page-aware anchor watch, the reactive viewport, the
//! content-height measurement, the expanded and spring targets, the spring
//! itself, and the morph progress derived from all of them.
//!
//! Extracted from the popover's wiring so the composition root reads as
//! controller → targeting → lifecycle hooks → view, and so the pieces that
//! need to coordinate (the open effect re-anchors the spring; the settle
//! watcher reads the anchor and the sprung box) share one named bundle
//! instead of twelve loose locals.

use ai_core::gloss::GlossBox;
use leptos::{html, prelude::*};

use crate::components::ai::anchor::{watch_page_anchor, AnchorWatch, PageAnchor, CARD_EXIT_FRAC};
use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::hooks::use_content_measure::use_content_measure;
use crate::components::ai::gloss::placement::{expanded_target, spring_target};
use crate::components::ai::types::GlossPhase;
use app_chrome::hooks::use_viewport::use_viewport;
use crate::components::primitives::motion::reduced_motion::reduced_motion_signal;
use crate::components::primitives::motion::spring::{use_spring_box, SpringBox};
use crate::state::AppState;

/// The targeting bundle consumed by the lifecycle hooks and the surface.
/// Created by [`use_card_targeting`] inside the popover's reactive scope.
pub struct CardTargeting {
    /// The page-aware anchor watcher: live screen box + the exit band.
    pub watch: AnchorWatch,
    /// Live viewport-space box of the current mark (None = page unmounted).
    pub anchor: RwSignal<Option<GlossBox>>,
    /// Reactive viewport size (resize-aware), owned by this scope.
    pub viewport: RwSignal<(f64, f64)>,
    /// NodeRef for the invisible measure twin rendered beside the surface
    /// (the twin's measured height feeds the expanded target directly).
    pub measure_ref: NodeRef<html::Div>,
    /// Where the expanded card wants to sit (side-aware, viewport-clamped).
    pub expanded: Memo<Option<GlossBox>>,
    /// The spring, with its hard-reset for newly opened anchors.
    pub spring: SpringBox<GlossBox>,
    /// The live sprung box.
    pub sprung: RwSignal<Option<GlossBox>>,
    /// 0..1 morph progress, derived from the sprung width.
    pub progress: Memo<f64>,
}

/// Build the card's targeting layer: anchor watch, viewport, measurement,
/// targets, spring and progress. Must run before the lifecycle hooks that
/// re-anchor the spring (the open effect) or read the anchor (the
/// origin-exit and settle watchers).
pub fn use_card_targeting(state: AppState, ctrl: GlossController) -> CardTargeting {
    // ONE shared, page-aware anchor: follows scroll/zoom/mode/page, and
    // flags `exited` once the origin passes CARD_EXIT_FRAC of the viewport
    // height (or leaves the top, or its page unmounts).
    let watch = watch_page_anchor(
        Signal::derive(move || ctrl.open.mark.get().map(|m| PageAnchor::from_mark(&m))),
        state.reader.viewer.zoom.display.into(),
        state.reader.viewer.mode.into(),
        state.reader.viewer.scroll_top.into(),
        state.reader.viewer.page.into(),
        CARD_EXIT_FRAC,
    );
    let anchor = watch.screen;

    // Reactive viewport (the shared primitive): resize-aware signal, owned
    // by this reactive scope.
    let viewport = use_viewport();
    let reduced = reduced_motion_signal();

    // Card content measurement: the invisible twin's height, re-measured
    // whenever the word or the answer changes (title wrap included).
    let (measure_ref, content_height) =
        use_content_measure(ctrl.content.word, ctrl.content.word_info);

    let expanded = expanded_target(anchor.into(), content_height, viewport);
    let target = spring_target(
        anchor.into(),
        ctrl.geometry.gphase,
        ctrl.drag.offset,
        expanded,
        viewport,
    );

    // Snapping while compact was what made closing read as a cut: the spring
    // teleported the surface onto the anchor instead of morphing down to it.
    // Only the processing phase (where no surface exists anyway) snaps.
    let snap = Signal::derive(move || {
        ctrl.drag.active.get() || reduced.get() || ctrl.geometry.gphase.get() == GlossPhase::Processing
    });
    let spring: SpringBox<GlossBox> = use_spring_box(target.into(), snap);
    let sprung = spring.value;

    let progress = Memo::new(move |_| {
        let (Some(b), Some(a), Some(e)) = (sprung.get(), anchor.get(), expanded.get()) else {
            return if ctrl.geometry.gphase.get() == GlossPhase::Expanded {
                1.0
            } else {
                0.0
            };
        };
        ((b.w - a.w) / (e.w - a.w).max(1.0)).clamp(0.0, 1.0)
    });

    CardTargeting {
        watch,
        anchor,
        viewport,
        measure_ref,
        expanded,
        spring,
        sprung,
        progress,
    }
}
