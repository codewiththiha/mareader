//! The compact per-file sheet a folder merge asks: two names, three answers,
//! and the switch that gives every waiting question the same answer in one
//! click.

use leptos::prelude::*;

use library_core::book::find_row;
use library_core::conflict::{next_name, Placement};

use crate::components::primitives::menu::choice_row::ChoiceRow;
use crate::components::primitives::overlay::question_sheet::QuestionSheet;
use crate::services::library::conflict::{self, ConflictAsk};

use super::info::more_waiting;
use crate::state::AppState;


/// The compact per-file sheet a folder merge asks: two names, three answers,
/// and — behind the row of them — the switch that gives every waiting question
/// the same answer in one click.
///
/// The move sheet's three, re-spelled for an arrival with no row of its own:
/// there is nothing to fold INTO the shelf's row yet, so *merge* is the file
/// handing the row its measurement rather than two rows becoming one. Smaller
/// than the import sheet on purpose: the shelf's question is already answered,
/// and a sheet that re-explained the whole situation per file would be a
/// sentence the reader has to re-read forty times.
#[component]
pub(super) fn FolderMergeSheet(state: AppState, ask: ConflictAsk) -> impl IntoView {
    // The switch starts off on every question: "apply to all" is the reader's
    // answer per sheet, not a preference the first click leaves behind.
    let apply_all = RwSignal::new(false);
    let waiting = state
        .library
        .conflict_waiting
        .with_untracked(|w| w.iter().filter(|each| each.kind.is_folder_merge()).count());
    let incoming = ask.arrival.name.clone();
    let existing = ask.existing_name.clone();
    let heading = incoming.clone();
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
    let merge_note =
        format!("One book — “{existing}” stays, and takes this file's measurement");
    const REPLACE_NOTE: &str =
        "The row on the shelf leaves the library; this file takes its slot";
    let new_name = {
        let (rows, shelves) = state.library.snapshot_rows();
        next_name(&rows, &shelves, &ask.arrival.shelf_id, &incoming)
    };
    let new_note = format!("Keep both — this file becomes “{new_name}”");

    view! {
        <QuestionSheet
            heading=heading
            subtitle=subtitle
            question=question
            on_close=Callback::new(move |_| conflict::cancel(state))
            apply_all=(waiting, apply_all)
        >
                    <ChoiceRow
                        label="Merge"
                        note=merge_note
                        on_click=Callback::new(move |_| {
                            conflict::answer_folder_merge(
                                state,
                                Placement::Merge,
                                apply_all.get_untracked(),
                            )
                        })
                    />
                    <ChoiceRow
                        label="Replace"
                        note=REPLACE_NOTE.to_string()
                        on_click=Callback::new(move |_| {
                            conflict::answer_folder_merge(
                                state,
                                Placement::Replace,
                                apply_all.get_untracked(),
                            )
                        })
                    />
                    {(!twin).then(move || {
                        view! {
                            <ChoiceRow
                                label="As new"
                                note=new_note.clone()
                                on_click=Callback::new(move |_| {
                                    conflict::answer_folder_merge(
                                        state,
                                        Placement::KeepBoth,
                                        apply_all.get_untracked(),
                                    )
                                })
                            />
                        }
                    })}
        </QuestionSheet>
    }
}
