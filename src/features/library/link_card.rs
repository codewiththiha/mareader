//! A link row: the shelf's own card and row shape, with a pointer's facts on it
//! instead of a book's.
//!
//! A link is not a book and cannot borrow a book's card — it has no address to
//! read, no page to render art from, no resume point to draw a bar for and no
//! format to name. What it has is a name (the name of the book it points at,
//! which is what makes it recognisable beside it), a target, and the same three
//! gestures every other row on the shelf answers to: a tap goes to the book, a
//! hold selects it, a movement files it somewhere else. So it wears the shelf's
//! own item shell — `crate::features::library::shelf_item`, the one wiring a
//! card and a list row both wear — and the plate where a cover would be says
//! what it is instead.
//!
//! The one thing a link does that no book does is open somewhere else: a tap
//! reveals the book it points at, on the shelf that book is filed on, and lights
//! its card (`crate::services::library::reveal_book`). That is the whole of the
//! row's purpose, and it is why a link is a row the reader can file anywhere
//! without ever moving a file.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

use crate::features::library::entry::{EntryDescriptor, EntryShell};
use crate::features::library::gestures::link_policy;
use crate::features::library::list::row_indent;
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::shelf_item::SeamVocab;
use crate::state::AppState;

/// The line a link carries where a book carries its author or its page: what
/// it is, and that a tap goes to the thing rather than opening a file. A
/// folder link says folder: the promise has to name what the tap reveals.
fn link_line(to_shelf: bool) -> &'static str {
    if to_shelf {
        "Link · opens the folder where it is"
    } else {
        "Link · opens the book where it is"
    }
}

/// The badge and chip's own sentence, in the same two voices.
fn link_title(to_shelf: bool) -> &'static str {
    if to_shelf {
        "A pointer at a folder, not a second one"
    } else {
        "A pointer at a book, not a copy of one"
    }
}

/// One link on the grid.
#[component]
pub(crate) fn LinkCard(state: AppState, id: String, name: String, to_shelf: bool) -> impl IntoView {
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");

    let remove_id = id.clone();
    let entry = EntryDescriptor {
        id: id.clone(),
        vocab: SeamVocab::GridCard,
        base_class: "book-card book-link",
        // No shelf of its own: a link card is drawn by the open level, which
        // is the container the session resolves a nameless lift to.
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

/// One link in the list, at the depth its branch puts it at.
#[component]
pub(crate) fn LinkRow(
    state: AppState,
    id: String,
    name: String,
    /// Whether the target is a shelf rather than a book — the first letter of
    /// the target's id, which the mint guarantees is an answer
    /// (`library_core::id::is_shelf`). The words a link wears, and nothing
    /// else: the tap's routing is `crate::services::document::open_row`'s.
    to_shelf: bool,
    depth: usize,
    /// The shelf whose member list renders this row — the tree's own id inside
    /// an expanded branch, `None` in the flat section. See
    /// `crate::features::library::list::ListRow` for why a row carries it.
    parent: Option<String>,
) -> impl IntoView {
    // Asked for rather than expected, the way the list's own row asks: the
    // sidebar's tree mounts this row with no sheet under it, and a row with no
    // sheet has no ✕ to draw.
    let remove_sheet = use_context::<RemoveSheet>();

    let remove_id = id.clone();
    let entry = EntryDescriptor {
        id: id.clone(),
        vocab: SeamVocab::ListRow,
        base_class: "lib-row book-link",
        // The shelf whose member list drew this row: the tree's own id inside
        // an expanded branch, `None` in the flat section.
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
