//! The shared behaviours. Every path funnels through these instead of
//! re-implementing a close, a persistence dance or a retry.

use ai_core::gloss::GlossMark;
use leptos::prelude::*;

use crate::components::ai::types::{AiPhase, GlossPhase};
use crate::services::ai::invoke_explain_word;
use crate::state::AppState;

use super::cache::GlossCache;
use super::content::GlossContent;
use super::drag::GlossDrag;
use super::geometry::GlossGeometry;
use super::open::GlossOpen;
use super::MARK_CAP;

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

/// Build the commands over a controller's slices. Split from the slices
/// themselves because these are behaviour, not state: they are the only place
/// that writes to more than one slice at a time, and to the document's
/// persisted marks.
pub(super) fn build_commands(
    state: AppState,
    content: GlossContent,
    geometry: GlossGeometry,
    open: GlossOpen,
    drag: GlossDrag,
    cache: GlossCache,
) -> GlossCommands {
    let popover_open = state.reader.ai_selection.popover_open;
    let processing_id = state.reader.gloss.processing_id;
    let marks = state.reader.gloss.marks;
    let doc_path = state.reader.document.path;

    // Full dismiss back to Idle. NOTE: the mark itself is intentionally kept
    // — the highlight is the point, and it is what reopens this card later.
    let reset = Callback::new(move |_| {
        popover_open.set(false);
        content.clear();
        geometry.clear();
        drag.clear();
        processing_id.set(None);
        open.mark.set(None);
        // A dismissed card has no run to wait on: a late chunk from the run
        // it abandoned must not reopen it.
        open.end_run();
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
        let mut evicted = None;
        marks.update(|v| {
            v.push(m.clone());
            if v.len() > MARK_CAP {
                evicted = Some(v.remove(0));
            }
        });
        // The cap drops the oldest mark; its answer must go with it, or the
        // session cache outlives every stroke it belongs to and grows without
        // the bound MARK_CAP exists to impose.
        if let Some(old) = evicted {
            cache.remove(&old.id);
        }
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
        if !tauri_bridge::has_tauri() {
            return; // the environment cannot change mid-session
        }
        content.error.set(None);
        content.word_info.set(None);
        content.phase.set(AiPhase::Processing);
        geometry.gphase.set(GlossPhase::Processing);
        geometry.surface_visible.set(false);
        processing_id.set(Some(mark.id.clone()));
        // A retry is a NEW run: the failed one's late chunks are no longer
        // this card's business.
        let run = open.begin_run(&mark.id);
        invoke_explain_word(mark.word, mark.context, run);
    });

    GlossCommands {
        reset,
        collapse_to_mark,
        add_mark,
        remove_marks,
        restore_marks,
        retry,
    }
}
