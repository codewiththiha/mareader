//! The gloss popover: the composition root.

use leptos::prelude::*;

use crate::components::ai::gloss::context_menu::GlossContextMenu;
use crate::components::ai::gloss::controller::{
    use_gloss_controller, use_open_effect, use_open_listener,
};
use crate::components::ai::gloss::drag::use_card_drag;
use crate::components::ai::gloss::gloss_surface::{
    GlossMeasureTwin, GlossSurface, GlossSurfaceContent,
};
use crate::components::ai::gloss::hooks::use_ai_chunks::use_ai_chunks;
use crate::components::ai::gloss::interactions::{
    use_dismiss_interactions, use_origin_exit_collapse, use_settle_unmount, use_zoom_reset,
};
use crate::components::ai::gloss::phase::AiPhase;
use crate::components::ai::gloss::selection_bar::GlossSelectBar;
use crate::components::ai::gloss::selection_mode::use_select_mode;
use crate::components::ai::gloss::targeting::use_card_targeting;
use crate::components::ai::gloss::undo_toast::GlossUndoToast;

#[component]
pub fn GlossAiPopover(state: crate::context::ReaderContext) -> impl IntoView {
    // ── State machine hub ─────────────────────────────────────────────
    let ctrl = use_gloss_controller(state);

    // ── Card targeting: anchor watch, viewport, spring, progress ──────
    let card = use_card_targeting(state, ctrl);

    // ── Open/close plumbing ───────────────────────────────────────────
    use_open_listener(state, ctrl);
    use_open_effect(state, ctrl, card.watch, card.spring, card.viewport);
    use_ai_chunks(state, ctrl);

    // ── Window-level behaviour ────────────────────────────────────────
    use_dismiss_interactions(ctrl);
    use_origin_exit_collapse(card.watch, ctrl);
    use_settle_unmount(ctrl, card.anchor.into(), card.sprung.into());
    use_zoom_reset(state, ctrl);

    // ── Drag physics ──────────────────────────────────────────────────
    let drag = use_card_drag(ctrl, card.expanded);

    // ── Mark management (selection mode, context menu, undo) ──────────
    let sm = use_select_mode(state, ctrl);

    // ── View ──────────────────────────────────────────────────────────
    // Surface props (unwrapped — the surface only renders while visible).
    let phase_sig = Signal::derive(move || ctrl.geometry.gphase.get());
    let box_sig = Signal::derive(move || card.sprung.get().unwrap_or_default());
    let expanded_sig = Signal::derive(move || card.expanded.get().unwrap_or_default());
    let progress_sig = Signal::derive(move || card.progress.get());
    let word_sig = Signal::derive(move || ctrl.content.word.get());
    // The POS rides the same signal the sections patch through.
    let pos_sig = Signal::derive(move || {
        ctrl.content
            .local_pos
            .get()
            .or_else(|| ctrl.content.word_info.get().map(|i| i.pos.clone()))
            .unwrap_or_default()
    });
    let density_sig = Signal::derive(move || state.settings.with(|s| s.gloss_density));

    view! {
        <GlossMeasureTwin
            node_ref=card.measure_ref
            word=word_sig
            word_info=ctrl.content.word_info
            density=density_sig
        />

        <Show when=move || ctrl.geometry.surface_visible.get() && ctrl.content.phase.get() != AiPhase::Idle>
            <GlossSurface
                phase=phase_sig
                box_=box_sig
                expanded=expanded_sig
                progress=progress_sig
                word=word_sig
                pos=pos_sig
                density=density_sig
                on_drag_start=drag.on_drag_start
            >
                <GlossSurfaceContent
                    phase=ctrl.content.phase
                    word_info=ctrl.content.word_info
                    density=density_sig
                    error=ctrl.content.error
                    retry=ctrl.commands.retry
                />
            </GlossSurface>
        </Show>

        // Mark chrome: selection bar, remove menu, undo toast.
        <GlossSelectBar state=state ctrl=ctrl undo=sm.undo />
        <GlossContextMenu state=state ctrl=ctrl menu=sm.menu undo=sm.undo />
        <GlossUndoToast state=state ctrl=ctrl undo=sm.undo />
    }
}
