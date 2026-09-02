//! The open path, wired up.
//!
//! Every open (a stroke click or the selection Info pill) arrives as a
//! `pdfreader:gloss-open` CustomEvent carrying the mark, which
//! [`use_open_listener`] turns into a pending mark plus a bumped request
//! nonce. [`use_open_effect`] tracks that nonce, asks [`open_verdict`] what
//! the request means given the card's current state, and dispatches to one of
//! three named transitions — [`begin_open`], [`serve_cached`],
//! [`begin_fetch`]. Keeping the decision pure and separate from the
//! transitions is what replaced a five-deep early-return chain with a table
//! that can be read (and tested) at a glance.

use std::sync::Arc;

use ai_core::gloss::{GlossBox, GlossMark};
use leptos::prelude::*;

use crate::components::ai::anchor::AnchorWatch;
use crate::components::ai::gloss::mark_layer::GLOSS_OPEN_EVENT;
use crate::components::ai::types::{AiError, AiErrorKind, AiPhase, GlossPhase, WordInfo};
use app_chrome::hooks::use_viewport::viewport_size;
use crate::components::primitives::motion::spring::SpringBox;
use crate::services::ai::invoke_explain_word;
use crate::state::AppState;

use super::GlossController;

/// Every open (stroke click OR Info pill) arrives as a CustomEvent that
/// carries the mark and bumps the nonce. Tracking `request` is what makes a
/// second open of an already-open popover re-run the open effect.
pub fn use_open_listener(state: AppState, ctrl: GlossController) {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;

    let handle = window_event_listener(
        leptos::ev::Custom::new(GLOSS_OPEN_EVENT),
        move |ev: web_sys::CustomEvent| {
            let Ok(m) = serde_wasm_bindgen::from_value::<GlossMark>(ev.detail()) else {
                return;
            };
            detail.set(None);
            state.reader.ai_selection.anchor.set(None);
            ctrl.open.pending.set(Some(m));
            ctrl.open.request.update(|n| *n += 1);
            popover_open.set(true);
        },
    );
    on_cleanup(move || handle.remove());
}

/// What an open request means, given the controller's state when it lands.
/// Pure: the open effect reads its inputs untracked and dispatches on this,
/// so the decision table is named, exhaustive, and unit-tested.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum OpenVerdict {
    /// `popover_open` is set but no request is pending — a stale flag (e.g.
    /// a remount after a document switch). Clear it, don't sit on it.
    ClearFlag,
    /// The request is for the mark whose run is still thinking: swallow the
    /// re-click (a second request would restart or duplicate the run).
    Swallow,
    /// The request is for the mark expanded on screen: fold it back down
    /// (toggle semantics live here, and only here — marks stay dumb open
    /// dispatchers).
    Collapse,
    /// Adopt the request: run the opening ritual, then serve it from the
    /// cache or the backend. Compact or mid-outro re-clicks land here too,
    /// deliberately — they recall/reopen.
    Open,
}

fn open_verdict(
    pending: Option<&GlossMark>,
    current: Option<&GlossMark>,
    gphase: GlossPhase,
    surface_visible: bool,
) -> OpenVerdict {
    let Some(pending) = pending else {
        return OpenVerdict::ClearFlag;
    };
    if current.is_some_and(|m| m.same_spot(pending)) {
        match (gphase, surface_visible) {
            (GlossPhase::Processing, _) => return OpenVerdict::Swallow,
            (GlossPhase::Expanded, true) => return OpenVerdict::Collapse,
            _ => {}
        }
    }
    OpenVerdict::Open
}

/// The opening ritual every fresh open runs: canonicalize + persist the
/// mark, adopt it as current, re-derive the anchor NOW (same tick) and
/// re-anchor the spring onto THIS word — every open morphs out of its own
/// mark, never out of the previous card's resting place — and clear the
/// transient state (native selection, drag offset, exit arming). Returns
/// the CANONICAL mark.
fn begin_open(
    state: &AppState,
    ctrl: GlossController,
    watch: &AnchorWatch,
    spring: &SpringBox<GlossBox>,
    viewport: RwSignal<(f64, f64)>,
    mark: GlossMark,
) -> GlossMark {
    // Self-contained open: mark is already in hand (Info pill or stroke
    // click). Persist it so re-open/re-explain reuse the id.
    let mark = ctrl.commands.add_mark.run(mark);

    ctrl.open.pending.set(None);
    // Whatever the previous card was waiting on, this card is not: a run in
    // flight for the last word must not answer into this one.
    ctrl.open.end_run();
    state.reader.ai_selection.detail.set(None);
    state.reader.ai_selection.anchor.set(None);
    ctrl.open.mark.set(Some(mark.clone()));

    watch.refresh.run(());
    if let Some(a) = watch.screen.get_untracked() {
        spring.reset_to.run(a);
    }

    ctrl.content.word.set(mark.word.clone());
    viewport.set(viewport_size());
    ctrl.drag.offset.set(None);
    ctrl.drag.active.set(false);
    ctrl.geometry.exit_armed.set(false);

    // Exactly one highlighter: the native tint goes the moment the stroke
    // takes over (it would also fight the card's own text selection).
    if let Some(Some(s)) = web_sys::window().and_then(|w| w.get_selection().ok()) {
        let _ = s.remove_all_ranges();
    }

    mark
}

/// Recall, not rescan: a stroke whose answer is already cached morphs
/// straight back open, with no request and no shimmer.
fn serve_cached(
    ctrl: GlossController,
    processing_id: RwSignal<Option<String>>,
    info: Arc<WordInfo>,
) {
    ctrl.content.word_info.set(Some(info));
    ctrl.content.error.set(None);
    ctrl.content.phase.set(AiPhase::Done);
    processing_id.set(None);
    ctrl.geometry.gphase.set(GlossPhase::Expanded);
    ctrl.geometry.surface_visible.set(true);
}

/// Fresh (or retried) explain. No surface while thinking: the highlighter
/// stroke is the only processing UI, so nothing is stacked over the word.
fn begin_fetch(
    ctrl: GlossController,
    processing_id: RwSignal<Option<String>>,
    mark: GlossMark,
) {
    ctrl.content.word_info.set(None);
    ctrl.content.error.set(None);
    ctrl.content.phase.set(AiPhase::Processing);
    ctrl.geometry.gphase.set(GlossPhase::Processing);
    ctrl.geometry.surface_visible.set(false);
    processing_id.set(Some(mark.id.clone()));

    if tauri_bridge::has_tauri() {
        let run = ctrl.open.begin_run(&mark.id);
        invoke_explain_word(mark.word, mark.context, run);
    } else {
        // The environment cannot change mid-session: this is a terminal,
        // non-retryable state, shown as an expanded error card.
        ctrl.content.error.set(Some(AiError {
            kind: AiErrorKind::Other("desktop-only".into()),
            message: "AI explanations are only available in the desktop app.".into(),
            retryable: false,
        }));
        ctrl.content.phase.set(AiPhase::Error);
        processing_id.set(None);
        ctrl.geometry.gphase.set(GlossPhase::Expanded);
        ctrl.geometry.surface_visible.set(true);
    }
}

/// The open effect: re-runs on EVERY request (nonce). Reads the state
/// untracked, asks [`open_verdict`] what the request means, and dispatches —
/// the branches are one screenful and each names its transition.
pub fn use_open_effect(
    state: AppState,
    ctrl: GlossController,
    watch: AnchorWatch,
    spring: SpringBox<GlossBox>,
    viewport: RwSignal<(f64, f64)>,
) {
    let popover_open = state.reader.ai_selection.popover_open;
    let processing_id = state.reader.gloss.processing_id;

    Effect::new(move |_| {
        let _ = ctrl.open.request.get(); // tracked nonce
        if !popover_open.get() {
            return;
        }

        let pending = ctrl.open.pending.get_untracked();
        let current = ctrl.open.mark.get_untracked();
        let verdict = open_verdict(
            pending.as_ref(),
            current.as_ref(),
            ctrl.geometry.gphase.get_untracked(),
            ctrl.geometry.surface_visible.get_untracked(),
        );

        match verdict {
            OpenVerdict::ClearFlag => popover_open.set(false),
            OpenVerdict::Swallow => ctrl.open.pending.set(None),
            OpenVerdict::Collapse => {
                ctrl.open.pending.set(None);
                ctrl.commands.collapse_to_mark.run(());
            }
            OpenVerdict::Open => {
                let Some(pending) = pending else {
                    return;
                };
                let mark = begin_open(&state, ctrl, &watch, &spring, viewport, pending);
                match ctrl.cache.get(&mark.id) {
                    Some(info) => serve_cached(ctrl, processing_id, info),
                    None => begin_fetch(ctrl, processing_id, mark),
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(page: u32, word: &str, x: f64) -> GlossMark {
        GlossMark {
            id: format!("g{page}-{x}"),
            word: word.to_string(),
            context: String::new(),
            anchor: ai_core::gloss::PageAnchor {
                page,
                rect: GlossBox {
                    x,
                    y: 100.0,
                    w: 40.0,
                    h: 12.0,
                    r: 6.0,
                },
            },
        }
    }

    #[test]
    fn no_pending_mark_means_a_stale_flag() {
        assert_eq!(
            open_verdict(None, Some(&mark(3, "word", 10.0)), GlossPhase::Expanded, true),
            OpenVerdict::ClearFlag
        );
    }

    #[test]
    fn a_reclick_while_processing_is_swallowed() {
        let m = mark(3, "word", 10.0);
        assert_eq!(
            open_verdict(Some(&m), Some(&m), GlossPhase::Processing, false),
            OpenVerdict::Swallow
        );
        // Even with a stray visible surface.
        assert_eq!(
            open_verdict(Some(&m), Some(&m), GlossPhase::Processing, true),
            OpenVerdict::Swallow
        );
    }

    #[test]
    fn a_reclick_on_the_expanded_card_collapses_it() {
        let m = mark(3, "word", 10.0);
        assert_eq!(
            open_verdict(Some(&m), Some(&m), GlossPhase::Expanded, true),
            OpenVerdict::Collapse
        );
    }

    #[test]
    fn compact_or_unmounting_reclicks_reopen() {
        let m = mark(3, "word", 10.0);
        // Compact chip: recall/reopen.
        assert_eq!(
            open_verdict(Some(&m), Some(&m), GlossPhase::Compact, true),
            OpenVerdict::Open
        );
        // Expanded phase but the surface already unmounted (mid-outro).
        assert_eq!(
            open_verdict(Some(&m), Some(&m), GlossPhase::Expanded, false),
            OpenVerdict::Open
        );
    }

    #[test]
    fn a_different_mark_always_opens() {
        let a = mark(3, "word", 10.0);
        let b = mark(3, "other", 80.0);
        let c = mark(4, "word", 10.0);
        for gphase in [GlossPhase::Processing, GlossPhase::Expanded, GlossPhase::Compact] {
            assert_eq!(open_verdict(Some(&b), Some(&a), gphase, true), OpenVerdict::Open);
            assert_eq!(open_verdict(Some(&c), Some(&a), gphase, true), OpenVerdict::Open);
        }
    }

    #[test]
    fn a_same_spot_duplicate_is_the_same_mark_for_verdict_purposes() {
        // add_mark canonicalizes same-spot duplicates; the verdict must
        // treat them as the current mark (same page/word/rect, different id).
        let current = mark(3, "word", 10.0);
        let duplicate = GlossMark {
            id: "g3-999".into(),
            ..current.clone()
        };
        assert_eq!(
            open_verdict(Some(&duplicate), Some(&current), GlossPhase::Expanded, true),
            OpenVerdict::Collapse
        );
    }
}
