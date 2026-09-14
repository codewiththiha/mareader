//! The list: one row per book and one row per shelf — the level as a tree that
//! unfolds in place, for a library the reader is scanning rather than browsing.
//!
//! Same books, same order, same drag rules and the same three gestures as the
//! grid — the only thing that changes is the shape of a row, which is why the
//! order arrives from the same context signal rather than being derived a second
//! time here. A reader who switched to the denser layout has not thereby lost the
//! way in, the way out, or the hold that starts a selection.
//!
//! Shelves are rows here now, and were not before: the shelf tile only ever
//! rendered in the grid, so the dense layout was the books and the way in. A
//! shelf row unfolds — the shelves filed in it and its own books indent under it,
//! as deep as the forest goes — while the way in stays a separate gesture: the
//! row's Open drills the breadcrumb route, and the row itself only unfolds.
//! Splitting the two is the disclosure's whole contract, because unfolding is a
//! way of LOOKING and must never move the reader: a tree that navigated on expand
//! could not be scanned without being travelled. Which shelves are unfolded is
//! this component's own memory and is never persisted — the tree is a way of
//! looking at the library, and the forest the library stores is the same one
//! whichever layout is showing it.
//!
//! A search divides the tree the way the page divides it: the doors narrow by
//! name, with the same rule `crate::features::library::content` filters the
//! grid's folders with, and the matches themselves are the flat list's alone —
//! an unfolded shelf withholds its members while a query is open, because the
//! root search already lists every match in the library and the same book twice
//! at two indents is one book too many.
//!
//! A row is the same drop target a card is, and registers under the same kind: a
//! book at one density and the same book at the other are one thing to a drag, and
//! a session that had to be told which layout was showing would be a session that
//! could only drop on the one the reader happened to be looking at. The tree's
//! rows carry one fact a card never had to: the shelf whose member list renders
//! them, so a drop inside an expanded branch lands in the branch and not in the
//! level the page is on. A shelf row is a target in its own right — its middle
//! takes a hold inside, its outer quarters reorder held folders beside it, and a
//! hold that rests on a collapsed one opens it, the courtesy every file manager's
//! tree gives a drag. And a shelf row is a LIFT as well as a landing: a hold
//! enters the selection with the shelf in it and a movement picks it up — the
//! same wiring the grid's folder card wears — so a folder is draggable at both
//! densities, watched or not, and a set of books and folders lifts as one.
//!
//! A row shows the author when the book has one and the resume point when it does
//! not: at this density there is room for one line of prose and the reader gets to
//! choose which by opening the book.
//!
//! ## The tree is a component, not a layout
//!
//! [`ShelfTree`] is the whole recipe — which root to walk and how dense to draw —
//! as a plain prop bag, and nothing under it reads a context the reader's sidebar
//! could not provide (the right-click's host is asked for, not expected). The
//! sidebar's shelf tab mounts the same tree with `dense`: file-name rows whose
//! format chip stands in for the cover art, because at that width the kind of
//! thing a row is earns the pixels the art would cost. When it gets there, the
//! rows' gestures stand down with the page's hosts: a tree with no drag session
//! and no menu keeps the tap and the disclosure and nothing else (see
//! `crate::features::library::gestures`).

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
use crate::features::library::cover_thumb::CoverThumb;
use crate::features::library::content::{ShelfOrder, level_folders};
use crate::features::library::dnd::controller::DragController;
use crate::features::library::entry::{EntryDescriptor, EntryShell};
use crate::features::library::folder_card::summary;
use crate::features::library::gestures::{book_policy, folder_policy};
use crate::features::library::link_card::LinkRow;
use crate::features::library::facts::book_facts;
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::selection::SelectionCheck;
use crate::features::library::shelf_item::SeamVocab;
use crate::state::AppState;

/// One level of the shelf tree, expanded in place.
///
/// A plain prop bag on purpose: the reader sidebar's shelf tab will mount the
/// same tree inside its own panel and get the same walk with file-name rows and
/// format chips, with nothing this page provides.
#[derive(Clone, Default)]
pub struct ShelfTree {
    /// Root to walk. `None` follows the level the page is on: the roots of the
    /// whole library at the top of it, and the shelves filed inside a shelf once
    /// the breadcrumb has drilled — the list is a view OF a level, the same level
    /// the grid shows, and the disclosure is how the reader goes deeper without
    /// leaving it. A mount that wants a fixed subtree names its root.
    pub root: Option<String>,
    /// Sidebar density: no counts, no covers-as-art — the format chip and the
    /// file's own name, because at that width the art is the row's whole budget.
    pub dense: bool,
}

/// What every row under a [`ListView`] shares: which shelves the reader has
/// unfolded, and how dense the tree is being drawn.
#[derive(Clone, Copy)]
struct TreeCtx {
    expanded: RwSignal<HashSet<String>>,
    dense: bool,
}

/// The tree's indent scale: the row's own padding at the level, plus one step per
/// depth — enough to read the shape of the forest at a glance, not enough to run
/// a deep row's title out of room.
pub(crate) fn row_indent(depth: usize) -> String {
    format!("padding-left:{}rem", 0.75 + depth as f32 * 0.9)
}

/// How long a hold rests on a collapsed shelf row before the tree opens it: the
/// way deeper is the way IN, and a reader carrying books should not have to put
/// them down to knock. Longer than the fold's own dwell, because opening a level
/// is a navigation the reader has to see happen before they aim into it.
const AUTO_EXPAND_MS: u64 = 650;

#[component]
pub(crate) fn ListView(state: AppState, #[prop(optional)] tree: ShelfTree) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let crop = Signal::derive(move || state.library.view.with(|v| v.cover == CoverFit::Crop));
    let expanded: RwSignal<HashSet<String>> = RwSignal::new(HashSet::new());
    provide_context(TreeCtx {
        expanded,
        dense: tree.dense,
    });

    // The shelves the tree's top level lists: the prop's root when a mount
    // pinned one, else the level the page is on. One query for both densities —
    // `crate::features::library::content::level_folders` is the grid's doors and
    // this tree's, so a search cannot narrow one and not the other. The books a
    // search keeps arrive flat in `order`, and the rows below withhold their
    // members while it is open, so a match is listed once.
    let roots = Signal::derive(move || level_folders(state, tree.root.clone()));

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
            // The grid ends in an add card, so the list ends in an add row: the two
            // layouts are the same library, and a reader who switched to the denser
            // one has not thereby lost the way in.
            <AddMenuButton state=state face=AddFace::Row />
        </div>
    }
}

/// A shelf as a row, and — once unfolded — everything inside it: the shelves
/// filed in it as rows of their own one indent deeper, and its member books as
/// the same rows the level's books get. Recursive because the forest is.
#[component]
fn TreeRow(state: AppState, shelf: Shelf, depth: usize, crop: Signal<bool>) -> impl IntoView {
    let ctx = use_context::<TreeCtx>().expect("the list provides the tree context");
    // The sidebar will mount this tree with no library page under it, and the
    // drag session is the one host the ROW itself still asks about — the
    // tree's hover-to-open courtesy needs to know whether there is a session
    // to serve. The shell asks for the hosts itself and stands the gestures
    // down when they are absent (see `crate::features::library::shelf_item`),
    // so a tree without them keeps the tap and the disclosure and nothing
    // else.
    let drag = use_context::<DragController>();

    // The prop is the shelf the `For` keyed this row on, and a keyed row is not
    // re-created when the shelf's CONTENTS change — a book filed into it, a
    // rename, a shelf nested inside. So everything that can move is read back out
    // of the state by id and the prop supplies the identity: the rule the grid's
    // folder card follows (see `crate::features::library::folder_card`).
    let id = shelf.id.clone();

    // The doors under this row, narrowed by an open query the same way the top
    // level is: a search that hid the matching shelves but kept showing the ones
    // between them would be a filter of the leaves and not of the tree.
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
            find(shelves, &members_id).map(|s| s.books.clone()).unwrap_or_default()
        })
    });
    let name = state.library.shelf_name_signal(&id);
    let open_id = id.clone();
    let open = Signal::derive(move || ctx.expanded.with(|set| set.contains(&open_id)));

    // The tree's courtesy to a drag: a hold resting on a COLLAPSED row opens
    // it, so the way deeper is the way in and the reader never has to put the
    // hold down to knock. The timer belongs to an effect on the hover, so
    // leaving the row — or the row opening by hand — is the cancellation.
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
                expanded.update(|set| {
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

    // A search lists its matches flat — the level's `order` is the whole
    // library's when a query is open — so an unfolded shelf withholds its
    // members while the search is on: the doors stay, to show WHERE the
    // matches live, and the matches themselves are the flat list's alone.
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

    // The row wears the shelf's one press contract — the same wiring the grid's
    // folder card and the book rows wear (see
    // `crate::features::library::gestures`) with the disclosure's own answers: a
    // tap unfolds, a hold enters the selection with this shelf in it, and a
    // movement lifts it, watched or not — a hand-move is marked on the row and
    // the next re-hang passes it by. What the shell gets besides the policy is
    // the disclosure's two facts of its own: its expanded state for the aria,
    // and the Space key it owns before the shared keyboard halves.
    // The row's own fact, described rather than wired: a folder in the tree
    // speaks the vocabulary the grid's folder card speaks (`SeamVocab::FolderRow`
    // writes the row's class names and the `shelf-row-` element id) and takes the
    // folder's policy with the DISCLOSURE for its open — a tap here unfolds the
    // branch, where the card's tap drills into the shelf.
    let entry = EntryDescriptor {
        id: id.clone(),
        vocab: SeamVocab::FolderRow,
        base_class: "lib-row lib-row-shelf",
        // A folder's lift is a nesting, which writes a parent rather than a
        // membership: there is no list to lift it off.
        policy: folder_policy(&id, name, toggle, None),
    };

    let nav_id = id.clone();
    // Parked in a `StoredValue` rather than captured: the member rows are built
    // inside the unfold's `Show`, whose children closure has to stay an `Fn` —
    // a `String` owned by the rows' `move` closure would be moved out of it on
    // the first build, and a Copy handle to a scoped cell is the same fix
    // `crate::features::library::breadcrumb` uses for its folded chain.
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
                    // Space is the key a disclosure owns — prevented, so the
                    // page does not scroll on the row that meant to open. Enter
                    // and Shift+Enter are the shared wiring's: open (unfold)
                    // and the keyboard's hold.
                    if ev.key() == " " {
                        ev.prevent_default();
                        toggle.run(());
                        return true;
                    }
                    false
                })
            >
                {move || {
                    // Closed points at what the row would open; open points down
                    // at what it is showing. The chevron is the disclosure's
                    // whole picture.
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
                <Show when=move || !ctx.dense>
                    <span class="shrink-0 text-xs text-muted">
                        {move || summary((members.with(|m| m.len()), kids.get().len()))}
                    </span>
                </Show>
                // Open drills the breadcrumb route; the row itself only unfolds.
                // Two gestures on one shelf because they are two questions:
                // "show me inside it" and "take me to it".
                <button
                    class="lib-row-action"
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
                    // Erased through `AnyView`, the way the grid's recursive
                    // folder plate is: a recursive component whose children
                    // named its own opaque return type would be a type that
                    // never resolves.
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

/// The books on a shelf's member list, in the order the page shows books: the
/// shelf's own order is the base and the view's sort rides over it — the same
/// `library_core::sort::ordered` the page's own level runs in
/// `crate::features::library::content::level_rows`, so an unfolded row and the
/// page it mirrors cannot disagree about what comes first.
/// The two kinds of row, as one erased view: a `For` needs one type and a link
/// is not a book, so it cannot borrow a book's row.
fn row_view(
    state: AppState,
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
        Row::Link { id, name, target, .. } => {
            let to_shelf = library_core::id::is_shelf(&target);
            view! { <LinkRow state=state id=id name=name to_shelf=to_shelf depth=depth parent=parent /> }
                .into_any()
        }
    }
}

fn member_books(state: AppState, members: Signal<Vec<String>>) -> Signal<Vec<Row>> {
    Signal::derive(move || {
        let ids = members.get();
        let view = state.library.view.get();
        state
            .library
            .books
            .with(|books| sort::ordered(books, &ids, view.sort, view.sort_asc))
    })
}

/// The list's last row: the same two sources the grid's add card offers, in the
/// shape of a row rather than the shape of a cover.
#[component]
fn ListRow(
    state: AppState,
    book: Book,
    crop: Signal<bool>,
    depth: usize,
    /// The shelf whose member list renders this row: the tree's own id for a
    /// row inside an expanded branch, `None` for the flat section — which is
    /// the open level renders, and which the session resolves at the drop rather
    /// than at the mount, so a flat row can never carry a stale container
    /// through a drill. It is the fact a grid card never had to state, because
    /// a card is only ever drawn by the level the page is on.
    parent: Option<String>,
) -> impl IntoView {
    let ctx = use_context::<TreeCtx>().expect("the list provides the tree context");
    // The dense variant's whole difference, decided once at the mount: at
    // sidebar width the format IS the cover — the kind of thing the row is, in
    // the art's own footprint — and the one line of prose under the title is a
    // line the sidebar does not have.
    let dense = ctx.dense;

    // The row keeps its own ✕, and the sheet is ASKED for rather than
    // expected: the sidebar mounts this same row with no page hosts under it,
    // and a row with no sheet has no ✕ to draw. The drag session and the menu
    // are the shell's question, not this row's (see
    // `crate::features::library::shelf_item`).
    let remove_sheet = use_context::<RemoveSheet>();


    // The prop supplies the identity; everything that can move — a startup
    // measurement marking the book missing, a relink moving the address the
    // cover keys on, a rename, a fold merging a twin into it — is read back
    // by id, because a keyed row is not re-created when its content changes.
    let id = book.id.clone();
    let facts = book_facts(state, &id);
    // The list has room for the format on every row, and at this density a reader
    // is scanning names rather than looking at art — so the kind of thing a row is
    // earns its place here in a way a chip on a cover would not. A row's format
    // is the one fact nothing rewrites under it, so it stays the prop's.
    let chip = (book.format != Format::Pdf).then(|| book.format.label().to_string());
    let ext = book.format.label();

    // The id the cover's check mark reads through: the mark asks the library
    // for the membership itself, the way the shell's own selected class does.
    let check_id = id.clone();

    // The shelf's one press contract, the same one the grid's cards wear — a
    // row and a card answer to a hold, a tap and a movement alike at two
    // densities because they are ONE wiring (see
    // `crate::features::library::gestures`). A movement is always a drag here,
    // including from inside a selection: a set that could not be lifted was a
    // set the bar's "Add to shelf" was the only way to move.
    // Opening names the ROW, not its address: the library can hold two rows of
    // one file, and the address cannot say which of them the reader clicked.
    // The tree's own fact: a nested row answers to its branch, a flat row to
    // the level the page is on.
    let entry = EntryDescriptor {
        id: id.clone(),
        vocab: SeamVocab::ListRow,
        base_class: "lib-row",
        policy: book_policy(state, &id, facts, parent.clone()),
    };

    // The row's one own class beyond the reveal's light, which the shell
    // paints: the grey a book wears whose address stopped resolving.
    let missing_class = Signal::derive(move || {
        facts.with(|f| f.as_ref().is_some_and(|x| x.missing))
    });
    let remove_id = id;
    let indent = row_indent(depth);

    view! {
        <EntryShell
            state=state
            entry=entry
            style=indent
            extra_classes=vec![("row-missing".to_string(), missing_class)]
        >
            {if dense {
                // The dense variant's cover: the extension chip, in the art's own
                // footprint, so a sidebar row still leads with what the file IS.
                // No selection check rides it — the dense tree is a browser, not
                // a picker, and the row's own tint is the whole of its state.
                view! { <span class="lib-row-ext">{ext}</span> }.into_any()
            } else {
                view! {
                    <span
                        class="lib-row-cover"
                        class=("book-cover-crop", move || crop.get())
                    >
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
                }
                    .into_any()
            }}
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
                {if dense {
                    None
                } else {
                    Some(
                        view! {
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
                        },
                    )
                }}
            </span>
            {if dense {
                // The chip's whole job is done by the head-of-row extension.
                None
            } else {
                chip.map(|label| view! { <span class="lib-row-format">{label}</span> })
            }}
            {if dense {
                None
            } else {
                Some(move || {
                    facts.get().and_then(|f| f.percent()).map(|p| {
                        view! {
                            <span class="shrink-0 text-xs tabular-nums text-muted">{p}</span>
                        }
                    })
                })
            }}
            {move || {
                remove_sheet.map(|sheet| {
                    let at = remove_id.clone();
                    view! {
                        <button
                            class="lib-row-action"
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
