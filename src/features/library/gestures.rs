//! The wiring every shelf item shares: one press decided once, and the four answers around it.
//!
//! A book card in the grid, a book row in the list and a folder card are three surfaces and ONE
//! gesture contract — a tap opens (or toggles inside a selection), a hold starts that selection
//! with this item in it, a movement hands the press to the drag session, and Enter and
//! Shift+Enter are the keyboard's two halves of the same.

use std::rc::Rc;

use leptos::prelude::*;

use crate::components::primitives::interactions::draggable_item::{
    DRAG_THRESHOLD_PX, DraggableItemOptions, use_draggable_item,
};
use crate::components::primitives::interactions::long_press::SELECT_PRESS_MS;
use crate::features::library::context_menu::{LibraryMenuHost, MenuTarget};
use crate::features::library::dnd::controller::DragController;
use crate::features::library::facts::BookFacts;
use crate::features::library::selection::{enter_selection, payload_for, toggle_selected};
use crate::services::document;
use crate::state::AppState;

#[derive(Clone)]
pub(crate) struct ShelfItemPolicy {
    pub id: String,
    /// So the label reads "Open Dune" and "Open the Sci-fi shelf" in the two voices the shelves already have.
    pub label: Signal<String>,
    /// The knob stays the caller's, because "may this be lifted" is a policy and the wiring is not the place one is decided.
    pub draggable: Signal<bool>,
    pub open: Callback<()>,
    /// Read at the ask rather than at the mount: a rescan can move the facts a menu carries between the two.
    pub menu_target: Callback<(), MenuTarget>,
    pub container: Option<String>,
}

pub(crate) struct ShelfItem {
    pub pressing: RwSignal<bool>,
    pub is_selected: Signal<bool>,
    pub on_pointerdown: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointermove: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointerup: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_pointercancel: Rc<dyn Fn(&leptos::ev::PointerEvent)>,
    pub on_click: Rc<dyn Fn(&leptos::ev::MouseEvent)>,
    pub on_contextmenu: Rc<dyn Fn(&leptos::ev::MouseEvent)>,
    /// Enter opens (or toggles, inside a selection); Shift+Enter is the
    /// keyboard's hold.
    pub on_keydown: Rc<dyn Fn(&leptos::ev::KeyboardEvent)>,
    /// "Select Dune" / "Open the Sci-fi shelf" — the item's two voices.
    pub aria_label: Signal<String>,
    /// The pressed state of a toggle, only while the shelf is selecting. A
    /// signal rather than a closure over one: a reactive attribute wants a
    /// value that is `Send` between renders, and the two facts it reads are
    /// signals already.
    pub aria_pressed: Signal<Option<&'static str>>,
}

/// Wire one shelf item. Called from the surface's component body, inside its
/// reactive owner: the wrapper, the derived signals and the session callbacks
/// all belong to the item the surface is drawing, and they die with it.
///
/// `drag` and `menu` are the page's hosts, and a mount that has neither — the
/// sidebar's tree — gets a row that still answers a tap and stands everything
/// else down: nothing to lift into, nothing to ask, no hold to start a
/// selection no bar could act on.
/// The policy the two BOOK surfaces share: the grid's card and the list's row
/// label themselves from the same facts, open the same way, and right-click to
/// the same question — they differ only in where the row is filed
/// (`container`). One spelling, beside the contract they both hand to
/// [`use_shelf_item`], so the three shapes a shelf item's policy comes in —
/// this, [`folder_policy`] and [`link_policy`] — are written once each, in the
/// file that owns the wiring they feed.
pub(crate) fn book_policy(
    state: AppState,
    id: &str,
    facts: Signal<Option<BookFacts>>,
    container: Option<String>,
) -> ShelfItemPolicy {
    let open_id = id.to_string();
    let context_id = id.to_string();
    ShelfItemPolicy {
        id: id.to_string(),
        label: Signal::derive(move || {
            facts.with(|f| f.as_ref().map(|x| x.title.clone()).unwrap_or_default())
        }),
        draggable: Signal::derive(|| true),
        open: Callback::new(move |_| document::open_row(state, open_id.clone())),
        // The missing flag is read when the menu is ASKED rather than carried
        // from the mount: the row it describes is exactly the one a background
        // measurement can change between the two.
        menu_target: Callback::new(move |_| MenuTarget::Book {
            id: context_id.clone(),
            missing: facts.with_untracked(|f| f.as_ref().is_some_and(|x| x.missing)),
        }),
        container,
    }
}

/// The policy the two FOLDER surfaces share: the grid's card and the tree's
/// shelf row both speak of themselves as "the Sci-fi shelf" in an aria answer
/// and both right-click to the same question about a folder.
///
/// What is NOT shared is what a tap does, and that is why `open` is the
/// caller's: a card in the grid drills the route (the reader asked for that
/// shelf), while a row in the tree unfolds the branch it is standing on (the
/// reader asked to see what is inside it). A container is the tree's fact too —
/// a nested row knows the shelf whose member list drew it, a card does not —
/// and a folder's lift is a nesting rather than a membership, so the grid's
/// card passes `None` for the same reason the services refuse one.
pub(crate) fn folder_policy(
    id: &str,
    name: Signal<String>,
    open: Callback<()>,
    container: Option<String>,
) -> ShelfItemPolicy {
    let menu_id = id.to_string();
    ShelfItemPolicy {
        id: id.to_string(),
        label: Signal::derive(move || format!("the {} shelf", name.get())),
        draggable: Signal::derive(|| true),
        open,
        menu_target: Callback::new(move |_| MenuTarget::Folder {
            id: menu_id.clone(),
        }),
        container,
    }
}

/// The policy both LINK surfaces share: the grid's card and the list's row say
/// the same thing about themselves, open the same row, and right-click to the
/// same question — a link is a row like any other to the menu, Open goes to the
/// book, Select and Remove mean what they always mean, and "Find again" is not
/// offered because a pointer has no address to die. One spelling rather than
/// one per density.
pub(crate) fn link_policy(
    state: AppState,
    id: &str,
    name: Signal<String>,
    container: Option<String>,
) -> ShelfItemPolicy {
    let open_id = id.to_string();
    let menu_id = id.to_string();
    ShelfItemPolicy {
        id: id.to_string(),
        // Reactive like its book and folder cousins: a link renamed while its card stands
        // keeps an aria-answer that names it, rather than the name it was mounted with.
        label: name,
        draggable: Signal::derive(|| true),
        open: Callback::new(move |_| document::open_row(state, open_id.clone())),
        menu_target: Callback::new(move |_| MenuTarget::Book {
            id: menu_id.clone(),
            missing: false,
        }),
        container,
    }
}

pub(crate) fn use_shelf_item(
    state: AppState,
    drag: Option<DragController>,
    menu: Option<LibraryMenuHost>,
    policy: ShelfItemPolicy,
) -> ShelfItem {
    let selecting = state.library.selecting;
    let selected_set = state.library.selected;
    let id = policy.id;
    // Both or neither: the page provides the pair, and a half-hosted shelf —
    // a lift with no menu to act on, a menu with no session to file through —
    // is a gesture the reader could start and not finish.
    let hosted = drag.is_some() && menu.is_some();

    let selected_id = id.clone();
    let is_selected = Signal::derive(move || selected_set.with(|s| s.contains(&selected_id)));

    // One wrapper, three gestures, and the mode is decided once per press: a
    // movement is a drag, a hold is a selection, and a release that was neither
    // is the open. The session's listeners live on the window rather than on
    // this element, so an item that unmounts mid-drag — a focus rescan filing
    // it elsewhere while the reader is holding it — leaves a drag that can
    // still end.
    let press_id = id.clone();
    let tap_id = id.clone();
    let lift_id = id.clone();
    let container = policy.container;
    let open_tap = policy.open;
    let item = use_draggable_item(DraggableItemOptions {
        press_ms: SELECT_PRESS_MS,
        drag_threshold_px: DRAG_THRESHOLD_PX,
        // A shelf with no session has nothing to lift into: the movement is a
        // scroll there, and the wrapper's own touch rule already says a finger
        // never drags.
        draggable: if hosted {
            policy.draggable
        } else {
            Signal::derive(|| false)
        },
        // A hold inside a selection would be a second way to do the thing a tap
        // now does — and a hold with no hosts starts a selection nothing can
        // act on, so it does not start one at all.
        selectable: Signal::derive(move || hosted && !selecting.get()),
        on_tap: Callback::new(move |_| {
            if selecting.get_untracked() {
                toggle_selected(state, &tap_id);
                return;
            }
            open_tap.run(());
        }),
        on_long_press: Callback::new(move |_| enter_selection(state, &press_id)),
        on_drag_start: Callback::new(move |(x, y)| {
            // What the press picks up: the whole set when this item is already
            // in it, and this item alone when it is not — with the container
            // the press was rendered by, which is where a move lifts FROM.
            if let Some(drag) = drag {
                drag.begin(payload_for(state, &lift_id, container.clone()), x, y);
            }
        }),
        // The session owns the move.
        on_drag_move: Callback::new(move |_| {}),
        // Both halves end the session and the first one there wins: this
        // release bubbles ahead of the window's own.
        on_drag_end: Callback::new(move |(x, y)| {
            if let Some(drag) = drag {
                drag.release(x, y);
            }
        }),
        on_drag_cancel: Callback::new(move |_| {
            if let Some(drag) = drag {
                drag.cancel();
            }
        }),
    });

    let swallow_click = Rc::clone(&item.swallow_click);
    let on_click: Rc<dyn Fn(&leptos::ev::MouseEvent)> = Rc::new(move |ev| {
        // The hold's exhaust and nothing else: the wrapper already decided
        // what this press meant, and the click that follows a completed hold
        // is not an intention to open the item.
        if (swallow_click)() {
            ev.stop_propagation();
        }
    });

    let swallow_context = Rc::clone(&item.swallow_context);
    let context_id = id.clone();
    let make_target = policy.menu_target;
    let on_contextmenu: Rc<dyn Fn(&leptos::ev::MouseEvent)> = Rc::new(move |ev| {
        // Stopped before the swallow is even asked: the item owns this event
        // whether or not it acts on it, and a completed hold's synthetic
        // contextmenu that went on to bubble would open the LEVEL's menu under
        // the finger that was busy selecting.
        ev.prevent_default();
        ev.stop_propagation();
        if (swallow_context)() {
            return;
        }
        // The row owns the event either way — no host to ask is no licence to
        // bubble to the level — but with no menu host there is nothing to draw.
        let Some(menu) = menu else {
            return;
        };
        let (x, y) = (ev.client_x() as f64, ev.client_y() as f64);
        // Inside a selection, the same button on an item ALREADY in the set
        // asks about the whole set, which is what a right-click on one of
        // several things means everywhere else; on an item outside the set it
        // asks about that item, because selecting it first would be a choice
        // the reader did not make.
        let in_set =
            selecting.get_untracked() && selected_set.with_untracked(|s| s.contains(&context_id));
        if in_set {
            menu.ask(x, y, MenuTarget::Selection);
            return;
        }
        menu.ask(x, y, make_target.run(()));
    });

    let key_id = id.clone();
    let select_key_id = id.clone();
    let open_key = policy.open;
    let on_keydown: Rc<dyn Fn(&leptos::ev::KeyboardEvent)> = Rc::new(move |ev| {
        if ev.key() != "Enter" {
            return;
        }
        // A keyboard has no hold to make, so it gets the gesture's two halves
        // as two keys: Shift+Enter enters selection the way a hold does, and
        // once inside, Enter toggles instead of opening.
        if ev.shift_key() && !selecting.get_untracked() {
            ev.prevent_default();
            enter_selection(state, &select_key_id);
            return;
        }
        if selecting.get_untracked() {
            toggle_selected(state, &key_id);
            return;
        }
        open_key.run(());
    });

    let label = policy.label;
    let aria_label = Signal::derive(move || {
        if selecting.get() {
            format!("Select {}", label.get())
        } else {
            format!("Open {}", label.get())
        }
    });

    let aria_id = id;
    let aria_pressed: Signal<Option<&'static str>> = Signal::derive(move || {
        selecting.get().then(|| {
            if selected_set.with(|s| s.contains(&aria_id)) {
                "true"
            } else {
                "false"
            }
        })
    });

    ShelfItem {
        pressing: item.pressing,
        is_selected,
        on_pointerdown: item.on_pointerdown,
        on_pointermove: item.on_pointermove,
        on_pointerup: item.on_pointerup,
        on_pointercancel: item.on_pointercancel,
        on_click,
        on_contextmenu,
        on_keydown,
        aria_label,
        aria_pressed,
    }
}
