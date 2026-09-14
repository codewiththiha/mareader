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

use leptos::prelude::*;

use crate::components::primitives::menu::choice_row::ChoiceRow;
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::question_sheet::QuestionSheet;
use library_core::conflict::Placement;

use crate::services::library::conflict::{self, ShelfConflictAsk};
use crate::state::AppState;

#[component]
pub(crate) fn ShelfConflictModal(state: AppState) -> impl IntoView {
    let open = state.library.shelf_conflict.open;

    // A close that came from the lane registry, the Escape key or the shell's
    // backdrop wrote only the boolean; the question goes with it, so the sheet
    // can never reopen onto a folder somebody already dismissed.
    Effect::new(move |_| {
        if !open.get() {
            state.library.shelf_conflict.ask.set(None);
        }
    });

    view! {
        <ModalShell
            open=open
            aria_label="A shelf of that name is already here"
            width="min(92vw, 420px)"
        >
            {move || {
                let ask = state.library.shelf_conflict.ask.get()?;
                let info = ShelfAskInfo::of(state, &ask);
                Some(view! { <ShelfSheet state=state info=info /> }.into_any())
            }}
        </ModalShell>
    }
}

/// Everything the folder sheet prints, read once per answer.
///
/// The removal receipt's rule and the collision sheet's: a `view!` body is a
/// builder, not a place to compute. This sheet was the one that broke it — its
/// whole body was a single closure deriving six sentences and then rendering
/// them, and one of the six (`replace_rows`) walked the entire library to count
/// what a *replace* would take out of it.
///
/// A count taken inside a render closure is a count re-taken on every reactive
/// re-run of the sheet, so a modal that repaints while the reader is reading it
/// re-walks every book and every folder the library owns to answer a question
/// nothing about the repaint changed. Read once per answer, here, and the body
/// only builds.
struct ShelfAskInfo {
    heading: String,
    subtitle: String,
    question: String,
    show_note: String,
    new_note: String,
    merge_note: String,
    replace_note: String,
    /// The answers this arrival gets, in the order the sheet shows them —
    /// `conflict::shelf_offers`' answer rather than a condition the sheet
    /// re-derives.
    ///
    /// A read-at-place import never offers *keep both*, because the folder's
    /// shelf IS the OS folder and a counter-named twin of it would be a second
    /// door onto the same ground, and never *replace*, because a linked tree is
    /// not the level's to empty. A stored arrival gets the level's own three and
    /// no pointer: copies are the library's to make another of, and "show me the
    /// first" is a light rather than a row.
    offers: &'static [Placement],
}

/// What a *make link* row promises. A `const` rather than a `format!` because
/// nothing in it varies: a pointer is a pointer whichever folder it points at.
const LINK_NOTE: &str = "A pointer row, not a second shelf: nothing is \
                         imported, and tapping it lights the folder where it is";

impl ShelfAskInfo {
    fn of(state: AppState, ask: &ShelfConflictAsk) -> Self {
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
            library_core::conflict::next_shelf_name(shelves, None, &ask.incoming_name)
        });
        // The replace row's promise, counted here rather than taken on
        // faith: of the folder's own read-at-place tree, the linked books the
        // sweep sends out; of any other shelf, the rows it holds.
        let replace_rows = if own && reads_in_place {
            crate::services::library::import::replace_rows_of_tree(state, &ask.root).len()
        } else {
            let (rows, shelves) = state.library.snapshot_rows();
            library_core::shelf::members_of(&rows, &shelves, &ask.existing_id).len()
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
        Self {
            heading: ask.incoming_name.clone(),
            subtitle,
            question,
            show_note,
            new_note,
            merge_note,
            replace_note,
            offers: conflict::shelf_offers(ask),
        }
    }
}

/// The sheet's body, split out so it takes the sentences by value: the outer view
/// answers "is there still a question?" on every run, and this one is built once
/// per answer with an answer it can keep.
///
/// Named for the question it answers rather than for being a sheet — the
/// collision sheet beside it in `conflict_modal` is a sheet too, and two private
/// components of one name in one feature folder are two components a reader has
/// to open both files to tell apart.
#[component]
fn ShelfSheet(state: AppState, info: ShelfAskInfo) -> impl IntoView {
    let ShelfAskInfo {
        heading,
        subtitle,
        question,
        show_note,
        new_note,
        merge_note,
        replace_note,
        offers,
    } = info;

    view! {
        <QuestionSheet
            heading=heading
            subtitle=subtitle
            question=question
            on_close=Callback::new(move |_| conflict::cancel_shelf(state))
            cancel_title="Import nothing".to_string()
        >
                    {if offers.contains(&Placement::LinkOnly) {
                        // The read-at-place arrival's two, which is
                        // `conflict::shelf_offers`' answer rather than a second
                        // spelling of the condition here: a pointer at the shelf
                        // that is here, or the folder's books joining it. No *as
                        // new* — a second shelf of one linked folder is the
                        // second instance the family gate exists to prevent — and
                        // no *replace*: a linked tree is not the level's to empty.
                        view! {
                            <>
                                <ChoiceRow
                                    label="Make link"
                                    note=LINK_NOTE.to_string()
                                    on_click=Callback::new(move |_| {
                                        conflict::answer_shelf(state, Placement::LinkOnly)
                                    })
                                />
                                <ChoiceRow
                                    label="Merge into it"
                                    note=merge_note
                                    on_click=Callback::new(move |_| {
                                        conflict::answer_shelf(state, Placement::Merge)
                                    })
                                />
                            </>
                        }
                            .into_any()
                    } else {
                        // The stored arrival's three, the level's own: go and
                        // look, replace what is here, or a shelf of the next
                        // free name. No pointer — a stored import is a second
                        // instance the library owns, and "show me the first"
                        // is a light rather than a row.
                        view! {
                            <>
                                <ChoiceRow
                                    label="Show it"
                                    note=show_note
                                    on_click=Callback::new(move |_| {
                                        conflict::answer_shelf(state, Placement::Open)
                                    })
                                />
                                <ChoiceRow
                                    label="Replace"
                                    note=replace_note
                                    on_click=Callback::new(move |_| {
                                        conflict::answer_shelf(state, Placement::Replace)
                                    })
                                />
                                <ChoiceRow
                                    label="Add as new"
                                    note=new_note
                                    on_click=Callback::new(move |_| {
                                        conflict::answer_shelf(state, Placement::KeepBoth)
                                    })
                                />
                            </>
                        }
                            .into_any()
                    }}
        </QuestionSheet>
    }
}
