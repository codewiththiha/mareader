//! The gloss state-machine hub. The controller groups its twenty-odd signals
//! into cohesive slices — [`GlossContent`] (what the model is doing),
//! [`GlossGeometry`] (what the card's box is doing), [`GlossOpen`] (which
//! mark the card belongs to), [`GlossDrag`] (pointer state) and
//! [`GlossCache`] (session answers) — so a hook that needs one concern takes
//! one slice instead of the whole flat field list, and the shared behaviours
//! ([`GlossCommands`]: reset, collapse, mark persistence, retry) are clearly
//! commands rather than more state.
//!
//! The open path is a named state machine: [`open_verdict`] decides what a
//! request means given the controller's state (stale flag / swallow /
//! collapse / open), and the open effect only dispatches on that verdict —
//! no five-deep early-return chain. [`begin_open`], [`serve_cached`] and
//! [`begin_fetch`] are the three transitions that actually move state.
//!
//! Open requests are deterministic: both the Info pill and a saved stroke
//! dispatch `pdfreader:gloss-open` with the mark in the event detail. The
//! listener ([`use_open_listener`]) sets `pending` and bumps `request`
//! (which the open effect tracks), so the effect always runs with a mark in
//! hand — never a race against `detail` being cleared or a stale
//! `popover_open = true` no-op.

use std::collections::HashMap;

use leptos::prelude::*;
use pdf_core::gloss::{GlossBox, GlossMark};

use crate::components::ai::anchor::AnchorWatch;
use crate::components::ai::gloss::mark_layer::GLOSS_OPEN_EVENT;
use crate::components::ai::types::{AiError, AiErrorKind, AiPhase, GlossPhase, WordInfo};
use crate::components::primitives::hooks::use_viewport::viewport_size;
use crate::components::primitives::motion::spring::SpringBox;
use crate::services::ai::invoke_explain_word;
use crate::state::AppState;

/// Per-document cap on persisted marks (oldest evicted). A reading session's
/// worth of looked-up words, bounded so localStorage can't grow without end.
pub const MARK_CAP: usize = 200;

/// The data phase + payload of the open card: what the *model* is doing,
/// independent of the card's geometry.
#[derive(Clone, Copy)]
pub struct GlossContent {
    pub phase: RwSignal<AiPhase>,
    pub word: RwSignal<String>,
    pub word_info: RwSignal<Option<WordInfo>>,
    /// The typed failure behind `AiPhase::Error`, if any. Drives both the
    /// friendly message and the retry affordance in the surface.
    pub error: RwSignal<Option<AiError>>,
}

/// The geometry phase of the surface: where the card's *box* is in its
/// morph lifecycle, and whether the surface exists at all.
#[derive(Clone, Copy)]
pub struct GlossGeometry {
    pub gphase: RwSignal<GlossPhase>,
    /// Whether the morphing surface exists at all. Distinct from
    /// `popover_open`: during processing the stroke IS the UI, and after the
    /// outro morph the surface unmounts while the gloss stays "open" on its
    /// mark.
    pub surface_visible: RwSignal<bool>,
    /// Whether the origin-exit watcher is armed for this open. A card opened
    /// near the bottom edge starts unarmed (its origin is already past
    /// CARD_EXIT_FRAC) so it is not instantly collapsed; it arms the first
    /// time the origin is inside the band, and only then can the band close it.
    pub exit_armed: RwSignal<bool>,
}

/// The open plumbing: which persisted mark the card belongs to, the mark
/// queued by the latest request, and the request nonce that re-runs the
/// open effect even when the popover is already open.
#[derive(Clone, Copy)]
pub struct GlossOpen {
    /// The mark the open card belongs to (None while closed).
    pub mark: RwSignal<Option<GlossMark>>,
    /// The mark queued by the most recent open request, consumed by the
    /// open effect.
    pub pending: RwSignal<Option<GlossMark>>,
    /// Monotonic request counter — tracking it is what makes a second open
    /// of an already-open popover re-run the open effect.
    pub request: RwSignal<u64>,
}

/// Drag state for the expanded card (the pointer physics live in
/// [`super::drag`]).
#[derive(Clone, Copy)]
pub struct GlossDrag {
    /// Anchor-relative offset of the dragged card (None = not dragged).
    pub offset: RwSignal<Option<(f64, f64)>>,
    /// Whether a drag is in progress (snaps the spring while true).
    pub active: RwSignal<bool>,
    /// Grab offset within the card, live only during a drag.
    pub grab: StoredValue<Option<(f64, f64)>, LocalStorage>,
}

/// Answers already fetched this session, keyed by mark id. Re-opening a
/// stroke is recall, not a rescan.
#[derive(Clone, Copy)]
pub struct GlossCache {
    answers: StoredValue<HashMap<String, WordInfo>, LocalStorage>,
}

impl GlossCache {
    fn new() -> Self {
        Self {
            answers: StoredValue::new_local(HashMap::new()),
        }
    }

    /// The cached answer for a mark id, if this session already fetched it.
    pub fn get(&self, id: &str) -> Option<WordInfo> {
        self.answers.with_value(|c| c.get(id).cloned())
    }

    /// Record a finished answer.
    pub fn insert(&self, id: String, info: WordInfo) {
        self.answers.update_value(|c| {
            c.insert(id, info);
        });
    }

    /// Drop one answer — a failed run must not leave a stale partial
    /// snapshot behind for the mark's next open to recall.
    pub fn remove(&self, id: &str) {
        self.answers.update_value(|c| {
            c.remove(id);
        });
    }

    /// Evict the answers of removed marks, so re-opening them re-requests
    /// instead of recalling an answer for a highlight that no longer exists.
    pub fn evict(&self, marks: &[GlossMark]) {
        self.answers.update_value(|c| {
            for m in marks {
                c.remove(&m.id);
            }
        });
    }
}

/// The shared behaviours: every path funnels through these instead of
/// re-implementing a close or a persistence dance.
#[derive(Clone, Copy)]
pub struct GlossCommands {
    /// Full dismiss back to Idle (keeps the mark — the highlight reopens it).
    pub reset: Callback<()>,
    /// The outro: fold the expanded card back down onto the word.
    pub collapse_to_mark: Callback<()>,
    /// Record + persist a freshly captured mark, returning the CANONICAL one
    /// (re-explaining the same word at the same spot reuses the existing
    /// mark rather than stacking a second stroke on it).
    pub add_mark: Callback<GlossMark, GlossMark>,
    /// Remove marks by id: persist, evict their cached answers, close the
    /// card if it belonged to one of them. Returns the removed marks so the
    /// caller can park them for undo.
    pub remove_marks: Callback<Vec<String>, Vec<GlossMark>>,
    /// Re-insert previously removed marks (the Undo path) and persist.
    pub restore_marks: Callback<Vec<GlossMark>>,
    /// Retry the current mark after a retryable failure.
    pub retry: Callback<()>,
}

/// All gloss state + the shared behaviours, grouped into cohesive slices.
/// Every field is `Copy`, so hooks can take the controller — or just the
/// slice they need — by value without lifetime gymnastics.
#[derive(Clone, Copy)]
pub struct GlossController {
    pub content: GlossContent,
    pub geometry: GlossGeometry,
    pub open: GlossOpen,
    pub drag: GlossDrag,
    pub cache: GlossCache,
    pub commands: GlossCommands,
}

pub fn use_gloss_controller(state: AppState) -> GlossController {
    let popover_open = state.reader.ai_selection.popover_open;
    let processing_id = state.reader.gloss.processing_id;
    let marks = state.reader.gloss.marks;
    let doc_path = state.reader.document.path;

    let content = GlossContent {
        phase: RwSignal::new(AiPhase::Idle),
        word: RwSignal::new(String::new()),
        word_info: RwSignal::new(None::<WordInfo>),
        error: RwSignal::new(None::<AiError>),
    };
    let geometry = GlossGeometry {
        gphase: RwSignal::new(GlossPhase::Processing),
        surface_visible: RwSignal::new(false),
        exit_armed: RwSignal::new(false),
    };
    let open = GlossOpen {
        mark: RwSignal::new(None::<GlossMark>),
        pending: RwSignal::new(None::<GlossMark>),
        request: RwSignal::new(0u64),
    };
    let drag = GlossDrag {
        offset: RwSignal::new(None::<(f64, f64)>),
        active: RwSignal::new(false),
        grab: StoredValue::new_local(None::<(f64, f64)>),
    };
    let cache = GlossCache::new();

    // Full dismiss back to Idle. NOTE: the mark itself is intentionally kept
    // — the highlight is the point, and it is what reopens this card later.
    let reset = Callback::new(move |_| {
        popover_open.set(false);
        content.phase.set(AiPhase::Idle);
        content.word.set(String::new());
        content.word_info.set(None);
        content.error.set(None);
        geometry.gphase.set(GlossPhase::Processing);
        geometry.surface_visible.set(false);
        geometry.exit_armed.set(false);
        processing_id.set(None);
        open.mark.set(None);
        drag.offset.set(None);
        drag.active.set(false);
        drag.grab.set_value(None);
    });

    // The outro: fold the expanded card back down onto the word. Every close
    // path funnels through here, and the popover's settle watcher unmounts
    // the surface once the spring has actually landed on the stroke.
    let collapse_to_mark = Callback::new(move |_| {
        if geometry.gphase.get_untracked() != GlossPhase::Expanded || drag.active.get_untracked() {
            return;
        }
        drag.offset.set(None);
        geometry.gphase.set(GlossPhase::Compact);
    });

    // Record + persist a freshly captured mark, and hand back the CANONICAL
    // one. Returning it matters — the id is what keys the processing glow
    // and the answer cache, so the caller must not go on holding the
    // discarded duplicate.
    let add_mark = Callback::new(move |m: GlossMark| -> GlossMark {
        let existing = marks.with_untracked(|v| {
            v.iter().find(|o| o.same_spot(&m)).cloned()
        });
        if let Some(existing) = existing {
            return existing;
        }
        marks.update(|v| {
            v.push(m.clone());
            if v.len() > MARK_CAP {
                v.remove(0);
            }
        });
        if let Some(path) = doc_path.get_untracked() {
            crate::storage::persist_gloss(&path, &marks.get_untracked());
        }
        m
    });

    // The single removal path: context menu, selection bar, anything later.
    // Persist first, evict the session cache, then close the card if it
    // belonged to one of the removed marks. Hands the batch back for undo.
    let remove_marks = Callback::new(move |ids: Vec<String>| -> Vec<GlossMark> {
        if ids.is_empty() {
            return Vec::new();
        }
        let id_set: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
        let mut removed = Vec::new();
        marks.update(|v| {
            let mut keep = Vec::with_capacity(v.len());
            for m in v.drain(..) {
                if id_set.contains(m.id.as_str()) {
                    removed.push(m);
                } else {
                    keep.push(m);
                }
            }
            *v = keep;
        });
        if removed.is_empty() {
            return removed;
        }
        if let Some(path) = doc_path.get_untracked() {
            crate::storage::persist_gloss(&path, &marks.get_untracked());
        }
        cache.evict(&removed);
        if open
            .mark
            .get_untracked()
            .is_some_and(|current| id_set.contains(current.id.as_str()))
        {
            reset.run(());
        }
        removed
    });

    // Undo: re-insert (id-deduped) and persist. The session cache stays
    // evicted — the next open of a restored mark re-fetches, which is the
    // honest behaviour for a word whose answer might have improved.
    let restore_marks = Callback::new(move |restored: Vec<GlossMark>| {
        if restored.is_empty() {
            return;
        }
        marks.update(|v| {
            for m in restored {
                if !v.iter().any(|o| o.id == m.id) {
                    v.push(m);
                }
            }
        });
        if let Some(path) = doc_path.get_untracked() {
            crate::storage::persist_gloss(&path, &marks.get_untracked());
        }
    });

    // Retry the current mark after a retryable failure: the same opening
    // ritual minus persistence (the mark is already canonical), so the stroke
    // thinks again and the surface is reborn on the first fresh chunk.
    let retry = Callback::new(move |_| {
        let Some(mark) = open.mark.get_untracked() else {
            return;
        };
        if !pdf_engine::has_tauri() {
            return; // the environment cannot change mid-session
        }
        content.error.set(None);
        content.word_info.set(None);
        content.phase.set(AiPhase::Processing);
        geometry.gphase.set(GlossPhase::Processing);
        geometry.surface_visible.set(false);
        processing_id.set(Some(mark.id.clone()));
        invoke_explain_word(mark.word, mark.context);
    });

    GlossController {
        content,
        geometry,
        open,
        drag,
        cache,
        commands: GlossCommands {
            reset,
            collapse_to_mark,
            add_mark,
            remove_marks,
            restore_marks,
            retry,
        },
    }
}

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
    info: WordInfo,
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

    if pdf_engine::has_tauri() {
        invoke_explain_word(mark.word, mark.context);
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
            page,
            word: word.to_string(),
            context: String::new(),
            rect: GlossBox {
                x,
                y: 100.0,
                w: 40.0,
                h: 12.0,
                r: 6.0,
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
