//! The folder's question: a level already holds the name.
//!
//! A folder arriving under a name its level holds is the shelf's own spelling
//! of the collision the book sheet asks about — two doors of one name on one
//! level are two doors a reader cannot tell apart — and it is asked BEFORE the
//! walk rather than after it, because the answer decides what the walk is for.
//! WHICH answers the sheet offers is the arrival's mode, and the two sets are
//! the two ways a folder can be held:
//!
//! A STORED arrival — copies the library owns, unrelated to any tree — gets
//! the level's own three: *show it* goes and lights the shelf that is here
//! and imports nothing, *replace* sends the books the shelf holds out through
//! the removal's sweep and seats the arriving copies on it, and *as new*
//! mints a shelf of the next free name. Of ground the library already READS
//! IN PLACE, the *as new* tree holds copies of its own — independent books of
//! their own bytes beside the linked ones the old tree keeps — and the
//! *replace* is the import module's own log-spending sweep, so the copies
//! take the shelf the linked books left.
//!
//! A READ-AT-PLACE arrival keeps two answers: *make link* leaves a pointer at
//! the sheet that is here, and *merge into it* files the folder's books onto
//! it, a book whose name it already holds asking one by one on the compact
//! sheet — at every level of the tree that already stands, with an
//! apply-to-all switch for a reader who has seen enough to answer for the
//! rest. Its *as new* is withheld: a second shelf of one linked folder is the
//! second instance the family gate exists to prevent. And a read-at-place
//! arrival of its OWN family never reaches this sheet at all — the gate in
//! `crate::services::library::import` answers it with a light, a
//! continuation, or the fold back into the tree its directory names.
//!
//! A folder colliding with its OWN previous shelf asks too — a re-import that
//! ended on "Imported 0 books" with no sheet in between was the silent
//! nothing this exists to stop — and the sheet words it as the continuation
//! it is.
//!
//! A drag never asks this: nesting a shelf writes a parent rather than a
//! membership, so nothing arrives on a level for a name to collide with — the
//! rule `crate::services::library::arrange` gives. And a watched folder's own
//! rescan never asks either: staying quiet is a rescan's whole job.
//!
//! Described rather than drawn, like the three book questions beside it: the
//! sheet is [`ConflictSheet`](crate::features::library::conflict_modal)'s, and
//! what is here is the folder's own sentences and its own list of answers.
//!
//! The removal receipt's rule and the collision sheet's hold here too: a
//! `view!` body is a builder, not a place to compute. This sheet was the one
//! that broke it — its whole body was a single closure deriving six sentences
//! and then rendering them, and one of the six (`replace_rows`) walked the
//! entire library to count what a *replace* would take out of it. A count taken
//! inside a render closure is a count re-taken on every reactive re-run of the
//! sheet, so a modal that repaints while the reader is reading it re-walks every
//! book and every folder the library owns to answer a question nothing about
//! the repaint changed. Read once per answer, here, and the sheet only draws.

use leptos::prelude::*;

use library_core::conflict::{Placement, next_shelf_name};
use library_core::shelf::members_of;

use crate::services::library::conflict::{self, ShelfConflictAsk};
use crate::services::library::import::replace_rows_of_tree;
use crate::state::AppState;

use super::sheet::{AnswerRoute, ChoiceSpec, SheetSpec};

/// What a *make link* row promises. A `const` rather than a `format!` because
/// nothing in it varies: a pointer is a pointer whichever folder it points at.
const LINK_NOTE: &str = "A pointer row, not a second shelf: nothing is \
                         imported, and tapping it lights the folder where it is";

/// The folder's own name collision, described.
pub(super) fn describe_shelf(state: AppState, ask: &ShelfConflictAsk) -> SheetSpec {
    // A folder colliding with its OWN previous shelf is a continuation,
    // and the sheet WORDS it as one — but the answers are the arrival
    // mode's either way.
    let own = ask.own;
    // The arrival's OWN mode: the two switches the sheet asked, read as
    // the one answer they add up to.
    let arrival_reads_in_place = ask.opts.mode().reads_in_place();
    // Whether the ground the arrival picks is one the library already
    // READS in place: the *as new* tree of copies beside the linked rows
    // the old tree keeps, and the *replace* that spends the tree's own
    // logs, are both this fact's.
    let reads_in_place = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .any(|f| f.root == ask.root && f.mode().reads_in_place())
    });
    // The name *as new* would mint, counted against the level's own
    // shelves — the row promises the counter rather than asking the
    // reader to take "the next free name" on faith.
    let new_name = state.library.shelves.with_untracked(|shelves| {
        next_shelf_name(shelves, None, &ask.incoming_name)
    });
    // The replace row's promise, counted here rather than taken on
    // faith: of the folder's own read-at-place tree, the linked books the
    // sweep sends out; of any other shelf, the rows it holds.
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
        // The subtitle already named the collision; the sentence is only
        // the rule and the two ways out of it.
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

    // The answers, in the order the apply's own list gives them.
    let choices = conflict::shelf_offers(ask)
        .iter()
        .map(|choice| match choice {
            // The read-at-place arrival's two: a pointer at the shelf that is
            // here, or the folder's books joining it. No *as new* — a second
            // shelf of one linked folder is the second instance the family gate
            // exists to prevent — and no *replace*: a linked tree is not the
            // level's to empty.
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
            // The stored arrival's three, the level's own: go and look, replace
            // what is here, or a shelf of the next free name. No pointer — a
            // stored import is a second instance the library owns, and "show me
            // the first" is a light rather than a row.
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
        // A folder's collision is answered one arrival at a time: the shelf's
        // own question decides what the walk is FOR, so there is nothing a
        // second answer could be shared with.
        waiting: 0,
        apply_all: false,
        route: AnswerRoute::Shelf,
        choices,
    }
}
