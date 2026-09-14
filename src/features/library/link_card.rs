//! A link row: the shelf's own card and row shape, with a pointer's facts on it instead of a
//! book's.
//!
//! A link has no address to read, no page to render art from, no resume point and no format.
//! What it has is a name, a target, and the promise that a tap goes there.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

use crate::features::library::entry::{EntryDescriptor, EntryShell};
use crate::features::library::gestures::link_policy;
use crate::features::library::list::row_indent;
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::shelf_item::SeamVocab;
use crate::state::AppState;

fn link_line(to_shelf: bool) -> &'static str {
    if to_shelf {
        "Link · opens the folder where it is"
    } else {
        "Link · opens the book where it is"
    }
}

fn link_title(to_shelf: bool) -> &'static str {
    if to_shelf {
        "A pointer at a folder, not a second one"
    } else {
        "A pointer at a book, not a copy of one"
    }
}

#[component]
pub(crate) fn LinkCard(state: AppState, id: String, name: String, to_shelf: bool) -> impl IntoView {
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");

    let remove_id = id.clone();
    let entry = EntryDescriptor {
        id: id.clone(),
        vocab: SeamVocab::GridCard,
        base_class: "book-card book-link",
        policy: link_policy(state, &id, &name, None),
    };
    let remove = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        remove_sheet.ask(&remove_id);
    };

    view! {
        <EntryShell state=state entry=entry>
            <div class="book-cover-wrap">
                <div class="book-cover" style:aspect-ratio="210 / 297">
                    <div class="book-cover-fallback">
                        <span>{name.clone()}</span>
                    </div>
                    <span class="book-link-badge" title=link_title(to_shelf)>
                        <Icon name=IconName::Link size=11 />
                    </span>
                </div>
            </div>

            <div class="book-info">
                <span class="book-title" title=name.clone()>{name.clone()}</span>
                <span class="book-page">{link_line(to_shelf)}</span>
            </div>

            <button
                type="button"
                class="book-remove"
                title="Remove this link"
                aria-label=move || format!("Remove the link to {}", name.clone())
                on:click=remove
            >
                <Icon name=IconName::Close size=12 />
            </button>
        </EntryShell>
    }
}

#[component]
pub(crate) fn LinkRow(
    state: AppState,
    id: String,
    name: String,
    /// The first letter of the target's id, which the mint guarantees answers (`library_core::id::is_shelf`). The words a link wears, and nothing else: the tap's routing is `crate::services::document::open_row`'s.
    to_shelf: bool,
    depth: usize,
    parent: Option<String>,
) -> impl IntoView {
    let remove_sheet = use_context::<RemoveSheet>();

    let remove_id = id.clone();
    let entry = EntryDescriptor {
        id: id.clone(),
        vocab: SeamVocab::ListRow,
        base_class: "lib-row book-link",
        policy: link_policy(state, &id, &name, parent),
    };
    let indent = row_indent(depth);
    let tooltip = name.clone();

    view! {
        <EntryShell state=state entry=entry style=indent>
            <span class="lib-row-ext" title=link_title(to_shelf)>
                <Icon name=IconName::Link size=12 />
            </span>
            <span class="min-w-0 flex-1">
                <span class="block truncate text-sm font-semibold text-ink" title=tooltip.clone()>
                    {name.clone()}
                </span>
                <span class="block truncate text-xs text-muted">{link_line(to_shelf)}</span>
            </span>
            {move || {
                remove_sheet.map(|sheet| {
                    let at = remove_id.clone();
                    view! {
                        <button
                            class="lib-row-action"
                            type="button"
                            title="Remove this link"
                            aria-label=format!("Remove the link to {}", name.clone())
                            on:click=move |ev: leptos::ev::MouseEvent| {
                                ev.stop_propagation();
                                sheet.ask(&at);
                            }
                        >
                            <Icon name=IconName::Close size=12 />
                        </button>
                    }
                })
            }}
        </EntryShell>
    }
}
