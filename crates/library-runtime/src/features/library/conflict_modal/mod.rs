//! The collision sheet: one question, three rows, and which three is the
//! arrival's own fact.

mod covered;
mod folder_merge;
mod info;
mod name_sheet;
mod sheet;
mod shelf;

use leptos::prelude::*;

use app_ui::components::primitives::overlay::modal_shell::ModalShell;

use covered::describe_covered;
use folder_merge::describe_folder_merge;
use name_sheet::describe_name;
use sheet::ConflictSheet;
use shelf::describe_shelf;

/// On the library state: the raisers are services inside spawned futures.
#[component]
pub(crate) fn ConflictModal(state: crate::context::LibraryContext) -> impl IntoView {
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

/// Its own host: a folder's question is asked before the walk starts.
#[component]
pub(crate) fn ShelfConflictModal(state: crate::context::LibraryContext) -> impl IntoView {
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
