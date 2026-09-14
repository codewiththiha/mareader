//! The collision sheet: the level already holds a book of this name, and the
//! question is which of three things the reader meant.
//!
//! One sheet, one question, three rows — and WHICH three is the arrival's own
//! fact, because an import and a move are different questions. A file arriving
//! has nothing of its own yet, so its answers are about what to put here:
//! *already imported* places nothing and takes the reader to the row that is
//! already there, *add as new* keeps both under the next free name, and *make
//! link* puts a pointer here instead of a copy. A row being moved is two books
//! the reader already has, so its answers are about which of them the level
//! keeps: *merge* folds the moved one into the one that is here, *replace*
//! sends the one that is here out of the library and seats the arrival in its
//! place, and *as new* keeps both under the next free name.
//!
//! Neither set gets a second ask. An import's answers cannot destroy anything,
//! so there is nothing to warn about; a move's Replace can, so its row says
//! what goes before the click — the name of the row and how many highlights
//! leave with it — which is the promise-on-the-row idiom the rest of the sheet
//! already keeps.
//!
//! One more shape wears this sheet's chrome without wearing its question: a
//! loose import of a file that sits inside a folder the library reads in
//! place asks about the FILE'S GROUND rather than the level's name — the
//! library's own stored copy here, or the book the folder holds, lit — because
//! a second link of one read-at-place file is the one thing the folder rule
//! never makes.
//!
//! The service half — what a collision is, what each answer writes — is
//! `crate::services::library::conflict` and the rule itself is
//! `library_core::conflict`; this directory is the ask.
//!
//! ## Four questions, one sheet
//!
//! Each question DESCRIBES itself and [`ConflictSheet`] draws it: [`info`] holds
//! the two strings more than one question needs, [`name_sheet`] is the level's
//! own name question, [`folder_merge`] the compact per-file sheet a merged
//! folder asks, [`covered`] the two answers a loose import of a file an
//! in-place tree already holds gets, and [`shelf`] the folder's own name
//! collision, which lives on the shelf state and is mounted by
//! [`ShelfConflictModal`] rather than by [`ConflictModal`].
//!
//! The four used to be four components that each drew their own list of
//! `ChoiceRow`s and wired their own apply-to-all signal. What is left is one
//! renderer, four describers, and one place that knows how an answer lands.
//!
//! Cancel — the button, the backdrop and the Escape key — drops the question on
//! screen and every one waiting behind it, which is what a file manager's copy
//! dialog has always meant by Cancel: the placements already answered keep
//! their answers and the ones not asked simply do not land. The folder's own
//! question is the exception: it has no queue, so cancelling drops the one.

mod covered;
mod folder_merge;
mod info;
mod name_sheet;
mod sheet;
mod shelf;

use leptos::prelude::*;

use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::state::AppState;

use covered::describe_covered;
use folder_merge::describe_folder_merge;
use name_sheet::describe_name;
use sheet::ConflictSheet;
use shelf::describe_shelf;

/// The sheet, mounted once by the library page.
///
/// The open flag lives on the library state rather than in a provided handle
/// (the remove sheet's shape) because the raisers are services: an import asks
/// from inside a spawned future no component owns, and a signal on the state is
/// the one door every raiser and this view already share.
#[component]
pub(crate) fn ConflictModal(state: AppState) -> impl IntoView {
    let open = state.library.conflict.open;

    // A close that came from the lane registry, the Escape key or the shell's
    // backdrop wrote only the boolean; the question and the ones waiting
    // behind it go with it, so the sheet can never reopen onto a question
    // somebody already dismissed.
    Effect::new(move |_| {
        if !open.get() {
            state.library.conflict.ask.set(None);
            state.library.conflict_waiting.set(Vec::new());
        }
    });

    view! {
        <ModalShell
            open=open
            aria_label="The library already holds a book of that name here"
            width="min(92vw, 420px)"
        >
            {move || {
                let ask = state.library.conflict.ask.get()?;
                // A folder merge's file asks wear the compact sheet: the
                // shelf's question is already answered, and what is left is a
                // run of files with the same three doors each.
                let spec = if ask.kind.is_folder_merge() {
                    describe_folder_merge(state, &ask)
                } else if ask.kind.is_two_answer() {
                    // A two-answer ask — a covered file's ground, or content
                    // the library already holds — is about the library rather
                    // than about the level's name, and its sheet is the pair
                    // the name question cannot offer.
                    describe_covered(state, &ask)
                } else {
                    describe_name(state, &ask)
                };
                Some(view! { <ConflictSheet state=state spec=spec /> }.into_any())
            }}
        </ModalShell>
    }
}

/// The folder's question, mounted beside [`ConflictModal`] by the library page.
///
/// A second host rather than a second question on the first, because the two
/// asks live on different states: a book collision is raised from inside a
/// walk that is already running and can queue, and the folder's is asked
/// before the walk starts and is the only thing standing in its way. The shell
/// is the same shell and the body is the same [`ConflictSheet`] — what is
/// separate is the signal, and the sentence the dialog calls itself to a
/// screen reader.
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
                let spec = describe_shelf(state, &ask);
                Some(view! { <ConflictSheet state=state spec=spec /> }.into_any())
            }}
        </ModalShell>
    }
}
