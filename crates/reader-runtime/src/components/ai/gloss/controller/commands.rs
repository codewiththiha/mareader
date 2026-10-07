//! The shared behaviours: one close, one persistence, one retry.

use ai_core::gloss::GlossMark;
use leptos::prelude::*;
use runtime_contract::boundary::ShellApi;

use crate::components::ai::gloss::phase::GlossPhase;

use super::MARK_CAP;
use super::cache::GlossCache;
use super::content::GlossContent;
use super::drag::GlossDrag;
use super::geometry::GlossGeometry;
use super::open::GlossOpen;

#[derive(Clone, Copy)]
pub struct GlossCommands {
    /// Full dismiss back to Idle (keeps the mark — the highlight reopens it).
    pub reset: Callback<()>,
    /// The outro: fold the expanded card back down onto the word.
    pub collapse_to_mark: Callback<()>,
    /// Record and persist a captured mark, returning the canonical one.
    pub add_mark: Callback<GlossMark, GlossMark>,
    /// Remove marks by id: persist, evict answers, close if open.
    pub remove_marks: Callback<Vec<String>, Vec<GlossMark>>,
    /// Re-insert previously removed marks (the Undo path) and persist.
    pub restore_marks: Callback<Vec<GlossMark>>,
    /// Retry the current mark after a retryable failure.
    pub retry: Callback<()>,
}

/// Whether two marks denote the same glossed spot, in either format.
fn same_glossed_spot(a: &GlossMark, b: &GlossMark) -> bool {
    if a.word != b.word {
        return false;
    }
    let (left, right) = (
        crate::components::ai::reflow_anchor::read_spot(&a.context),
        crate::components::ai::reflow_anchor::read_spot(&b.context),
    );
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        // Different pipelines: anchor tolerance decides.
        _ => a.same_spot(b),
    }
}

/// Build the commands over a controller's slices: behaviour, not state.
pub(super) fn build_commands(
    state: crate::context::ReaderContext,
    content: GlossContent,
    geometry: GlossGeometry,
    open: GlossOpen,
    drag: GlossDrag,
    cache: GlossCache,
) -> GlossCommands {
    let popover_open = state.reader.ai_selection.popover_open;
    let processing_id = state.reader.gloss.processing_id;
    let marks = state.reader.gloss.marks;

    // The marks' one write is the Shell's: every mutation sends the whole
    // list.
    let persist = move || {
        let key = crate::services::document::gloss_key(state);
        if key.is_empty() {
            return;
        }
        match marks.with_untracked(|list| storage::encode_gloss(list.as_slice())) {
            Ok(encoded) => state.api.save_gloss(&key, encoded),
            Err(e) => e.report(),
        }
    };

    // Full dismiss back to Idle; the mark is kept.
    let reset = Callback::new(move |_| {
        popover_open.set(false);
        content.clear();
        geometry.clear();
        drag.clear();
        processing_id.set(None);
        open.mark.set(None);
        // A dismissed card has no run: a late chunk cannot reopen it.
        open.end_run();
    });

    // Every close path funnels through here.
    let collapse_to_mark = Callback::new(move |_| {
        if geometry.gphase.get_untracked() != GlossPhase::Expanded || drag.active.get_untracked() {
            return;
        }
        drag.offset.set(None);
        geometry.gphase.set(GlossPhase::Compact);
    });

    // Hand back the CANONICAL mark: the id keys the glow and cache.
    let add_mark = Callback::new(move |m: GlossMark| -> GlossMark {
        let existing =
            marks.with_untracked(|v| v.iter().find(|o| same_glossed_spot(o, &m)).cloned());
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
        // The evicted oldest mark's answer goes with it.
        if let Some(old) = evicted {
            cache.remove(&old.id);
        }
        persist();
        m
    });

    // The single removal path; the batch comes back for undo.
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

    // Undo re-inserts (id-deduped) and persists; the cache stays evicted.
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

    // The opening ritual minus persistence: the mark is canonical.
    let retry = Callback::new(move |_| {
        let Some(mark) = open.mark.get_untracked() else {
            return;
        };
        if !tauri_bridge::has_tauri() {
            // The environment cannot change mid-session, so the button isn't
            // showing.
            return;
        }
        // A retry is a NEW run of the ritual, minus persistence.
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
            rect: GlossBox {
                x,
                y,
                w: 40.0,
                h: 12.0,
                r: 0.0,
            },
        }
    }

    fn mark(id: &str, word: &str, context: &str, anchor: PageAnchor) -> GlossMark {
        GlossMark {
            id: id.to_string(),
            word: word.to_string(),
            context: context.to_string(),
            anchor,
        }
    }

    #[test]
    fn a_pdf_mark_is_the_same_spot_within_the_anchor_tolerance() {
        let a = mark(
            "g1",
            "palimpsest",
            "a scraped manuscript page",
            anchor(3, 100.0, 40.0),
        );
        let drifted = mark(
            "g2",
            "palimpsest",
            "a scraped manuscript page",
            anchor(3, 100.4, 40.2),
        );
        assert!(same_glossed_spot(&a, &drifted));

        let moved = mark(
            "g3",
            "palimpsest",
            "a scraped manuscript page",
            anchor(3, 100.0, 90.0),
        );
        assert!(!same_glossed_spot(&a, &moved));
        let other = mark(
            "g4",
            "palimpsests",
            "a scraped manuscript page",
            anchor(3, 100.0, 40.0),
        );
        assert!(!same_glossed_spot(&a, &other));
    }

    #[test]
    fn a_reflowable_mark_is_the_same_spot_at_the_same_characters_not_pixels() {
        // The point of the envelope: same spot after a scroll, one stroke.
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
        // Different pipelines, so the anchors decide.
        let spot = spot_envelope(&ReflowSpot::new(1, 0, 4), "the word in a sentence");
        let a = mark("g1", "word", &spot, anchor(1, 10.0, 10.0));
        let b = mark(
            "g2",
            "word",
            "the word in a sentence",
            anchor(1, 10.0, 10.0),
        );
        assert!(
            same_glossed_spot(&a, &b),
            "same anchor, so the anchors decide"
        );
        let c = mark(
            "g3",
            "word",
            "the word in a sentence",
            anchor(2, 10.0, 10.0),
        );
        assert!(!same_glossed_spot(&a, &c));
    }
}
