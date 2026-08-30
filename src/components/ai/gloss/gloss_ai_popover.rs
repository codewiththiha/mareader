//! The gloss popover: the composition root. The state machine lives in
//! [`controller`], card targeting in [`targeting`], placement math in
//! [`placement`], pointer physics in [`drag`], window behaviour in
//! [`interactions`], and stream/measure hooks in [`hooks`]. This module
//! wires those together and renders: the measure twin, the morphing
//! surface with its phase-driven content, and the mark-management chrome.
//!
//! Two orthogonal phases run together:
//! * the **geometry phase** ([`GlossPhase`]): stroke → expanded card → chip;
//! * the **data phase** ([`AiPhase`]): processing → streaming → done/error.
//!
//! Four things are load-bearing here and easy to undo by accident:
//!
//! * **The anchor is a [`GlossMark`], never a DOM node.** Every scroll, zoom,
//!   page or mode change re-projects the mark's page-space rect through
//!   whichever host currently renders that page, so the card sticks to the
//!   *text* even across the virtualizer's unmounts. The mark is written to
//!   localStorage at capture time and is deliberately NOT removed on dismiss:
//!   closing the card leaves the word highlighted, and clicking that highlight
//!   re-opens the card through the same spring.
//! * **There is exactly one highlighter at a time.** The native `::selection`
//!   tint is cleared the moment the gloss takes over; while the model works
//!   there is NO surface at all (the in-page stroke pulses);
//!   and after the outro the surface unmounts once it has settled onto the
//!   stroke, so a chip can never sit on top of the mark it came from.
//! * **Every close is an outro, not a cut.** Origin-exit, Escape and outside
//!   clicks all run `collapse_to_mark`, and the spring is NOT snapped while
//!   compact, so the card visibly morphs back down onto the word before
//!   handing over to the persisted stroke.
//! * **Re-opening is recall, not a rescan.** Snapshots are cached by mark id,
//!   so clicking a stroke morphs the card open on `AiPhase::Done` content
//!   without touching the backend. The spring is hard-reset onto the new
//!   word's mark on every open so the morph never flies in from the previous
//!   card's resting place.

use leptos::prelude::*;

use crate::components::ai::gloss::controller::{
    use_gloss_controller, use_open_effect, use_open_listener,
};
use crate::components::ai::gloss::drag::use_card_drag;
use crate::components::ai::gloss::gloss_surface::{GlossMeasureTwin, GlossSurface, GlossSurfaceContent};
use crate::components::ai::gloss::hooks::use_ai_chunks::use_ai_chunks;
use crate::components::ai::gloss::interactions::{
    use_dismiss_interactions, use_origin_exit_collapse, use_page_flip_collapse, use_settle_unmount,
    use_zoom_reset,
};
use crate::components::ai::gloss::selection_bar::GlossSelectBar;
use crate::components::ai::gloss::selection_mode::use_select_mode;
use crate::components::ai::gloss::targeting::use_card_targeting;
use crate::components::ai::gloss::context_menu::GlossContextMenu;
use crate::components::ai::gloss::undo_toast::GlossUndoToast;
use crate::components::ai::types::AiPhase;
use crate::state::AppState;

#[component]
pub fn GlossAiPopover(state: AppState) -> impl IntoView {
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
    use_page_flip_collapse(state, ctrl);
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
    // The header's part of speech rides the same signal the sections patch
    // through, so a snapshot that fills the POS lands in both places in one
    // frame.
    let pos_sig = Signal::derive(move || {
        ctrl.content.word_info.get().map(|i| i.pos).unwrap_or_default()
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

        // Mark management chrome: the bottom-right selection bar, the
        // right-click remove menu, and the undo toast. All three sit above
        // the expanded surface (z 50): bar 60, menu/toast 70.
        <GlossSelectBar state ctrl undo=sm.undo />
        <GlossContextMenu state ctrl menu=sm.menu undo=sm.undo />
        <GlossUndoToast state ctrl undo=sm.undo />
    }
}

