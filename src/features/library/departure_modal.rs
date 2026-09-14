//! "Moving this shelf makes a copy."
//!
//! The question a hand gets when it takes a read-at-place shelf off the seat its folder's tree
//! names for it — the one move in the library that asks BEFORE it runs
//! (`crate::services::library::arrange`).

use leptos::prelude::*;

use library_core::text::plural;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::arrange::{ReturnPath, ShelfDepartureAsk};
use crate::services::library::{answer_departure_return, cancel_departure, confirm_departure};
use crate::state::AppState;

#[component]
pub(crate) fn ShelfDepartureModal(state: AppState) -> impl IntoView {
    let open = state.library.shelf_departure.open;

    Effect::new(move |_| {
        if !open.get() {
            state.library.shelf_departure.ask.set(None);
        }
    });

    view! {
        <ModalShell
            open=open
            aria_label="Moving this shelf makes a copy"
            width="min(92vw, 420px)"
        >
            {move || {
                let ask = state.library.shelf_departure.ask.get()?;
                let info = DepartureInfo::of(&ask);
                Some(view! { <DepartureSheet state=state info=info /> }.into_any())
            }}
        </ModalShell>
    }
}

struct DepartureInfo {
    heading: String,
    subtitle: String,
    lines: Vec<String>,
    promise: String,
    confirm_label: &'static str,
    return_lines: Vec<String>,
    return_label: &'static str,
}

impl DepartureInfo {
    fn of(ask: &ShelfDepartureAsk) -> Self {
        let one = ask.departing.len() == 1;
        let heading = match ask.departing.first() {
            Some(first) if one => first.name.clone(),
            Some(_) => plural(ask.departing.len(), "shelf", "shelves"),
            None => plural(0, "shelf", "shelves"),
        };
        let books: usize = ask.departing.iter().map(|each| each.books).sum();
        let subtitle = if books == 0 {
            "Read at place — moving makes it the library's own".to_string()
        } else {
            format!(
                "Read at place — moving copies {}",
                plural(books, "book", "books")
            )
        };
        let lines = ask
            .departing
            .iter()
            .map(|each| {
                if each.books == 0 {
                    format!(
                        "“{}” is read in place inside “{}” — the copy lands as “{}”.",
                        each.name, each.folder_name, each.copy_name
                    )
                } else {
                    format!(
                        "“{}” is read in place inside “{}” — the copy takes its {} \
                         with it and lands as “{}”.",
                        each.name,
                        each.folder_name,
                        plural(each.books, "book", "books"),
                        each.copy_name
                    )
                }
            })
            .collect();
        let promise = if books == 0 {
            "Nothing inside is read in place, so no bytes are copied. The folder on \
             disk is untouched — import it again and its shelves come back where they \
             were, lit up."
                .to_string()
        } else {
            "Copied books keep their names, highlights and places in them; their bytes \
             move into the library's store. The folder on disk is untouched — import \
             it again and its shelves come back where they were, with the books in \
             their old names, lit up."
                .to_string()
        };
        let return_lines = ask
            .returns
            .iter()
            .map(|each| match &each.path {
                ReturnPath::Reclaim { family_name, .. } => format!(
                    "“{}” goes back inside “{}”, on the shelf its directory names. \
                     Nothing is copied.",
                    each.name, family_name
                ),
                ReturnPath::Reseat { family_name, .. } => format!(
                    "“{}” goes back to the shelf “{}” names for it. Nothing is \
                     copied.",
                    each.name, family_name
                ),
            })
            .collect::<Vec<_>>();
        Self {
            heading,
            subtitle,
            lines,
            promise,
            confirm_label: if one {
                "Move as a copy"
            } else {
                "Move as copies"
            },
            return_label: if ask.returns.len() == 1 {
                "Put it back in its place"
            } else {
                "Put them back in their places"
            },
            return_lines,
        }
    }
}

#[component]
fn DepartureSheet(state: AppState, info: DepartureInfo) -> impl IntoView {
    let DepartureInfo {
        heading,
        subtitle,
        lines,
        promise,
        confirm_label,
        return_lines,
        return_label,
    } = info;
    let has_return = !return_lines.is_empty();

    view! {
        <>
            <SheetHeader
                heading=heading
                subtitle=subtitle
                on_close=Callback::new(move |_| cancel_departure(state))
            />
            <SheetBody>
                {lines
                    .into_iter()
                    .map(|line| {
                        view! { <p class="text-xs text-muted">{line}</p> }
                    })
                    .collect::<Vec<_>>()}
                <p class="text-xs text-muted mt-3">{promise}</p>
                {has_return.then(move || {
                    view! {
                        <div class="mt-3 border-t border-line pt-3">
                            {return_lines
                                .into_iter()
                                .map(|line| {
                                    view! { <p class="text-xs text-muted">{line}</p> }
                                })
                                .collect::<Vec<_>>()}
                        </div>
                    }
                })}
            </SheetBody>
            <SheetFooter>
                <Button
                    on_click=move |_| cancel_departure(state)
                    variant=ButtonVariant::Toolbar
                    title="Leave the shelf where the folder's tree put it"
                >
                    <span>"Cancel"</span>
                </Button>
                {has_return.then(move || {
                    view! {
                        <Button
                            on_click=move |_| answer_departure_return(state)
                            variant=ButtonVariant::Toolbar
                            title="Return each shelf to the place its folder names; nothing is copied"
                        >
                            <span>{return_label}</span>
                        </Button>
                    }
                })}
                <Button
                    on_click=move |_| confirm_departure(state)
                    variant=ButtonVariant::Primary
                    title="Copy the shelf and its read-at-place books, and move the copies"
                >
                    <span>{confirm_label}</span>
                </Button>
            </SheetFooter>
        </>
    }
}
