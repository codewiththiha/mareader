//! The two-answer question, and the two facts that raise it: a loose import of a
//! file that sits inside a folder the library reads in place, where that
//! folder's book for it is alive and standing; or a loose import of a file whose
//! content the library already holds, wherever it is filed.
//!
//! Two answers rather than three because the third a name collision offers — a
//! pointer at a row on THIS level — has nothing to point at in either case: the
//! row the library holds is not on this level, and a second linked row of one
//! read-at-place file is the one thing that rule never makes. Which fact was
//! noticed decides the sentence above the buttons and nothing else.
//!
//! Described rather than drawn: the sheet is
//! [`ConflictSheet`](crate::features::library::conflict_modal)'s.

use leptos::prelude::*;

use library_core::conflict::Placement;

use crate::services::library::conflict::ConflictAsk;
use crate::services::library::folder_label;
use crate::state::AppState;

use super::info::{more_waiting, where_line};
use super::sheet::{AnswerRoute, ChoiceSpec, SheetSpec};

/// The covered file's question, described: the library's own stored copy on this
/// level — a book of its own bytes with its own highlights and its own place in
/// it — or the book the library already holds, gone to and lit.
///
/// What is left after the stronger question has been asked. A covered file's
/// folder has already answered what a name collision would ask, and a file whose
/// content the library holds is a book the reader has rather than a name a level
/// is short of; in both cases the honest choice is a second instance or the first
/// one, and nothing else. The switch gives every other waiting question of this
/// shape the same answer, because what one file of a forty-file drop says is
/// usually what the other thirty-nine say.
pub(super) fn describe_covered(state: AppState, ask: &ConflictAsk) -> SheetSpec {
    let waiting = state
        .library
        .conflict_waiting
        .with_untracked(|w| w.iter().filter(|each| each.kind.is_two_answer()).count());
    let incoming = ask.arrival.name.clone();
    // Which fact the library noticed decides the sentence, and the two are not
    // the same question: a covered file is ground a folder reads in place, where
    // one OS file is one linked book; a file the library simply already holds is
    // a book the reader has, wherever it is filed and whatever it is called.
    // Same two answers, different reason.
    let folder_name = ask
        .kind
        .folder_id()
        .and_then(|folder_id| state.library.folder(folder_id).map(|f| folder_label(&f.root)));
    let book_name = ask.existing_name.clone();
    let subtitle = more_waiting(
        match &folder_name {
            Some(name) => format!("Inside “{name}”"),
            None => "Already in your library".to_string(),
        },
        waiting,
    );
    let where_line = where_line(state, &ask.arrival.shelf_id);
    let question = match &folder_name {
        Some(name) => format!(
            "“{incoming}” is inside “{name}”, which the library reads in place — one \
             book per file, never a second link. Import your own copy {where_line}, or go to \
             the book the folder holds."
        ),
        None => format!(
            "The library already holds this book as “{book_name}”. Import your own copy \
             {where_line}, or go to the one you have."
        ),
    };
    let import_note = format!(
        "The library's own copy — its own book {where_line}, its own highlights, its own \
         place in it"
    );
    let show_note = match &folder_name {
        Some(name) => format!("Add nothing — go to “{book_name}” inside “{name}” and light it up"),
        None => format!("Add nothing — go to “{book_name}” and light it up"),
    };

    SheetSpec {
        heading: incoming,
        subtitle,
        question,
        cancel_title: "Leave the shelf as it is",
        waiting,
        apply_all: true,
        route: AnswerRoute::Covered,
        choices: vec![
            ChoiceSpec {
                label: "Import a copy here",
                note: import_note,
                placement: Placement::KeepBoth,
            },
            ChoiceSpec {
                label: "Show the imported one",
                note: show_note,
                placement: Placement::Open,
            },
        ],
    }
}
