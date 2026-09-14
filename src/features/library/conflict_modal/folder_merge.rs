//! The compact per-file question a folder merge asks: two names, three answers,
//! and the switch that gives every waiting question the same answer in one
//! click.
//!
//! Described rather than drawn: the sheet is
//! [`ConflictSheet`](crate::features::library::conflict_modal)'s, and what is
//! left here is the pair of names and the sentences.
//!
//! Its three are the move sheet's, re-spelled for an arrival with no row of its
//! own: there is nothing to fold INTO the shelf's row yet, so *merge* is the
//! file handing the row its measurement rather than two rows becoming one.
//! Smaller than the import sheet on purpose — the shelf's question is already
//! answered, and a sheet that re-explained the whole situation per file would be
//! a sentence the reader has to re-read forty times.

use leptos::prelude::*;

use library_core::book::find_row;
use library_core::conflict::{Placement, next_name};

use crate::services::library::conflict::ConflictAsk;
use crate::state::AppState;

use super::info::more_waiting;
use super::sheet::{AnswerRoute, ChoiceSpec, SheetSpec};

/// A merged folder's per-file question, described.
pub(super) fn describe_folder_merge(state: AppState, ask: &ConflictAsk) -> SheetSpec {
    let waiting = state
        .library
        .conflict_waiting
        .with_untracked(|w| w.iter().filter(|each| each.kind.is_folder_merge()).count());
    let incoming = ask.arrival.name.clone();
    let existing = ask.existing_name.clone();
    let subtitle = more_waiting(format!("Into “{existing}”"), waiting);
    // The file arriving is the very file the row on the shelf reads — a
    // re-import of a read-at-place folder's own book. *As new* of it would be
    // a second row of one linked file, which the library does not make, so
    // the sheet offers the two answers that add no copy. A different file
    // wearing the same name keeps all three, and a STORED folder keeps all
    // three too: its *as new* is a second copy in the store, a book of its
    // own bytes rather than a second door on one file.
    let twin = ask.kind.reads_in_place()
        && state.library.books.with_untracked(|rows| {
            ask.arrival.file.as_ref().is_some_and(|file| {
                find_row(rows, &ask.existing_id)
                    .and_then(|row| row.book())
                    .is_some_and(|book| book.path() == file.path)
            })
        });
    let question = if twin {
        format!(
            "“{incoming}” is arriving, and “{existing}” on this shelf reads this very file. \
             Keep the one that is here, or seat this file in its place."
        )
    } else {
        format!(
            "“{incoming}” is arriving, and “{existing}” is already on this shelf. \
             Keep the one that is here, seat this file in its place, or keep both \
             under a name of its own."
        )
    };
    let merge_note = format!("One book — “{existing}” stays, and takes this file's measurement");
    const REPLACE_NOTE: &str =
        "The row on the shelf leaves the library; this file takes its slot";
    let new_name = {
        let (rows, shelves) = state.library.snapshot_rows();
        next_name(&rows, &shelves, &ask.arrival.shelf_id, &incoming)
    };
    let new_note = format!("Keep both — this file becomes “{new_name}”");

    let mut choices = vec![
        ChoiceSpec {
            label: "Merge",
            note: merge_note,
            placement: Placement::Merge,
        },
        ChoiceSpec {
            label: "Replace",
            note: REPLACE_NOTE.to_string(),
            placement: Placement::Replace,
        },
    ];
    if !twin {
        choices.push(ChoiceSpec {
            label: "As new",
            note: new_note,
            placement: Placement::KeepBoth,
        });
    }

    SheetSpec {
        heading: incoming,
        subtitle,
        question,
        cancel_title: "Leave the shelf as it is",
        waiting,
        apply_all: true,
        route: AnswerRoute::FolderMerge,
        choices,
    }
}
