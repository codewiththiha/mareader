//! "This book is not where the library left it."
//!
//! The question a click on a missing book asks instead of opening the reader onto an error: two
//! doors that point the row at the file it is now — pick the file yourself, or name a folder and
//! let the app walk it looking for a file of the book's own name — plus a Cancel that changes
//! nothing.

use leptos::prelude::*;

use ui_kit::menu::choice_row::ChoiceRow;
use ui_kit::overlay::modal_shell::ModalShell;
use ui_kit::overlay::question_sheet::QuestionSheet;
use crate::services::library::{cancel_relink, relink_dialog, relink_search_folder};
use crate::state::AppState;

#[component]
pub(crate) fn RelinkModal(state: AppState) -> impl IntoView {
    let open = state.library.relink.open;

    view! {
        <ModalShell
            open=open
            aria_label="This book is not where the library left it"
            width="min(92vw, 420px)"
        >
            {move || {
                let ask = state.library.relink.ask.get()?;
                let name = ask.name.clone();
                let pick_id = ask.book_id.clone();
                let search_id = ask.book_id;
                let question = format!(
                    "“{name}” is not at the address the library reads it from any \
                     more. Nothing is lost while it is missing: the row keeps its \
                     shelves, its place in the book and its highlights. Point it at \
                     the file it is now — or name a folder and let the app look \
                     inside for a file of this name."
                );
                let search_note =
                    format!("Walk a folder you pick and open the first “{name}” inside it");
                Some(
                    view! {
                        <QuestionSheet
                            heading=name
                            subtitle="Not where the library left it"
                            question=question
                            on_close=Callback::new(move |_| cancel_relink(state))
                            cancel_title="Leave the book missing".to_string()
                        >
                            <ChoiceRow
                                label="Choose the file…"
                                note="Pick the document this book reads from now"
                                on_click=Callback::new(move |_| {
                                    cancel_relink(state);
                                    relink_dialog(state, pick_id.clone());
                                })
                            />
                            <ChoiceRow
                                label="Search a folder…"
                                note=search_note
                                on_click=Callback::new(move |_| {
                                    cancel_relink(state);
                                    relink_search_folder(state, search_id.clone());
                                })
                            />
                        </QuestionSheet>
                    }
                        .into_any(),
                )
            }}
        </ModalShell>
    }
}
