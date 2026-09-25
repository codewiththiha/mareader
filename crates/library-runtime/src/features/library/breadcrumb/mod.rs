//! The breadcrumb: the library page's only prose, and it is navigation.
//!
//! One crumb per level, `Home` first and the open shelf last. Every crumb is
//! a button — a folder three levels down is three clicks from the root only
//! if the reader can see all three.

//! ## Crumbs are drop targets
//!
//! Every crumb — elided ones included — is a drop target, so a book can be
//! filed onto a level the reader is not standing on. The ellipsis is not: it
//! stands for several levels and the reader cannot see which one they would
//! be choosing.

//! ## The shelf's own menu
//!
//! The last crumb is a button for a second reason: it is where a shelf the
//! reader made gets renamed (inline) or taken apart.

mod fold;
mod panel;

use leptos::html;
use leptos::prelude::*;

use app_chrome::hooks::use_resize_observer::observe_elements;
use app_chrome::icon::{Icon, IconName};
use library_core::shelf::{ALL_SHELF, Shelf, ancestors};

use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::services::{ask_shelf_apart, duplicate_shelf, rename_shelf};
use app_ui::components::primitives::floating::menu_popover::MenuPopover;
use app_ui::components::primitives::form::text_input::TextInput;
use app_ui::components::primitives::menu::menu_item::{MenuItem, MenuItemTone};

use fold::choose_split;
use panel::{EllipsisCrumb, HoverIntent};

const ALL_CRUMB_DOM_ID: &str = "crumb-all";

/// `Clone` because the chain crosses a signal, and a `Signal` hands out
/// copies rather than references.
#[derive(Clone)]
struct Crumb {
    id: String,
    name: String,
    /// The one consequence of a removal worth a sentence under the menu row:
    /// the reader cannot see it coming — the shelf comes back when the folder
    /// places again.
    watched: bool,
}

fn current_shelf_id(state: crate::context::LibraryContext) -> Option<String> {
    let id = state.library.shelf.get_untracked();
    (id != ALL_SHELF).then_some(id)
}

/// Read when an action runs rather than when the crumb is built, so renaming
/// the same shelf twice starts from its current name.
fn shelf_name_now(state: crate::context::LibraryContext, shelf_id: &str) -> String {
    state.library.shelf_name(shelf_id)
}

fn crumbs(state: crate::context::LibraryContext) -> Signal<Vec<Crumb>> {
    Signal::derive(move || {
        let id = state.library.shelf.get();
        if id == ALL_SHELF {
            return Vec::new();
        }
        state.library.shelves.with(|shelves| {
            let Some(current) = library_core::shelf::find(shelves, &id) else {
                return Vec::new();
            };
            let of = |shelf: &Shelf| {
                let watched = state.library.shelf_tracked(&shelf.id);
                Crumb {
                    id: shelf.id.clone(),
                    name: shelf.name.clone(),
                    watched,
                }
            };
            let mut chain: Vec<Crumb> = ancestors(shelves, &id).into_iter().map(of).collect();
            chain.push(of(current));
            chain
        })
    })
}

fn crumb_dom_id(shelf_id: &str) -> String {
    if shelf_id.is_empty() {
        ALL_CRUMB_DOM_ID.to_string()
    } else {
        format!("crumb-{shelf_id}")
    }
}

/// One call rather than a `NodeRef` and a rect reader: a target that
/// outlived its crumb would be a way to file onto a level no longer shown.
fn register_crumb(ctrl: &DragController, shelf_id: &str) -> String {
    let dom_id = crumb_dom_id(shelf_id);
    ctrl.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Shelf, shelf_id.to_string()),
        dom_id: dom_id.clone(),
        shelf: None,
    });
    dom_id
}

/// A computed string rather than a conditional class: the hot state is the
/// third of three things deciding the look.
fn crumb_class(current: bool, hot: bool) -> String {
    let base = "flex min-w-0 max-w-40 items-center gap-1 rounded-md px-1.5 py-0.5 \
                transition-colors focus:outline-none focus-visible:ring-2 \
                focus-visible:ring-accent";
    let tone = if current {
        "font-medium text-ink"
    } else {
        "text-muted hover:bg-line hover:text-ink"
    };
    if hot {
        format!("{base} {tone} crumb-drop")
    } else {
        format!("{base} {tone}")
    }
}

#[component]
pub(crate) fn Breadcrumb(state: crate::context::LibraryContext) -> impl IntoView {
    let ctrl = use_context::<DragController>().expect("the library page installs the drag session");
    let chain = crumbs(state);
    let intent = HoverIntent::new();
    let live = ctrl.live();

    Effect::new(move |_| {
        let _ = chain.get();
        intent.close();
    });
    Effect::new(move |_| {
        if !live.get() {
            intent.close();
        }
    });

    // Both move without a window resize: a crumb renamed, a level drilled
    // into, the trailing cluster growing, the flex squeeze settling after the
    // fold's answer.
    let nav_ref: NodeRef<html::Nav> = NodeRef::new();
    let probe_ref: NodeRef<html::Span> = NodeRef::new();
    let widths: RwSignal<Vec<f64>> = RwSignal::new(Vec::new());
    let avail: RwSignal<f64> = RwSignal::new(0.0);

    Effect::new(move |_| {
        let _ = chain.get();
        request_animation_frame(move || {
            let Some(probe) = probe_ref.get() else {
                return;
            };
            let ws = measure_children_widths(&probe);
            if widths.get_untracked() != ws {
                widths.set(ws);
            }
        });
    });

    Effect::new(move |_| {
        let Some(nav) = nav_ref.get() else {
            return;
        };
        let Some(parent) = nav.parent_element() else {
            return;
        };
        let observed = parent.clone();
        let read = move || {
            let wide = parent.client_width() as f64;
            if (avail.get_untracked() - wide).abs() > 0.5 {
                avail.set(wide);
            }
        };
        read();
        observe_elements(vec![observed], move |_| read());
    });

    let split_sig = Signal::derive(move || {
        let len = chain.get().len();
        choose_split(&widths.get(), avail.get(), len)
    });

    view! {
        <nav
            node_ref=nav_ref
            class="flex min-w-0 items-center gap-0.5 text-sm"
            aria-label="Library location"
        >
            // Plain spans with no ids: a ruler is not a crumb, and a second
            // element carrying a crumb's id would be a second answer for the
            // hit-test and the reveal's scroll.
            <span node_ref=probe_ref class="lib-crumb-probe" aria-hidden="true">
                <span class="lib-crumb-probe-item">
                    <Icon name=IconName::More size=14 />
                </span>
                {move || {
                    let levels = chain.get();
                    let last = levels.len().saturating_sub(1);
                    levels
                        .into_iter()
                        .enumerate()
                        .map(|(at, crumb)| {
                            let trails = at != last;
                            view! {
                                <span class="lib-crumb-probe-item">
                                    <span class="truncate">{crumb.name}</span>
                                    {trails.then(|| {
                                        view! { <Icon name=IconName::Next size=13 /> }
                                    })}
                                </span>
                            }
                        })
                        .collect_view()
                }}
            </span>
            <AllCrumb state=state ctrl=ctrl />
            {move || {
                let levels = chain.get();
                let len = levels.len();
                let split = split_sig.get();
                let (elided, shown) = levels.split_at(split);
                let last = len.saturating_sub(1);
                // Left to right stays root to leaf, in the bar and panel
                // alike.
                let gap = (!elided.is_empty()).then(|| {
                    view! {
                        <EllipsisCrumb
                            state=state
                            ctrl=ctrl
                            elided=elided.to_vec()
                            intent=intent
                        />
                    }
                        .into_any()
                });
                let crumbs = shown.iter().enumerate().map(|(at, crumb)| {
                    let crumb = crumb.clone();
                    if split + at == last {
                        view! { <ShelfCrumbMenu state=state ctrl=ctrl crumb=crumb /> }.into_any()
                    } else {
                        view! { <LevelCrumb state=state ctrl=ctrl crumb=crumb /> }.into_any()
                    }
                });
                gap.into_iter().chain(crumbs).collect_view()
            }}

        </nav>
    }
}

/// Always a button — the way back — and a target with an empty id, the
/// library's spelling of "no shelf", which makes a drop here take a book off
/// the shelf it was dragged out of.
#[component]
fn AllCrumb(state: crate::context::LibraryContext, ctrl: DragController) -> impl IntoView {
    let dom_id = register_crumb(&ctrl, "");
    let at_root = Signal::derive(move || state.library.shelf.get() == ALL_SHELF);

    view! {
        <button
            id=dom_id
            type="button"
            title="The top of your library"
            on:click=move |_| state.library.shelf.set(ALL_SHELF.to_string())
            class=move || {
                let base = "shrink-0 rounded-md px-1.5 py-0.5 transition-colors \
                            focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
                let tone = if at_root.get() {
                    "font-medium text-ink"
                } else {
                    "text-muted hover:bg-line hover:text-ink"
                };
                if ctrl.over_shelf("") {
                    format!("{base} {tone} crumb-drop")
                } else {
                    format!("{base} {tone}")
                }
            }
        >
            "Home"
        </button>
    }
}

/// Its own component so the last crumb's menu state stays out of it: a link
/// with a rename field inside is two controls fighting over one click.
#[component]
fn LevelCrumb(
    state: crate::context::LibraryContext,
    ctrl: DragController,
    crumb: Crumb,
) -> impl IntoView {
    let id = crumb.id.clone();
    let label = crumb.name.clone();
    let tooltip = crumb.name.clone();
    let aria = format!("Go back to {}", crumb.name);
    let dom_id = register_crumb(&ctrl, &id);
    let hot_id = id.clone();

    view! {
        <span class="flex min-w-0 items-center gap-0.5">
            <Icon name=IconName::Next size=13 class="shrink-0 text-muted" />
            <button
                id=dom_id
                type="button"
                title=tooltip
                aria-label=aria
                on:click=move |_| state.library.shelf.set(id.clone())
                class=move || crumb_class(false, ctrl.over_shelf(&hot_id))
            >
                <span class="truncate">{label}</span>
            </button>
        </span>
    }
}

/// Still a drop target: releasing a held book here files it onto the level
/// the reader is already looking at.
#[component]
fn ShelfCrumbMenu(
    state: crate::context::LibraryContext,
    ctrl: DragController,
    crumb: Crumb,
) -> impl IntoView {
    let menu_open = RwSignal::new(false);
    let renaming = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let anchor: NodeRef<html::Div> = NodeRef::new();
    let name = crumb.name.clone();
    let tooltip = name.clone();
    let aria = format!("{name} shelf options");
    let id = crumb.id.clone();
    let dom_id = register_crumb(&ctrl, &id);
    let hot_id = id;
    let watched = crumb.watched;

    view! {
        <div class="flex min-w-0 items-center gap-0.5">
            <Icon name=IconName::Next size=13 class="shrink-0 text-muted" />
            {move || {
                if renaming.get() {
                    return view! { <RenameField state=state draft=draft renaming=renaming /> }
                        .into_any();
                }
                let title = tooltip.clone();
                let aria_label = aria.clone();
                let shown = name.clone();
                let button_id = dom_id.clone();
                let class_id = hot_id.clone();
                view! {
                    <div node_ref=anchor class="relative flex min-w-0 shrink items-center">
                        <button
                            id=button_id
                            type="button"
                            title=title
                            aria-label=aria_label
                            on:click=move |_| menu_open.set(!menu_open.get_untracked())
                            class="flex min-w-0 max-w-40 items-center gap-1 rounded-md px-1.5 py-0.5 \
                                   font-medium text-ink transition-colors hover:bg-line \
                                   focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                            class=("crumb-drop", move || ctrl.over_shelf(&class_id))
                        >
                            <span class="truncate">{shown}</span>
                            <Icon name=IconName::ChevronDown size=11 class="shrink-0 text-muted" />
                        </button>
                        <MenuPopover
                            open=menu_open
                            anchor=anchor
                            width=224u32
                            coordinate_space="toolbar-row"
                            class="p-1".to_string()
                        >
                            <MenuItem
                                icon=IconName::Type
                                label="Rename…"
                                on_click=move || {
                                    menu_open.set(false);
                                    if let Some(id) = current_shelf_id(state) {
                                        draft.set(shelf_name_now(state, &id));
                                    }
                                    renaming.set(true);
                                }
                            />
                            <MenuItem
                                icon=IconName::Copy
                                label="Duplicate"
                                title="A second shelf of your own, holding the same books".to_string()
                                on_click=move || {
                                    menu_open.set(false);
                                    if let Some(id) = current_shelf_id(state) {
                                        duplicate_shelf(state, &id);
                                    }
                                }
                            />
                            <MenuItem
                                icon=IconName::Close
                                label="Remove shelf"
                                tone=MenuItemTone::Danger
                                on_click=move || {
                                    menu_open.set(false);
                                    if let Some(id) = current_shelf_id(state) {
                                        ask_shelf_apart(state, &id);
                                    }
                                }
                            />
                            {watched.then(|| {
                                view! {
                                    <p class="px-2 py-1.5 text-[11px] text-muted">
                                        "Cut from a watched folder: removing takes it off the list, and it
                                         returns if the folder places a book here again."
                                    </p>
                                }
                            })}
                        </MenuPopover>
                    </div>
                }
                    .into_any()
            }}
        </div>
    }
}

#[component]
fn RenameField(
    state: crate::context::LibraryContext,
    draft: RwSignal<String>,
    renaming: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <span class="w-40 shrink-0">
            <TextInput
                value=draft
                on_input=Callback::new(move |text| draft.set(text))
                aria_label="Shelf name".to_string()
                autofocus=true
                class="w-full rounded border border-accent bg-paper px-1.5 py-0.5 text-sm text-ink focus:outline-none"
                    .to_string()
                on_keydown=Callback::new(move |ev: leptos::ev::KeyboardEvent| {
                    match ev.key().as_str() {
                        "Enter" => {
                            ev.prevent_default();
                            if let Some(id) = current_shelf_id(state) {
                                rename_shelf(state, &id, &draft.get_untracked());
                            }
                            renaming.set(false);
                        }
                        "Escape" => {
                            ev.prevent_default();
                            renaming.set(false);
                        }
                        _ => {}
                    }
                })
            />
        </span>
    }
}

/// The one child-width measurement loop, shared by the bar's probe and the
/// panel's ruler: two engines measuring the same shape with their own loops
/// is how they drift, and the panel packs rows from the same numbers the bar
/// splits by.
pub(crate) fn measure_children_widths(node: &web_sys::Element) -> Vec<f64> {
    let kids = node.children();
    let mut widths = Vec::with_capacity(kids.length() as usize);
    for index in 0..kids.length() {
        if let Some(kid) = kids.item(index) {
            widths.push(kid.get_bounding_client_rect().width());
        }
    }
    widths
}
