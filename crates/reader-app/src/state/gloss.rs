//! Gloss highlights: the persisted marks of the open document, plus the
//! transient multi-select and "thinking" states the mark layer paints.

use ai_core::gloss::GlossMark;
use leptos::prelude::*;

use super::ReaderState;

/// The persisted gloss highlights of the OPEN document. One flat list rather
/// than a per-page map: a document has a handful of marks, every page host
/// filters the list itself, and a `Vec` is what both localStorage and the
/// `<For>` in the mark layer want.
#[derive(Clone, Copy, Default)]
pub struct GlossState {
    pub marks: RwSignal<Vec<GlossMark>>,
    /// Gloss multi-select mode (long-press initiated on a mark).
    pub selection_active: RwSignal<bool>,
    /// Ids of the marks currently selected while in multi-select mode.
    pub selected_marks: RwSignal<std::collections::HashSet<String>>,
    /// Id of the mark whose "processing" highlighter animation is live, if
    /// any. Lives here, not in the popover, because the animation is painted
    /// by the in-page mark layer: while the model works there is NO surface at
    /// all, so the stroke itself carries the thinking state.
    pub processing_id: RwSignal<Option<String>>,
}

impl GlossState {
    /// Clear every field to its resting state. Runs on document close and as
    /// the first step of an open. Destructured with no rest, so a field added
    /// to the struct cannot be silently forgotten by either path.
    ///
    /// The handles are `Copy`, so this binds the signals the struct already
    /// holds; `Self::default()` would allocate a fresh arena node per field on
    /// every close and leak them.
    pub fn reset(&self) {
        let Self { marks, selection_active, selected_marks, processing_id } = *self;
        marks.set(Vec::new());
        selection_active.set(false);
        selected_marks.set(std::collections::HashSet::new());
        processing_id.set(None);
    }
}

/// The one write the reader owes storage: the open document's whole mark
/// list, under its key.
///
/// Injected rather than called. The persisted blob is the library's, and the
/// reader has no business reaching localStorage for another window's file —
/// so the shell, which owns `storage`, hands the reader this callback and the
/// reader hands back a key and a list. The shape is a document's list, not a
/// mark, because that is the shape the blob is stored in: every mutation
/// persists the whole list.
///
/// Provided once by the reader route (`features/reader/page.rs` in the shell
/// crate) and read where the marks are written
/// (`components::ai::gloss::controller`).
pub type GlossSave = Callback<(String, Vec<GlossMark>)>;

/// The key the open document's highlights are stored under: the id of the
/// row the library holds for it.
///
/// An id rather than a string derived from the address: an id does not move
/// when the bytes do, so a conversion or a merge keeps the marks where they
/// are, and two rows of one file keep their lists apart because each has its
/// own id.
///
/// Every writer of the marks asks here rather than reading the document's
/// path — the load at open, the save per stroke
/// ([`crate::components::ai::gloss::controller`]) and the sweep on a removal
/// — so the three cannot disagree about which list they mean.
///
/// Empty when nothing is open or the open has no row the library can name;
/// every caller reads that as "nowhere to put it" rather than as a key. An
/// address-only open settles onto its row before any tail loads the marks, so
/// the empty window is the window where there is no book to have marks about.
pub fn gloss_key(reader: &ReaderState) -> String {
    reader.document.book_id.get_untracked().unwrap_or_default()
}
