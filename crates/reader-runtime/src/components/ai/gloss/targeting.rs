//! Card targeting: where the sprung box wants to be right now.

use ai_core::gloss::GlossBox;
use leptos::html;
use leptos::prelude::*;

use crate::components::ai::anchor::{
    AnchorWatch, PageAnchor, anchor_resolver, no_invalidation, reflow_invalidation,
    watch_page_anchor,
};
use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::hooks::use_content_measure::use_content_measure;
use crate::components::ai::gloss::phase::GlossPhase;
use crate::components::ai::gloss::placement::{expanded_target, spring_target};
use crate::components::ai::reflow_anchor::parse_spot;
use app_chrome::hooks::use_viewport::use_viewport;
use app_ui::components::primitives::motion::reduced_motion::reduced_motion_signal;
use app_ui::components::primitives::motion::spring::{SpringBox, use_spring_box};

/// The targeting bundle consumed by the hooks and the surface.
pub struct CardTargeting {
    /// The page-aware anchor watcher: live screen box + the exit band.
    pub watch: AnchorWatch,
    /// Live viewport-space box of the current mark (None = page unmounted).
    pub anchor: RwSignal<Option<GlossBox>>,
    /// Reactive viewport size (resize-aware), owned by this scope.
    pub viewport: RwSignal<(f64, f64)>,
    /// NodeRef for the measure twin beside the surface.
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

/// Build the targeting layer, before the hooks that read it.
pub fn use_card_targeting(
    state: crate::context::ReaderContext,
    ctrl: GlossController,
) -> CardTargeting {
    // ONE page-aware anchor: follows scroll/zoom/mode/page, flags `exited`.
    let spots = state.reader.gloss.spots;
    let spot = Signal::derive(move || {
        ctrl.open
            .mark
            .get()
            .and_then(|m| parse_spot(spots, &m.context))
    });
    let resolve = anchor_resolver(state.reader, spot);
    // A re-cut moves the mark's words with nothing scrolling.
    let invalidate = if state.reader.reflowable_now() {
        reflow_invalidation(state.reader)
    } else {
        no_invalidation()
    };
    let watch = watch_page_anchor(
        Signal::derive(move || ctrl.open.mark.get().map(|m| PageAnchor::from_mark(&m))),
        resolve,
        state.reader.viewer.zoom.display.into(),
        state.reader.viewer.scroll_top.into(),
        state.reader.viewer.page.into(),
        invalidate,
    );
    let anchor = watch.screen;

    // Reactive viewport (the shared primitive): resize-aware signal, owned
    // by this reactive scope.
    let viewport = use_viewport();
    let reduced = reduced_motion_signal();

    // Card content measurement from the invisible twin.
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

    // Snapping while compact was what made closing read as a cut.
    let snap = Signal::derive(move || {
        ctrl.drag.active.get()
            || reduced.get()
            || ctrl.geometry.gphase.get() == GlossPhase::Processing
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
