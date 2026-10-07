//! The open path: a `mareader:gloss-open` event becomes a pending mark,
//! then a verdict dispatches.

use std::sync::Arc;

use ai_core::gloss::{GlossBox, GlossMark};
use ai_core::types::{AiError, AiErrorKind, WordInfo};
use leptos::prelude::*;

use crate::components::ai::anchor::AnchorWatch;
use crate::components::ai::gloss::mark_layer::GLOSS_OPEN_EVENT;
use crate::components::ai::gloss::phase::{AiPhase, GlossPhase};
use crate::context::ReaderContext;
use crate::pane::origin::raised_in;
use crate::services::ai::invoke_explain_word;
use app_chrome::hooks::use_viewport::viewport_size;
use app_ui::components::primitives::hooks::use_custom_event::use_typed_event_from;
use app_ui::components::primitives::motion::spring::SpringBox;

use super::GlossController;
use super::content::GlossContent;
use super::geometry::GlossGeometry;
use super::open::GlossOpen;

/// An open arrives as a CustomEvent carrying the mark and bumping the
/// nonce.
pub fn use_open_listener(state: crate::context::ReaderContext, ctrl: GlossController) {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;

    // Raised on the stroke or pill that asked, bubbling to every pane.
    use_typed_event_from::<GlossMark>(GLOSS_OPEN_EVENT, move |m, origin| {
        if !raised_in(&state.reader.dom, origin.as_ref()) {
            return;
        }
        detail.set(None);
        state.reader.ai_selection.anchor.set(None);
        ctrl.open.pending.set(Some(m));
        ctrl.open.request.update(|n| *n += 1);
        popover_open.set(true);
    });
}

/// What an open request means, given the controller's state; pure.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum OpenVerdict {
    /// A stale flag (a remount after a document switch): clear it.
    ClearFlag,
    /// The request is for the mark whose run is still thinking: swallow.
    Swallow,
    /// The request is for the mark on screen: fold it back down.
    Collapse,
    /// Adopt the request: open, then serve from cache or backend.
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

/// The opening ritual: canonicalize, adopt, re-anchor the spring, clear
/// transients.
fn begin_open(
    state: &ReaderContext,
    ctrl: GlossController,
    watch: &AnchorWatch,
    spring: &SpringBox<GlossBox>,
    viewport: RwSignal<(f64, f64)>,
    mark: GlossMark,
) -> GlossMark {
    // Self-contained open: the mark is in hand; persist it.
    let mark = ctrl.commands.add_mark.run(mark);

    ctrl.open.pending.set(None);
    // A run in flight for the last word must not answer into this one.
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

    // Exactly one highlighter: the native tint goes when the stroke takes
    // over.
    if let Some(Some(s)) = web_sys::window().and_then(|w| w.get_selection().ok()) {
        let _ = s.remove_all_ranges();
    }

    mark
}

/// Recall, not rescan: a cached answer morphs straight back open.
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

/// Fresh or retried explain; the stroke is the only processing UI.
pub(super) fn begin_fetch(
    content: GlossContent,
    geometry: GlossGeometry,
    open: GlossOpen,
    processing_id: RwSignal<Option<String>>,
    mark: GlossMark,
) {
    content.word_info.set(None);
    content.error.set(None);
    content.phase.set(AiPhase::Processing);
    geometry.gphase.set(GlossPhase::Processing);
    geometry.surface_visible.set(false);
    processing_id.set(Some(mark.id.clone()));

    if tauri_bridge::has_tauri() {
        let run = open.begin_run(&mark.id);
        // The sentence, not the envelope: the model wants only the prose.
        let explain = crate::components::ai::reflow_anchor::explain_context(&mark);
        invoke_explain_word(mark.word, explain, run);
    } else {
        // Terminal, non-retryable: shown as an expanded error card.
        content.error.set(Some(AiError {
            kind: AiErrorKind::Other("desktop-only".into()),
            message: "AI explanations are only available in the desktop app.".into(),
            retryable: false,
        }));
        content.phase.set(AiPhase::Error);
        processing_id.set(None);
        geometry.gphase.set(GlossPhase::Expanded);
        geometry.surface_visible.set(true);
    }
}

/// The open effect: re-runs on every nonce and dispatches the verdict.
pub fn use_open_effect(
    state: crate::context::ReaderContext,
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
                    None => {
                        begin_fetch(ctrl.content, ctrl.geometry, ctrl.open, processing_id, mark)
                    }
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
            open_verdict(
                None,
                Some(&mark(3, "word", 10.0)),
                GlossPhase::Expanded,
                true
            ),
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
        for gphase in [
            GlossPhase::Processing,
            GlossPhase::Expanded,
            GlossPhase::Compact,
        ] {
            assert_eq!(
                open_verdict(Some(&b), Some(&a), gphase, true),
                OpenVerdict::Open
            );
            assert_eq!(
                open_verdict(Some(&c), Some(&a), gphase, true),
                OpenVerdict::Open
            );
        }
    }

    #[test]
    fn a_same_spot_duplicate_is_the_same_mark_for_verdict_purposes() {
        // `add_mark` canonicalizes same-spot duplicates.
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
