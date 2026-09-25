//! The two-answer question, and the two facts that raise it: a loose import
//! of a file inside a folder the library reads in place whose book is alive,
//! or of a file whose content the library already holds.
//!
//! Two answers rather than three: a pointer at a row on this level is not an
//! option a covered file has.

use leptos::prelude::*;

use library_core::conflict::Placement;

use crate::services::library::conflict::ConflictAsk;
use crate::services::library::folder_label;
use crate::state::AppState;

use super::info::{more_waiting, where_line};
use super::sheet::{AnswerRoute, ChoiceSpec, SheetSpec};

/// The library's own stored copy on this level, or the book the library
/// already holds — what is left after the stronger question has been asked.
pub(super) fn describe_covered(state: AppState, ask: &ConflictAsk) -> SheetSpec {
    let waiting = state
        .library
        .conflict_waiting
        .with_untracked(|w| w.iter().filter(|each| each.kind.is_two_answer()).count());
    let incoming = ask.arrival.name.clone();
    // Which fact the library noticed decides the sentence: same two answers,
    // different reason.
    let folder_name = ask.kind.folder_id().and_then(|folder_id| {
        state
            .library
            .folder(folder_id)
            .map(|f| folder_label(&f.root))
    });
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
