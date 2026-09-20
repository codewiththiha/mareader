//! The ⋯ menu: how the shelf looks, and the one row that is not a look —
//! Reload Window, the reset for a footprint that latched (the shelf is
//! exactly where the latch is visible: after the close, the number stays).
//!
//! Four decisions — layout, column count, cover treatment and sort — every
//! one a `LibraryView` field, so no row here reaches into the DOM to arrange
//! anything itself.

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use library_core::sort::SortKey;
use library_core::view::{CoverFit, LibraryLayout, LibraryView};

use crate::components::primitives::controls::toggle_button::ToggleButton;
use crate::services::library::create_shelf_and_enter;
use crate::components::primitives::menu::menu_item::MenuItem;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::components::primitives::floating::menu_popover::MenuPopover;
use crate::state::AppState;

const SORTS: [SortKey; 5] = [
    SortKey::Manual,
    SortKey::Title,
    SortKey::Author,
    SortKey::Added,
    SortKey::LastRead,
];

fn set_view(state: AppState, change: impl FnOnce(&mut LibraryView) + 'static) {
    state.library.view.update(change);
    crate::storage::persist_library(state.library);
}

#[component]
pub(crate) fn ViewMenu(state: AppState) -> impl IntoView {
    let open = RwSignal::new(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();

    let is_list = Signal::derive(move || state.library.view.with(|v| v.is_list()));
    // The pinned count when there is one, else the count Auto's flow is
    // producing, which the grid reports on every resize (see
    // `crate::features::library::grid`).
    let columns = Signal::derive(move || {
        state.library.view.with(|v| v.columns.or(Some(v.auto_fit)))
    });
    let auto = Signal::derive(move || state.library.view.with(|v| v.columns.is_none()));
    let stepper_live = Signal::derive(move || state.library.view.with(|v| v.columns_enabled()));
    let at_min = Signal::derive(move || {
        columns.with(|c| c.is_some_and(|n| n <= library_core::view::COLUMNS_MIN))
    });
    let at_max = Signal::derive(move || {
        columns.with(|c| c.is_some_and(|n| n >= library_core::view::COLUMNS_MAX))
    });
    let fit = Signal::derive(move || state.library.view.with(|v| v.cover == CoverFit::Fit));
    let sort = Signal::derive(move || state.library.view.with(|v| v.sort));
    let ascending = Signal::derive(move || state.library.view.with(|v| v.sort_asc));
    let has_direction = Signal::derive(move || !sort.get().is_manual());

    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <IconButton
                icon=IconName::More
                title="Shelf view"
                pressed=Signal::derive(move || open.get())
                on_click=move || open.set(!open.get_untracked())
            />
            <MenuPopover
                open=open
                anchor=root_ref
                width=264u32
                coordinate_space="toolbar-row"
                class="p-2".to_string()
            >
                <MenuItem
                    icon=IconName::Plus
                    label="New shelf"
                    on_click=move || {
                        open.set(false);
                        create_shelf_and_enter(state, None);
                    }
                />
                <Separator spacing="my-1.5" />
                <MenuItem
                    label="List"
                    selected=Signal::derive(move || is_list.get())
                    check=true
                    on_click=move || {
                        set_view(state, |v| v.layout = LibraryLayout::List);
                    }
                />
                <MenuItem
                    label="Grid"
                    selected=Signal::derive(move || !is_list.get())
                    check=true
                    on_click=move || {
                        set_view(state, |v| v.layout = LibraryLayout::Grid);
                    }
                />

                <Separator spacing="my-1.5" />
                <SectionLabel text="Columns" />
                <div class="flex items-center justify-between gap-2 px-1 py-1">
                    <ToggleButton
                        active=auto
                        on_click=move || {
                            set_view(state, LibraryView::auto_columns);
                        }
                        variant_class="px-2 py-1 text-xs"
                    >
                        <span>"Auto"</span>
                    </ToggleButton>
                    <div class="flex items-center gap-0.5">
                        <IconButton
                            icon=IconName::Minus
                            size=13
                            title="Fewer columns"
                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                            disabled=Signal::derive(move || !stepper_live.get() || at_min.get())
                            on_click=move || {
                                set_view(state, |v| v.step_columns(-1));
                            }
                        />
                        <span class="w-5 text-center text-xs tabular-nums text-ink">
                            {move || {
                                columns.get().map_or_else(|| "–".to_string(), |n| n.to_string())
                            }}
                        </span>
                        <IconButton
                            icon=IconName::Plus
                            size=13
                            title="More columns"
                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                            disabled=Signal::derive(move || !stepper_live.get() || at_max.get())
                            on_click=move || {
                                set_view(state, |v| v.step_columns(1));
                            }
                        />
                    </div>
                </div>

                <Separator spacing="my-1.5" />
                <SectionLabel text="Book covers" />
                <MenuItem
                    label=CoverFit::Fit.label()
                    selected=Signal::derive(move || fit.get())
                    check=true
                    on_click=move || {
                        set_view(state, |v| v.cover = CoverFit::Fit);
                    }
                />
                <MenuItem
                    label=CoverFit::Crop.label()
                    selected=Signal::derive(move || !fit.get())
                    check=true
                    on_click=move || {
                        set_view(state, |v| v.cover = CoverFit::Crop);
                    }
                />

                <Separator spacing="my-1.5" />
                <SectionLabel text="Sort by" />
                {move || {
                    has_direction.get().then(|| {
                        view! {
                            <div class="mb-1 flex gap-1.5 px-1">
                                <ToggleButton
                                    active=Signal::derive(move || ascending.get())
                                    on_click=move || {
                                        set_view(state, |v| v.sort_asc = true);
                                    }
                                    variant_class="flex flex-1 items-center justify-center gap-1 px-2 py-1 text-xs"
                                    title="Ascending"
                                >
                                    <Icon name=IconName::ChevronUp size=12 />
                                    <span>"Ascending"</span>
                                </ToggleButton>
                                <ToggleButton
                                    active=Signal::derive(move || !ascending.get())
                                    on_click=move || {
                                        set_view(state, |v| v.sort_asc = false);
                                    }
                                    variant_class="flex flex-1 items-center justify-center gap-1 px-2 py-1 text-xs"
                                    title="Descending"
                                >
                                    <Icon name=IconName::ChevronDown size=12 />
                                    <span>"Descending"</span>
                                </ToggleButton>
                            </div>
                        }
                    })
                }}
                {SORTS
                    .iter()
                    .copied()
                    .map(|key| {
                        view! {
                            <MenuItem
                                label=key.label().to_string()
                                selected=Signal::derive(move || sort.get() == key)
                                check=true
                                on_click=move || {
                                    set_view(state, move |v| v.sort = key);
                                }
                            />
                        }
                    })
                    .collect_view()}
                <Separator spacing="my-1.5" />
                // Not a view decision — the memory reset, and the shelf is
                // where it belongs most: the footprint a reading session
                // latched onto survives the close, so the empty shelf is
                // where the number is both highest and least explicable
                // (Mareader.md, "The memory model").
                <MenuItem
                    icon=IconName::Reload
                    label="Reload Window".to_string()
                    sublabel="Restarts in place; your place is kept, the memory is not".to_string()
                    on_click=move || {
                        open.set(false);
                        crate::services::reload::reload_app(state);
                    }
                />
            </MenuPopover>
        </div>
    }
}
