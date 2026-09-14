//! Multi-select on the shelf: the gesture that starts it, the set it fills, and the bar that
//! acts on it.
//!
//! The gesture is the app's one card wrapper at the app's one hold tuning
//! (`crate::components::primitives::interactions::draggable_item`), so holding a book feels
//! exactly like holding a highlight.

use std::collections::HashSet;

use leptos::html;
use leptos::prelude::*;

use app_chrome::floating::dismiss::{DismissPolicy, DismissTrigger, use_dismiss};
use app_chrome::floating::types::PlacementSide;
use app_chrome::icon::{Icon, IconName};
use library_core::shelf::{ALL_SHELF, Shelf, can_nest};

use crate::components::primitives::controls::button::{Button, ButtonTone, ButtonVariant};
use crate::components::primitives::menu::menu_item::MenuItem;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::components::primitives::overlay::action_bar::ActionBar;
use crate::components::primitives::floating::menu_popover::MenuPopover;
use crate::features::library::content::{FolderOrder, ShelfOrder, level_rows};
use crate::features::library::dnd::controller::DragPayload;
use crate::features::library::remove_modal::RemoveSheet;
use crate::services::library::{create_shelf_here, file_many, nest_many};
use crate::state::AppState;

/// The mark is the visible half of a selection — an outline alone asks the reader to remember
/// which cards they have already tapped — and three copies of it were three places the check could
/// drift from the set it marks.
#[component]
pub(crate) fn SelectionCheck(state: AppState, id: String) -> impl IntoView {
    let selected = state.library.is_selected(&id);
    view! {
        {move || {
            state.library.selecting.get().then(|| {
                view! {
                    <span class="lib-check" aria-hidden="true">
                        {move || {
                            selected
                                .get()
                                .then(|| view! { <Icon name=IconName::Check size=11 /> })
                        }}
                    </span>
                }
            })
        }}
    }
}

pub(crate) fn enter_selection(state: AppState, item_id: &str) {
    let id = item_id.to_string();
    state.library.selecting.set(true);
    state.library.selected.update(|selected| {
        selected.insert(id);
    });
}

/// Every exit goes through here — Done, Escape, a click on empty shelf, an action that consumed the selection, leaving the page — so there is one place that decides what "not selecting" means.
pub(crate) fn exit_selection(state: AppState) {
    state.library.selecting.set(false);
    state.library.selected.set(HashSet::new());
}

// "Is this one in it" is asked by every card on every repaint, and a list would answer it by walking.
pub(crate) fn toggle_selected(state: AppState, item_id: &str) {
    state.library.selected.update(|selected| {
        if !selected.remove(item_id) {
            selected.insert(item_id.to_string());
        }
    });
}

pub(crate) fn selected_ids(state: AppState) -> Vec<String> {
    state
        .library
        .selected
        .with_untracked(|selected| selected.iter().cloned().collect())
}

/// It itemises what a removal costs, and a link costs nothing but itself, which is a line the receipt says rather than a reason to leave the row out of the set.
fn selected_books(state: AppState) -> Vec<String> {
    let folders = selected_folders(state);
    selected_ids(state)
        .into_iter()
        .filter(|id| !folders.contains(id))
        .collect()
}

fn selected_folders(state: AppState) -> Vec<String> {
    let ids = selected_ids(state);
    state.library.shelves.with_untracked(|shelves| {
        ids.into_iter()
            .filter(|id| shelves.iter().any(|s| &s.id == id))
            .collect()
    })
}

/// Holding three books and lifting one of them lifts all three, while lifting a book nobody selected lifts that book and leaves the set the reader built alone.
pub(crate) fn payload_for(
    state: AppState,
    item_id: &str,
    source: Option<String>,
) -> DragPayload {
    if state
        .library
        .selected
        .with_untracked(|selected| selected.contains(item_id))
    {
        return DragPayload {
            books: in_page_order(state, selected_books(state)),
            folders: selected_folders(state),
            source,
        };
    }
    let folder = state
        .library
        .shelves
        .with_untracked(|shelves| shelves.iter().any(|s| s.id == item_id));
    if folder {
        DragPayload {
            books: Vec::new(),
            folders: vec![item_id.to_string()],
            source,
        }
    } else {
        DragPayload {
            books: vec![item_id.to_string()],
            folders: Vec::new(),
            source,
        }
    }
}

/// The set has no order, but a drop does: three books put down before a card land in whatever order the payload names them, and an order a hash iteration chose is one the reader cannot predict.
fn in_page_order(state: AppState, ids: Vec<String>) -> Vec<String> {
    let mut ordered: Vec<String> = level_rows(state)
        .into_iter()
        .map(|row| row.id().to_string())
        .filter(|id| ids.contains(id))
        .collect();
    let rest: Vec<String> = ids
        .into_iter()
        .filter(|id| !ordered.contains(id))
        .collect();
    ordered.extend(rest);
    ordered
}

/// One action from the reader's side, two operations on the same list, and a folder that cannot be nested there (because it would end up inside itself) is left where it is rather than failing the batch.
fn file_selection(state: AppState, shelf_id: &str) {
    let books = selected_books(state);
    file_many(state, &books, shelf_id);
    let folders = selected_folders(state);
    nest_many(state, &folders, shelf_id);
}

/// Created without drilling into it: the reader picked cards on one shelf and asked for them to be on another, and navigating away is an answer to a question they did not ask.
pub(crate) fn file_selection_on_new_shelf(state: AppState) {
    let shelf_id = create_shelf_here(state);
    file_selection(state, &shelf_id);
    exit_selection(state);
}

// Both halves go to the sheet, because the sheet receipts both: an ask that handed over the books alone would take the shelves apart with no receipt at all.
pub(crate) fn ask_remove_selection(state: AppState, sheet: &RemoveSheet) {
    let books = selected_books(state);
    let folders = selected_folders(state);
    if books.is_empty() && folders.is_empty() {
        return;
    }
    exit_selection(state);
    sheet.ask_many(books, folders);
}

pub(crate) fn select_on_screen(state: AppState, order: ShelfOrder, folders: FolderOrder) {
    let on_screen: Vec<String> = order
        .0
        .get_untracked()
        .into_iter()
        .map(|row| row.id().to_string())
        .chain(folders.0.get_untracked().into_iter().map(|each| each.id))
        .collect();
    state.library.selecting.set(true);
    state.library.selected.update(|selected| {
        selected.extend(on_screen);
    });
}

pub(crate) fn use_select_mode(state: AppState) {
    use_dismiss(
        state.library.selecting.into(),
        Callback::new(move |_| exit_selection(state)),
        DismissPolicy {
            escape: true,
            outside: Some(DismissTrigger::Click),
            exclude_selectors: vec![
                ".book-card",
                ".folder-card",
                ".lib-row",
                ".lib-select-bar",
                ".menu-popover",
            ],
            enabled: None,
            topmost_only: false,
        },
        |_| false,
    );

    on_cleanup(move || exit_selection(state));
}

/// "All books" is not one of them: filing onto it would be a way of filing nowhere at all. A selection holding a folder cannot be filed inside that folder or inside anything under it.
fn shelf_choices(state: AppState) -> Signal<Vec<Shelf>> {
    Signal::derive(move || {
        let selected = state.library.selected.get();
        state.library.shelves.with(|shelves| {
            shelves
                .iter()
                .filter(|s| s.id != ALL_SHELF)
                .filter(|s| {
                    selected
                        .iter()
                        .all(|id| can_nest(shelves, id, &s.id))
                })
                .cloned()
                .collect()
        })
    })
}

#[component]
pub(crate) fn LibrarySelectBar(state: AppState) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let folders = use_context::<FolderOrder>().expect("the library content provides the folders");
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");

    let selecting = state.library.selecting;
    let count = Signal::derive(move || state.library.selected.with(|s| s.len()));
    let choices = shelf_choices(state);
    let shelf_menu = RwSignal::new(false);
    let shelf_anchor: NodeRef<html::Div> = NodeRef::new();

    view! {
        <ActionBar
            visible=Signal::derive(move || selecting.get())
            role="toolbar"
            aria_label="Library selection"
            class="lib-select-bar"
        >
            <span class="mr-1.5 text-xs font-medium tabular-nums text-muted">
                {move || format!("{} selected", count.get())}
            </span>

            <Button
                on_click=move |_| select_on_screen(state, order, folders)
                variant=ButtonVariant::Ghost
                compact=true
                class="rounded-full px-3"
            >
                "All"
            </Button>

            <div node_ref=shelf_anchor class="relative inline-flex">
                <Button
                    on_click=move |_| shelf_menu.set(!shelf_menu.get_untracked())
                    variant=ButtonVariant::Ghost
                    compact=true
                    active=Signal::derive(move || shelf_menu.get())
                    disabled=Signal::derive(move || count.get() == 0)
                    class="rounded-full px-3"
                    title="Add the selected items to a shelf"
                >
                    "Add to shelf"
                </Button>
                <MenuPopover
                    open=shelf_menu
                    anchor=shelf_anchor
                    width=240u32
                    placement=PlacementSide::Above
                    hold_titlebar=false
                    class="max-h-72 overflow-y-auto p-1".to_string()
                >
                    <SectionLabel text="Add to shelf" />
                    {move || {
                        choices.get().into_iter().map(|shelf| {
                            let label = shelf.name.clone();
                            let shelf_id = shelf.id.clone();
                            view! {
                                <MenuItem
                                    label=label
                                    on_click=move || {
                                        shelf_menu.set(false);
                                        file_selection(state, &shelf_id);
                                        exit_selection(state);
                                    }
                                />
                            }
                        }).collect_view()
                    }}
                    <Separator spacing="my-1" />
                    <MenuItem
                        icon=IconName::Plus
                        label="New shelf"
                        on_click=move || {
                            shelf_menu.set(false);
                            file_selection_on_new_shelf(state);
                        }
                    />
                </MenuPopover>
            </div>

            <Button
                on_click=move |_| ask_remove_selection(state, &remove_sheet)
                variant=ButtonVariant::Ghost
                tone=ButtonTone::Danger
                compact=true
                class="rounded-full px-3"
                disabled=Signal::derive(move || count.get() == 0)
                title="Remove the selected books and shelves"
            >
                {move || format!("Remove ({})", count.get())}
            </Button>

            <Button
                on_click=move |_| exit_selection(state)
                variant=ButtonVariant::Ghost
                compact=true
                class="rounded-full px-3"
            >
                "Done"
            </Button>
        </ActionBar>
    }
}
