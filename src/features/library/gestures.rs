//! The wiring every shelf item shares: one press decided once.
//!
//! The grid's book card, the list's book row and the folder card are three
//! surfaces under one gesture contract: a tap opens (or toggles inside a
//! selection), a hold starts a selection with this item in it, a movement
//! hands the press to the drag session, and Enter / Shift+Enter are the
//! keyboard's two halves of the same.

use std::rc::Rc;

use leptos::prelude::*;

use ui_kit::interactions::draggable_item::{
    DRAG_THRESHOLD_PX, DraggableItemOptions, use_draggable_item,
};
use ui_kit::interactions::long_press::SELECT_PRESS_MS;
use crate::features::library::context_menu::{LibraryMenuHost, MenuTarget};
use crate::features::library::dnd::controller::DragController;
use crate::features::library::facts::BookFacts;
use crate::features::library::selection::{enter_selection, payload_for, toggle_selected};
use crate::services::document;
use crate::state::AppState;

#[derive(Clone)]
pub(crate) struct ShelfItemPolicy {
    pub id: String,
    /// So the aria label reads "Open Dune" and "Open the Sci-fi shelf" in the
    /// two voices the shelves already have.
    pub label: Signal<String>,
    pub open: Callback<()>,
    /// Read at the ask rather than at the mount: a rescan can change the facts
    /// a menu carries between the two.
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
    /// Enter opens (or toggles inside a selection); Shift+Enter is the
    /// keyboard's hold.
    pub on_keydown: Rc<dyn Fn(&leptos::ev::KeyboardEvent)>,
    /// "Select Dune" / "Open the Sci-fi shelf" — the item's two voices.
    pub aria_label: Signal<String>,
    /// The toggle's pressed state, only while the shelf is selecting. A
    /// signal rather than a closure: a reactive attribute wants a value that
    /// is `Send` between renders, and both facts it reads are signals.
    pub aria_pressed: Signal<Option<&'static str>>,
}

/// The policy the two book surfaces share: the grid's card and the list's row
/// label themselves from the same facts, open the same way and right-click to
/// the same question — they differ only in `container`. One spelling here, so
/// the three policy shapes (this, [`folder_policy`], [`link_policy`]) are each
/// written once, in the file that owns the wiring they feed.
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
        open: Callback::new(move |_| document::open_row(state, open_id.clone())),
        // The missing flag is read when the menu is asked, not carried from
        // the mount: a background measurement can change the row between the
        // two.
        menu_target: Callback::new(move |_| MenuTarget::Book {
            id: context_id.clone(),
            missing: facts.with_untracked(|f| f.as_ref().is_some_and(|x| x.missing)),
        }),
        container,
    }
}

/// The policy the two folder surfaces share: the grid's card and the tree's
/// shelf row both say "the Sci-fi shelf" in aria and right-click to the same
/// folder question.
///
/// What a tap DOES is the caller's: a grid card drills the route, a tree row
/// unfolds its branch. `container` is the tree's fact — a nested row knows
/// the shelf whose member list drew it, a card does not — and a folder's lift
/// is a nesting rather than a membership, so the card passes `None`.
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
        open,
        menu_target: Callback::new(move |_| MenuTarget::Folder {
            id: menu_id.clone(),
        }),
        container,
    }
}

/// The policy both link surfaces share. A link is a row like any other to
/// the menu — Open goes to the book, and "Find again" is not offered because
/// a pointer has no address to die.
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
        // Reactive like its cousins: a link renamed while its card stands
        // keeps an aria answer that names it.
        label: name,
        open: Callback::new(move |_| document::open_row(state, open_id.clone())),
        menu_target: Callback::new(move |_| MenuTarget::Book {
            id: menu_id.clone(),
            missing: false,
        }),
        container,
    }
}

/// Wire one shelf item. Called from the surface's component body, inside its
/// reactive owner, so the wrapper, derived signals and session callbacks die
/// with the item.
///
/// `drag` and `menu` are the page's hosts; a mount that has neither (the
/// sidebar's tree) still answers a tap and stands everything else down:
/// nothing to lift into, nothing to ask, no selection no bar could act on.
pub(crate) fn use_shelf_item(
    state: AppState,
    drag: Option<DragController>,
    menu: Option<LibraryMenuHost>,
    policy: ShelfItemPolicy,
) -> ShelfItem {
    let selecting = state.library.selecting;
    let selected_set = state.library.selected;
    let id = policy.id;
    // Both or neither: a half-hosted shelf — a lift with no menu to act on,
    // a menu with no session to file through — is a gesture the reader could
    // start and not finish.
    let hosted = drag.is_some() && menu.is_some();

    let selected_id = id.clone();
    let is_selected = Signal::derive(move || selected_set.with(|s| s.contains(&selected_id)));

    // One wrapper, three gestures, mode decided once per press: a movement
    // is a drag, a hold is a selection, a release that was neither is the
    // open. The session's listeners live on the window, so an item that
    // unmounts mid-drag (a focus rescan filing it elsewhere) leaves a drag
    // that can still end.
    let press_id = id.clone();
    let tap_id = id.clone();
    let lift_id = id.clone();
    let container = policy.container;
    let open_tap = policy.open;
    let item = use_draggable_item(DraggableItemOptions {
        press_ms: SELECT_PRESS_MS,
        drag_threshold_px: DRAG_THRESHOLD_PX,
        // With no session there is nothing to lift into: the movement is a
        // scroll, and the wrapper's touch rule already says a finger never
        // drags.
        draggable: Signal::derive(move || hosted),
        // A hold inside a selection would duplicate the tap's toggle, and a
        // hold with no hosts would start a selection nothing can act on.
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
            // The press picks up the whole set when this item is in it,
            // else this item alone — with the container it was rendered by,
            // which is where a move lifts from.
            if let Some(drag) = drag {
                drag.begin(payload_for(state, &lift_id, container.clone()), x, y);
            }
        }),
        on_drag_move: Callback::new(move |_| {}),
        // Both releases end the session; the first one there wins — this
        // handler bubbles ahead of the window's own.
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
        // The wrapper already decided what this press meant; the click that
        // follows a completed hold is not an intention to open.
        if (swallow_click)() {
            ev.stop_propagation();
        }
    });

    let swallow_context = Rc::clone(&item.swallow_context);
    let context_id = id.clone();
    let make_target = policy.menu_target;
    let on_contextmenu: Rc<dyn Fn(&leptos::ev::MouseEvent)> = Rc::new(move |ev| {
        // Stopped before the swallow is asked: a completed hold's synthetic
        // contextmenu that bubbled would open the level's menu under the
        // finger that was busy selecting.
        ev.prevent_default();
        ev.stop_propagation();
        if (swallow_context)() {
            return;
        }
        // With no menu host there is nothing to draw; the event stays
        // stopped above either way.
        let Some(menu) = menu else {
            return;
        };
        let (x, y) = (ev.client_x() as f64, ev.client_y() as f64);
        // Inside a selection, right-click on an item already in the set asks
        // about the whole set; on an item outside it, about that item —
        // selecting it first would be a choice the reader did not make.
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
        // A keyboard has no hold: Shift+Enter enters selection the way a
        // hold does, and inside a selection Enter toggles instead of opening.
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
