//! The wiring every shelf item shares: one press decided once.

use std::rc::Rc;

use leptos::prelude::*;

use crate::features::library::context_menu::{LibraryMenuHost, MenuTarget};
use crate::features::library::dnd::controller::DragController;
use crate::features::library::facts::BookFacts;
use crate::features::library::selection::{enter_selection, payload_for, toggle_selected};
use crate::services::open;
use app_ui::components::primitives::interactions::draggable_item::{
    DRAG_THRESHOLD_PX, DraggableItemOptions, use_draggable_item,
};
use app_ui::components::primitives::interactions::long_press::SELECT_PRESS_MS;

#[derive(Clone)]
pub struct ShelfItemPolicy {
    pub id: String,
    /// So the aria label can say "Open the Sci-fi shelf".
    pub label: Signal<String>,
    pub open: Callback<()>,
    /// Read at the ask: a rescan can change the menu's facts.
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
    /// The toggle's pressed state while the shelf is selecting.
    pub aria_pressed: Signal<Option<&'static str>>,
}

/// The policy the two book surfaces share (grid card, list row).
pub(crate) fn book_policy(
    state: crate::context::LibraryContext,
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
        open: Callback::new(move |_| open::open_row(&state, open_id.clone())),
        // The missing flag is read at the ask.
        menu_target: Callback::new(move |_| MenuTarget::Book {
            id: context_id.clone(),
            missing: facts.with_untracked(|f| f.as_ref().is_some_and(|x| x.missing)),
        }),
        container,
    }
}

/// The policy the two folder surfaces share: aria name and menu.
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

/// The policy both link surfaces share.
pub(crate) fn link_policy(
    state: crate::context::LibraryContext,
    id: &str,
    name: Signal<String>,
    container: Option<String>,
) -> ShelfItemPolicy {
    let open_id = id.to_string();
    let menu_id = id.to_string();
    ShelfItemPolicy {
        id: id.to_string(),
        // Reactive like its cousins: a renamed link stays named.
        label: name,
        open: Callback::new(move |_| open::open_row(&state, open_id.clone())),
        menu_target: Callback::new(move |_| MenuTarget::Book {
            id: menu_id.clone(),
            missing: false,
        }),
        container,
    }
}

/// Wire one shelf item, owned by the surface's reactive owner.
pub(crate) fn use_shelf_item(
    state: crate::context::LibraryContext,
    drag: Option<DragController>,
    menu: Option<LibraryMenuHost>,
    policy: ShelfItemPolicy,
) -> ShelfItem {
    let selecting = state.library.selecting;
    let selected_set = state.library.selected;
    let id = policy.id;
    // Both or neither: a half-hosted shelf cannot finish a gesture.
    let hosted = drag.is_some() && menu.is_some();

    let selected_id = id.clone();
    let is_selected = Signal::derive(move || selected_set.with(|s| s.contains(&selected_id)));

    // One wrapper, three gestures, mode decided once per press.
    let press_id = id.clone();
    let tap_id = id.clone();
    let lift_id = id.clone();
    let container = policy.container;
    let open_tap = policy.open;
    let item = use_draggable_item(DraggableItemOptions {
        press_ms: SELECT_PRESS_MS,
        drag_threshold_px: DRAG_THRESHOLD_PX,
        // With no session there is nothing to lift into.
        draggable: Signal::derive(move || hosted),
        // A hold inside a selection would duplicate the tap's toggle.
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
            // The press picks up the whole set when this item is in it.
            if let Some(drag) = drag {
                drag.begin(payload_for(state, &lift_id, container.clone()), x, y);
            }
        }),
        // Both releases end the session; the first one wins.
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
        // A completed hold's click is not an intention to open.
        if (swallow_click)() {
            ev.stop_propagation();
        }
    });

    let swallow_context = Rc::clone(&item.swallow_context);
    let context_id = id.clone();
    let make_target = policy.menu_target;
    let on_contextmenu: Rc<dyn Fn(&leptos::ev::MouseEvent)> = Rc::new(move |ev| {
        // Stopped before the swallow is asked.
        ev.prevent_default();
        ev.stop_propagation();
        if (swallow_context)() {
            return;
        }
        // With no menu host there is nothing to draw.
        let Some(menu) = menu else {
            return;
        };
        let (x, y) = (ev.client_x() as f64, ev.client_y() as f64);
        // Right-click asks about the set an item is in, else that item.
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
        // Shift+Enter is the keyboard's hold.
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
