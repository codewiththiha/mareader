//! The shared behaviours. Every path funnels through these instead of
//! re-implementing a close, a persistence dance or a retry.

use ai_core::gloss::GlossMark;
use leptos::prelude::*;

use crate::components::ai::gloss::phase::GlossPhase;
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

/// Whether two marks denote the same glossed spot, across both formats.
///
/// A PDF's spot IS its page-space rect, so the anchor's own tolerance is the
/// whole answer. A reflowable mark's rect is only the box it happened to be
/// captured in — viewport pixels, which move when the reader scrolls — so
/// comparing rects there would stack a second stroke on the same word the
/// moment the page moved. Two reflowable marks are the same spot when their
/// envelopes are: same block, same characters.
fn same_glossed_spot(a: &GlossMark, b: &GlossMark) -> bool {
    if a.word != b.word {
        return false;
    }
    let (left, right) = (
        crate::components::ai::reflow_anchor::parse_spot(&a.context),
        crate::components::ai::reflow_anchor::parse_spot(&b.context),
    );
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        // One carries a spot and the other does not: different pipelines, and
        // nothing honest to compare but the anchors.
        _ => a.same_spot(b),
    }
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

    // The marks' one write to storage. There is no per-mark write: the stored
    // shape is a document's list, so every mutation of it persists the whole
    // list, and a document with no path (nothing open) has nowhere to put it.
    // Captures two signals, so it is `Copy` and each command below takes its
    // own copy.
    let persist = move || {
        if let Some(path) = doc_path.get_untracked() {
            crate::storage::persist_gloss(&path, &marks.get_untracked());
        }
    };

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
            v.iter().find(|o| same_glossed_spot(o, &m)).cloned()
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
        persist();
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
        persist();
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
        persist();
    });

    // Retry the current mark after a retryable failure: the same opening
    // ritual minus persistence (the mark is already canonical), so the stroke
    // thinks again and the surface is reborn on the first fresh chunk.
    let retry = Callback::new(move |_| {
        let Some(mark) = open.mark.get_untracked() else {
            return;
        };
        if !tauri_bridge::has_tauri() {
            // The environment cannot change mid-session, and the desktop-only
            // verdict `begin_fetch` would reach is not retryable — so the
            // button that got us here cannot be showing. Leave the card alone.
            return;
        }
        // A retry is a NEW run of the same opening ritual: `begin_fetch` starts
        // the run (the failed one's late chunks are no longer this card's
        // business), clears the last answer, and puts the stroke back into
        // thinking. Persistence is deliberately not repeated — the mark is
        // already canonical, which is why this goes through the transition and
        // not through the open path.
        super::wiring::begin_fetch(content, geometry, open, processing_id, mark);
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
#[cfg(test)]
mod tests {
    use ai_core::gloss::{GlossBox, PageAnchor, ReflowSpot};

    use super::*;
    use crate::components::ai::reflow_anchor::spot_envelope;

    fn anchor(page: u32, x: f64, y: f64) -> PageAnchor {
        PageAnchor {
            page,
            rect: GlossBox { x, y, w: 40.0, h: 12.0, r: 0.0 },
        }
    }

    fn mark(id: &str, word: &str, context: &str, anchor: PageAnchor) -> GlossMark {
        GlossMark { id: id.to_string(), word: word.to_string(), context: context.to_string(), anchor }
    }

    #[test]
    fn a_pdf_mark_is_the_same_spot_within_the_anchor_tolerance() {
        let a = mark("g1", "palimpsest", "a scraped manuscript page", anchor(3, 100.0, 40.0));
        let drifted = mark("g2", "palimpsest", "a scraped manuscript page", anchor(3, 100.4, 40.2));
        assert!(same_glossed_spot(&a, &drifted));

        let moved = mark("g3", "palimpsest", "a scraped manuscript page", anchor(3, 100.0, 90.0));
        assert!(!same_glossed_spot(&a, &moved));
        let other = mark("g4", "palimpsests", "a scraped manuscript page", anchor(3, 100.0, 40.0));
        assert!(!same_glossed_spot(&a, &other));
    }

    #[test]
    fn a_reflowable_mark_is_the_same_spot_at_the_same_characters_not_pixels() {
        // The whole point of the envelope: re-glossing a word after the reader
        // has scrolled (so the captured viewport box differs entirely) is the
        // SAME mark, and must not stack a second stroke on it.
        let spot = ReflowSpot::new(12, 30, 40);
        let envelope = spot_envelope(&spot, "a manuscript page, scraped clean");
        let a = mark("g1", "palimpsest", &envelope, anchor(4, 100.0, 40.0));
        let scrolled = mark("g2", "palimpsest", &envelope, anchor(4, 250.0, 610.0));
        assert!(same_glossed_spot(&a, &scrolled));

        // One character over is another word, whatever the pixels say.
        let neighbour = spot_envelope(&ReflowSpot::new(12, 31, 41), "the next word over");
        let b = mark("g3", "palimpsest", &neighbour, anchor(4, 100.0, 40.0));
        assert!(!same_glossed_spot(&a, &b));
        // And so is the same range in another block.
        let elsewhere = spot_envelope(&ReflowSpot::new(13, 30, 40), "another paragraph");
        let c = mark("g4", "palimpsest", &elsewhere, anchor(4, 100.0, 40.0));
        assert!(!same_glossed_spot(&a, &c));
    }

    #[test]
    fn a_mark_with_no_envelope_is_compared_by_its_anchor_instead() {
        // A PDF's sentence context and a reflowable envelope are different
        // pipelines, so neither can read the other's identity: the anchors
        // decide, exactly as they did before there were two.
        let spot = spot_envelope(&ReflowSpot::new(1, 0, 4), "the word in a sentence");
        let a = mark("g1", "word", &spot, anchor(1, 10.0, 10.0));
        let b = mark("g2", "word", "the word in a sentence", anchor(1, 10.0, 10.0));
        assert!(same_glossed_spot(&a, &b), "same anchor, so the anchors decide");
        let c = mark("g3", "word", "the word in a sentence", anchor(2, 10.0, 10.0));
        assert!(!same_glossed_spot(&a, &c));
    }
}
