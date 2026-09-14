//! The name question: an arrival whose name a row on this level already carries, and the answers
//! — the import's or the move's, decided by what is arriving. Described rather than drawn:
//! [`ConflictSheet`](crate::features::library::conflict_modal) draws it.

use leptos::prelude::*;

use library_core::conflict::{Placement, next_name};

use crate::services::library::conflict::{self, ConflictAsk};
use crate::state::AppState;

use super::info::{more_waiting, where_line};
use super::sheet::{AnswerRoute, ChoiceSpec, SheetSpec};

/// Every sentence here is built off one snapshot of the library, because a row that counted one way and answered another is a receipt for something else.
pub(super) fn describe_name(state: AppState, ask: &ConflictAsk) -> SheetSpec {
    let where_line = where_line(state, &ask.arrival.shelf_id);
    let import = ask.arrival.is_import();
    // The apply's own list rather than a condition the sheet re-derives, so a row the sheet renders is a row the answer will take.
    let offers = conflict::offers_for(state, ask);
    let existing_name = ask.existing_name.clone();
    let (rows, shelves) = state.library.snapshot_rows();
    let new_name = next_name(
        &rows,
        &shelves,
        &ask.arrival.shelf_id,
        &ask.arrival.name,
    );
    // A count taken from the address would promise a loss the merge cannot make, because a twin still reading that address keeps its own marks.
    let marks = crate::storage::load_gloss()
        .get(&ask.existing_id)
        .map(Vec::len)
        .unwrap_or(0);

    let question = if import {
        format!(
            "“{}” is already {where_line}. Add a second book of its own, put a link here \
             instead, or go to the one you have.",
            ask.arrival.name
        )
    } else if offers.contains(&Placement::LinkOnly) {
        format!(
            "A book called “{existing_name}” is already {where_line}, and it is one of the library's own copies. \
             Keep one book, reach the copy from here, or keep both under a new name.",
        )
    } else {
        format!(
            "A book called “{existing_name}” is already {where_line}. Keep one book, keep this one \
             instead, or keep both under a new name.",
        )
    };
    let go_to_note = format!("Add nothing — go to “{existing_name}” where it already is");
    let new_note = format!("Keeps both, under the next free name — “{new_name}”");
    const LINK_NOTE: &str = "A pointer row, not a copy: tapping it goes to the book where it lives";
    let merge_note = format!(
        "One book — “{existing_name}” stays, and takes this one's shelves, its highlights, \
         and the further place in it"
    );
    let replace_note = if marks > 0 {
        format!(
            "“{existing_name}” leaves the library, with its {} — this one takes its place \
             on every shelf it was on",
            library_core::text::plural(marks, "highlight", "highlights")
        )
    } else {
        format!(
            "“{existing_name}” leaves the library — this one takes its place on every shelf \
             it was on"
        )
    };
    let move_new_note = format!("Keeps both — this one becomes “{new_name}”");
    let link_note = format!(
        "The book you dragged becomes a pointer here — “{existing_name}” stays, the file on disk stays, \
         and nothing is destroyed"
    );

    let choices = offers
        .iter()
        .map(|choice| match choice {
            Placement::Open => ChoiceSpec {
                label: "Already imported",
                note: go_to_note.clone(),
                placement: Placement::Open,
            },
            Placement::KeepBoth if import => ChoiceSpec {
                label: "Add as new",
                note: new_note.clone(),
                placement: Placement::KeepBoth,
            },
            Placement::KeepBoth => ChoiceSpec {
                label: "As new",
                note: move_new_note.clone(),
                placement: Placement::KeepBoth,
            },
            Placement::LinkOnly if import => ChoiceSpec {
                label: "Make link",
                note: LINK_NOTE.to_string(),
                placement: Placement::LinkOnly,
            },
            Placement::LinkOnly => ChoiceSpec {
                label: "Make link",
                note: link_note.clone(),
                placement: Placement::LinkOnly,
            },
            Placement::Merge => ChoiceSpec {
                label: "Merge",
                note: merge_note.clone(),
                placement: Placement::Merge,
            },
            Placement::Replace => ChoiceSpec {
                label: "Replace",
                note: replace_note.clone(),
                placement: Placement::Replace,
            },
        })
        .collect();

    let waiting = state.library.conflict_waiting.with_untracked(|w| w.len());
    SheetSpec {
        heading: ask.arrival.name.clone(),
        subtitle: more_waiting(format!("Already {where_line}"), waiting),
        question,
        cancel_title: "Leave the shelf as it is",
        waiting,
        apply_all: false,
        route: AnswerRoute::Placement,
        choices,
    }
}
