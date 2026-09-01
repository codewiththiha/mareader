//! Gloss highlights: the persisted marks of the open document, plus the
//! transient multi-select and "thinking" states the mark layer paints.

use leptos::prelude::*;

use pdf_core::gloss::GlossMark;

/// The persisted gloss highlights of the OPEN document.
///
/// One flat list rather than a per-page map: a document has a handful of
/// marks, every page host filters the list itself, and a `Vec` is what both
/// localStorage and the `<For>` in the mark layer want.
#[derive(Clone, Copy, Default)]
pub struct GlossState {
    pub marks: RwSignal<Vec<GlossMark>>,
    /// Gloss multi-select mode (long-press initiated on a mark).
    pub selection_active: RwSignal<bool>,
    /// Ids of the marks currently selected while in multi-select mode.
    pub selected_marks: RwSignal<std::collections::HashSet<String>>,
    /// id of the mark whose "processing" highlighter animation is live, if any.
    ///
    /// Lives here, not in the popover, because the animation is painted by the
    /// in-page mark layer: while the model is working there is NO surface at
    /// all, so the stroke itself has to carry the thinking state.
    pub processing_id: RwSignal<Option<String>>,
}

impl GlossState {
    /// Clear every field to its resting state. Runs on document close and as
    /// the first step of an open, so a field added to the struct cannot be
    /// silently forgotten by either path — the same invariant the other
    /// slices enforce with their own `reset` methods.
    pub fn reset(&self) {
        self.marks.set(Vec::new());
        self.selection_active.set(false);
        self.selected_marks.set(std::collections::HashSet::new());
        self.processing_id.set(None);
    }
}
