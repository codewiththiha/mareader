//! The folder's question: a level already holds the name.
//!
//! Asked BEFORE the walk rather than after it, because the answer decides what the walk is for.
//! WHICH answers the sheet offers is the arrival's mode.

use leptos::prelude::*;

use library_core::conflict::{Placement, next_shelf_name};
use library_core::shelf::members_of;

use crate::services::library::conflict::{self, ShelfConflictAsk};
use crate::services::library::import::replace_rows_of_tree;
use crate::state::AppState;

use super::sheet::{AnswerRoute, ChoiceSpec, SheetSpec};

const LINK_NOTE: &str = "A pointer row, not a second shelf: nothing is \
                         imported, and tapping it lights the folder where it is";

pub(super) fn describe_shelf(state: AppState, ask: &ShelfConflictAsk) -> SheetSpec {
    let own = ask.own;
    let arrival_reads_in_place = ask.opts.mode().reads_in_place();
    let reads_in_place = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .any(|f| f.root == ask.root && f.mode().reads_in_place())
    });
    // The row promises the counter rather than asking the reader to take "the next free name" on faith.
    let new_name = state.library.shelves.with_untracked(|shelves| {
        next_shelf_name(shelves, None, &ask.incoming_name)
    });
    let replace_rows = if own && reads_in_place {
        replace_rows_of_tree(state, &ask.root).len()
    } else {
        let (rows, shelves) = state.library.snapshot_rows();
        members_of(&rows, &shelves, &ask.existing_id).len()
    };
    let subtitle = if own {
        format!("Already in the library as “{}”", ask.existing_name)
    } else {
        format!("A shelf called “{}” is already here", ask.existing_name)
    };
    let question = if arrival_reads_in_place {
        "A folder read in place cannot mint a second shelf of itself. Leave a \
         pointer to the shelf that is here, or file this folder's books into it."
            .to_string()
    } else if own {
        format!(
            "“{}” is the shelf this folder's last import made. Look at it, \
             replace its books with these copies, or give the copies a shelf of \
             the next free name.",
            ask.existing_name
        )
    } else {
        "The arriving copies are the library's own, so all three answers are \
         open: look at the shelf that is here, replace its books, or shelve the \
         copies under the next free name."
            .to_string()
    };
    let show_note = format!(
        "Import nothing — go to “{}” and light it up where it stands",
        ask.existing_name
    );
    let new_note = if reads_in_place {
        format!(
            "Import as “{new_name}” — the library's own copies; the tree here \
             keeps reading the folder"
        )
    } else {
        format!("Import as “{new_name}” — its own shelf, its own tree")
    };
    let merge_note = format!(
        "The folder's books join “{}” — a name it already holds asks one by one",
        ask.existing_name
    );
    let replace_note = match replace_rows {
        0 => format!(
            "Nothing to remove — the copies simply take “{}”",
            ask.existing_name
        ),
        1 => format!(
            "One book leaves, highlights and all — a copy takes its place on “{}”",
            ask.existing_name
        ),
        n => format!(
            "{n} books leave, highlights and all — copies take “{}”",
            ask.existing_name
        ),
    };

    let choices = conflict::shelf_offers(ask)
        .iter()
        .map(|choice| match choice {
            // No *as new* — a second shelf of one linked folder is the second instance the family gate exists to prevent — and no *replace*: a linked tree is not the level's to empty.
            Placement::LinkOnly => ChoiceSpec {
                label: "Make link",
                note: LINK_NOTE.to_string(),
                placement: Placement::LinkOnly,
            },
            Placement::Merge => ChoiceSpec {
                label: "Merge into it",
                note: merge_note.clone(),
                placement: Placement::Merge,
            },
            // No pointer — a stored import is a second instance the library owns, and "show me the first" is a light rather than a row.
            Placement::Open => ChoiceSpec {
                label: "Show it",
                note: show_note.clone(),
                placement: Placement::Open,
            },
            Placement::Replace => ChoiceSpec {
                label: "Replace",
                note: replace_note.clone(),
                placement: Placement::Replace,
            },
            Placement::KeepBoth => ChoiceSpec {
                label: "Add as new",
                note: new_note.clone(),
                placement: Placement::KeepBoth,
            },
        })
        .collect();

    SheetSpec {
        heading: ask.incoming_name.clone(),
        subtitle,
        question,
        cancel_title: "Import nothing",
        waiting: 0,
        apply_all: false,
        route: AnswerRoute::Shelf,
        choices,
    }
}
