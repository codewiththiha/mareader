//! The grid: the folders at this level, then the books, then the add card.
//!
//! One CSS grid holds all three, and a folder is a cell of it exactly like a
//! book's card — which is what makes the library nestable.

use leptos::html;
use leptos::prelude::*;

use library_core::view::{COLUMNS_MAX, CoverFit, LibraryView};

use crate::features::library::add_menu::{AddFace, AddMenuButton};
use library_core::book::Row;

use crate::features::library::book_card::BookCard;
use crate::features::library::content::{FolderOrder, ShelfOrder};
use crate::features::library::folder_card::FolderCard;
use crate::features::library::link_card::LinkCard;

#[component]
pub(crate) fn GridView(state: crate::context::LibraryContext) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let folders = use_context::<FolderOrder>().expect("the library content provides the folders");
    let crop = Signal::derive(move || state.library.view.with(|v| v.cover == CoverFit::Crop));
    let columns = Signal::derive(move || state.library.view.with(|v| v.columns_token()));
    let grid_ref: NodeRef<html::Div> = NodeRef::new();

    // Report the computed track count so the stepper's `+` starts from what
    // the shelf shows (5 → 6) rather than from 1. The write cannot re-layout:
    // `auto_fit` is deliberately no part of `columns_token`.
    Effect::new(move |_| {
        let Some(node) = grid_ref.get() else {
            return;
        };
        if state.library.view.with(|v| v.columns.is_some()) {
            return;
        }
        let view = state.library.view;
        let report = move || {
            let Some(tracks) = web_sys::window()
                .and_then(|w| w.get_computed_style(&node).ok().flatten())
                .and_then(|style| style.get_property_value("grid-template-columns").ok())
            else {
                return;
            };
            if tracks == "none" {
                return;
            }
            let count = tracks.split_whitespace().count();
            if count == 0 {
                return;
            }
            // The same clamp [`LibraryView::report_auto_fit`] applies: a
            // measurement clamped one way and a report clamped another would
            // never compare equal and would write on every resize.
            let fit = LibraryView::clamped_fit(u8::try_from(count).unwrap_or(COLUMNS_MAX));
            if view.with_untracked(|v| v.auto_fit) != fit {
                view.update(|v| v.report_auto_fit(fit));
            }
        };
        report();
        let handle = window_event_listener(leptos::ev::resize, move |_| report());
        on_cleanup(move || handle.remove());
    });

    view! {
        <div
            node_ref=grid_ref
            class="lib-grid"
            class=("lib-grid-selecting", move || state.library.selecting.get())
            style=move || format!("--lib-cols:{}", columns.get())
        >
            <For each=move || folders.0.get() key=|s| s.id.clone() let:shelf>
                <FolderCard state=state shelf=shelf />
            </For>
            <For each=move || order.0.get() key=|r| r.id().to_string() let:row>
                {match row {
                    Row::Book(book) => {
                        view! { <BookCard state=state book=book crop=crop /> }.into_any()
                    }
                    Row::Link { id, target, .. } => {
                        let to_shelf = library_core::id::is_shelf(&target);
                        // Read back by id: a keyed card is not re-created
                        // when the row's name changes.
                        let name = state.library.row_name_signal(&id);
                        view! { <LinkCard state=state id=id name=name to_shelf=to_shelf /> }.into_any()
                    }
                }}
            </For>
            <AddMenuButton state=state face=AddFace::Card />
        </div>
    }
}
