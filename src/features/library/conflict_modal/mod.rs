//! The collision sheet: the level already holds a book of this name, and the
//! question is which of three things the reader meant.
//!
//! One sheet, one question, three rows — and which three is the arrival's own
//! fact, because an import and a move are different questions.

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

/// The open flag lives on the library state rather than a provided handle:
/// the raisers are services, and an import asks from inside a spawned future
/// no component owns.
#[component]
pub(crate) fn ConflictModal(state: AppState) -> impl IntoView {
    let open = state.library.conflict.open;

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
                let spec = if ask.kind.is_folder_merge() {
                    describe_folder_merge(state, &ask)
                } else if ask.kind.is_two_answer() {
                    describe_covered(state, &ask)
                } else {
                    describe_name(state, &ask)
                };
                Some(view! { <ConflictSheet state=state spec=spec /> }.into_any())
            }}
        </ModalShell>
    }
}

/// A second host rather than a second question on the first: a book
/// collision is raised inside a running walk and can queue, while the
/// folder's is asked before the walk starts.
#[component]
pub(crate) fn ShelfConflictModal(state: AppState) -> impl IntoView {
    let open = state.library.shelf_conflict.open;

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
