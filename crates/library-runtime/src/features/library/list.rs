//! The list: the level as a tree of shelf and book rows.

use std::collections::HashSet;
use std::time::Duration;

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::{Book, Row};
use library_core::query;
use library_core::shelf::{Shelf, children_of, find};
use library_core::sort;
use library_core::view::CoverFit;
use reader_core::format::Format;

use crate::features::library::add_menu::{AddFace, AddMenuButton};
use crate::features::library::content::{ShelfOrder, level_folders};
use crate::features::library::cover_thumb::CoverThumb;
use crate::features::library::dnd::controller::DragController;
use crate::features::library::entry::{EntryDescriptor, EntryShell};
use crate::features::library::facts::book_facts;
use crate::features::library::folder_badge::FolderBadge;
use crate::features::library::folder_card::summary;
use crate::features::library::gestures::{book_policy, folder_policy};
use crate::features::library::link_card::LinkRow;
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::selection::SelectionCheck;
use crate::features::library::shelf_item::SeamVocab;

#[derive(Clone, Copy)]
struct TreeCtx {
    expanded: RwSignal<HashSet<String>>,
}

pub(crate) fn row_indent(depth: usize) -> String {
    format!("padding-left:{}rem", 0.75 + depth as f32 * 0.9)
}

const AUTO_EXPAND_MS: u64 = 650;

#[component]
pub(crate) fn ListView(state: crate::context::LibraryContext) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let crop = Signal::derive(move || state.library.view.with(|v| v.cover == CoverFit::Crop));
    let expanded: RwSignal<HashSet<String>> = RwSignal::new(HashSet::new());
    provide_context(TreeCtx { expanded });

    // The tree and the leaves share one query, so a search
    // narrows both or neither.
    let roots = Signal::derive(move || level_folders(state, None));

    view! {
        <div
            class="lib-list divide-y divide-line rounded-xl border border-line"
            class=("lib-list-selecting", move || state.library.selecting.get())
        >
            <For each=move || roots.get() key=|s| s.id.clone() let:shelf>
                <TreeRow state=state shelf=shelf depth=0 crop=crop />
            </For>
            <For each=move || order.0.get() key=|r| r.id().to_string() let:row>
                {row_view(state, row, crop, 0, None)}
            </For>
            <AddMenuButton state=state face=AddFace::Row />
        </div>
    }
}

#[component]
fn TreeRow(
    state: crate::context::LibraryContext,
    shelf: Shelf,
    depth: usize,
    crop: Signal<bool>,
) -> impl IntoView {
    let ctx = use_context::<TreeCtx>().expect("the list provides the tree context");
    let drag = use_context::<DragController>();

    // Identity only: a keyed row survives, so movable facts are read by id.
    let id = shelf.id.clone();

    // Hiding a matching shelf but keeping its children filters
    // the leaves and not the tree.
    let kids_id = id.clone();
    let kids = Signal::derive(move || {
        let terms = state.library.query.get();
        state.library.shelves.with(|shelves| {
            children_of(shelves, Some(kids_id.as_str()))
                .into_iter()
                .filter(|s| query::matches_terms(&s.name, &terms))
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let members_id = id.clone();
    let members = Signal::derive(move || {
        state.library.shelves.with(|shelves| {
            find(shelves, &members_id)
                .map(|s| s.books.clone())
                .unwrap_or_default()
        })
    });
    let name = state.library.shelf_name_signal(&id);

    let badge_id = id.clone();
    let open_id = id.clone();
    let open = Signal::derive(move || ctx.expanded.with(|set| set.contains(&open_id)));

    // Hover-to-expand: a reader carrying books should not have to put them
    // down to knock.
    let hover_id = id.clone();
    let hover_collapsed = Signal::derive(move || {
        drag.is_some_and(|each| each.live().get() && each.over_folder(&hover_id)) && !open.get()
    });
    let expand_id = id.clone();
    Effect::new(move |_| {
        if !hover_collapsed.get() {
            return;
        }
        let at = expand_id.clone();
        let expanded = ctx.expanded;
        let handle = set_timeout_with_handle(
            move || {
                // A queued expand can outlive the row list.
                let _ = expanded.try_update(|set| {
                    set.insert(at);
                });
            },
            Duration::from_millis(AUTO_EXPAND_MS),
        )
        .ok();
        on_cleanup(move || {
            if let Some(handle) = handle {
                handle.clear();
            }
        });
    });

    let books = member_books(state, members);
    let searching = Signal::derive(move || state.library.query.with(|q| query::is_active(q)));
    let shown_books = Signal::derive(move || {
        if searching.get() {
            Vec::new()
        } else {
            books.get()
        }
    });

    let toggle_id = id.clone();
    let toggle = Callback::new(move |_| {
        let at = toggle_id.clone();
        ctx.expanded.update(|set| {
            if !set.remove(&at) {
                set.insert(at);
            }
        });
    });

    // The grid's press contract with the disclosure's answers:
    // tap unfolds, hold selects, movement lifts.
    let entry = EntryDescriptor {
        id: id.clone(),
        vocab: SeamVocab::FolderRow,
        base_class: "lib-row lib-row-shelf",
        policy: folder_policy(&id, name, toggle, None),
    };

    let nav_id = id.clone();
    // In a `StoredValue` because the unfold's `Show` children
    // closure must stay an `Fn`.
    let members_parent: StoredValue<Option<String>, LocalStorage> =
        StoredValue::new_local(Some(id.clone()));
    let indent = row_indent(depth);

    view! {
        <>
            <EntryShell
                state=state
                entry=entry
                style=indent
                aria_expanded=open
                on_keydown_first=Callback::new(move |ev: leptos::ev::KeyboardEvent| {
                    if ev.key() == " " {
                        ev.prevent_default();
                        toggle.run(());
                        return true;
                    }
                    false
                })
            >
                {move || {
                    let glyph = if open.get() {
                        IconName::ChevronDown
                    } else {
                        IconName::Next
                    };
                    view! { <Icon name=glyph size=13 class="shrink-0 text-muted" /> }
                }}
                <Icon name=IconName::Outline size=14 class="shrink-0 text-muted" />
                <span
                    class="min-w-0 flex-1 truncate text-sm font-semibold text-ink"
                    title=move || name.get()
                >
                    {move || name.get()}
                </span>
                <FolderBadge state=state shelf_id=badge_id class="folder-mode ml-2" />
                <span class="shrink-0 text-xs text-muted">
                    {move || summary((members.with(|m| m.len()), kids.get().len()))}
                </span>
                <button
                    class="icon-ghost lib-row-action"
                    type="button"
                    title="Open shelf"
                    aria-label=move || format!("Open the {} shelf", name.get())
                    on:click=move |ev: leptos::ev::MouseEvent| {
                        ev.stop_propagation();
                        state.library.shelf.set(nav_id.clone());
                    }
                >
                    <Icon name=IconName::Open size=12 />
                </button>
            </EntryShell>
            <Show when=move || open.get()>
                <For each=move || kids.get() key=|s| s.id.clone() let:child>
                    // Through `AnyView`: a recursive component's opaque
                    // return type would never resolve.
                    {view! { <TreeRow state=state shelf=child depth=depth + 1 crop=crop /> }
                        .into_any()}
                </For>
                <For each=move || shown_books.get() key=|r| r.id().to_string() let:row>
                    {row_view(state, row, crop, depth + 1, members_parent.get_value())}
                </For>
            </Show>
        </>
    }
}

/// The shelf's own order under the view's sort, the same call the
/// page's level runs.
fn row_view(
    state: crate::context::LibraryContext,
    row: Row,
    crop: Signal<bool>,
    depth: usize,
    parent: Option<String>,
) -> AnyView {
    match row {
        Row::Book(book) => view! {
            <ListRow state=state book=book crop=crop depth=depth parent=parent />
        }
        .into_any(),
        Row::Link { id, target, .. } => {
            let to_shelf = library_core::id::is_shelf(&target);
            // Read back by id, for the same reason the grid's card does.
            let name = state.library.row_name_signal(&id);
            view! { <LinkRow state=state id=id name=name to_shelf=to_shelf depth=depth parent=parent /> }
                .into_any()
        }
    }
}

fn member_books(
    state: crate::context::LibraryContext,
    members: Signal<Vec<String>>,
) -> Signal<Vec<Row>> {
    Signal::derive(move || {
        let ids = members.get();
        let view = state.library.view.get();
        state
            .library
            .books
            .with(|books| sort::ordered(books, &ids, view.sort, view.sort_asc))
    })
}

#[component]
fn ListRow(
    state: crate::context::LibraryContext,
    book: Book,
    crop: Signal<bool>,
    depth: usize,
    /// The tree's own id for an expanded row; `None` for the flat
    /// section.
    parent: Option<String>,
) -> impl IntoView {
    let remove_sheet = use_context::<RemoveSheet>();

    // Identity only: a keyed row survives, so movable facts are read by id.
    let id = book.id.clone();
    let facts = book_facts(state, &id);
    let chip = (book.format != Format::Pdf).then(|| book.format.label().to_string());

    let check_id = id.clone();

    // The grid's press contract. A movement is always a drag here.
    let entry = EntryDescriptor {
        id: id.clone(),
        vocab: SeamVocab::ListRow,
        base_class: "lib-row",
        policy: book_policy(state, &id, facts, parent.clone()),
    };

    let missing_class =
        Signal::derive(move || facts.with(|f| f.as_ref().is_some_and(|x| x.missing)));
    let remove_id = id;
    let indent = row_indent(depth);

    view! {
        <EntryShell
            state=state
            entry=entry
            style=indent
            extra_classes=vec![("row-missing".to_string(), missing_class)]
        >
            <span class="lib-row-cover" class=("book-cover-crop", move || crop.get())>
                <SelectionCheck state=state id=check_id />
                <CoverThumb
                    state=state
                    path=Signal::derive(move || {
                        facts
                            .with(|f| f.as_ref().map(|x| x.path.clone()).unwrap_or_default())
                    })
                    alt=Signal::derive(move || {
                        facts
                            .with(|f| f.as_ref().map(|x| x.title.clone()).unwrap_or_default())
                    })
                    img_class="lib-row-img"
                />
            </span>
            <span class="min-w-0 flex-1">
                <span
                    class="block truncate text-sm font-semibold text-ink"
                    title=move || {
                        facts.with(|f| f.as_ref().map(|x| x.title.clone()).unwrap_or_default())
                    }
                >
                    {move || {
                        facts.with(|f| f.as_ref().map(|x| x.title.clone()).unwrap_or_default())
                    }}
                </span>
                <span
                    class="block truncate text-xs text-muted"
                    title=move || {
                        facts.with(|f| f.as_ref().map(|x| x.path.clone()).unwrap_or_default())
                    }
                >
                    {move || {
                        facts.with(|f| {
                            f.as_ref().map(|x| x.author_line.clone()).unwrap_or_default()
                        })
                    }}
                </span>
            </span>
            {chip.map(|label| view! { <span class="lib-format lib-row-format">{label}</span> })}
            {move || {
                facts.get().and_then(|f| f.percent()).map(|p| {
                    view! {
                        <span class="shrink-0 text-xs tabular-nums text-muted">{p}</span>
                    }
                })
            }}
            {move || {
                remove_sheet.map(|sheet| {
                    let at = remove_id.clone();
                    view! {
                        <button
                            class="icon-ghost lib-row-action"
                            type="button"
                            title="Remove from library"
                            aria-label="Remove from library"
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
