//! Reusable window-aware anchored menu container, built on the shared
//! floating internals:
//!
//! * placement + viewport clamping + upward flip + transform origin come from
//!   [`position`](app_chrome::floating::position) (pure math in `ui_geom::floating`);
//! * Escape / outside-press dismissal comes from [`dismiss`](app_chrome::floating::dismiss);
//! * the open/close transition is reported through `on_open_change` rather
//!   than the popover reaching into app chrome itself (the app-shell
//!   `MenuPopover` in `crate::components::shell::titlebar::toolbar_popover` owns
//!   the titlebar-hold behaviour).
//!
//! Width is a prop so each menu can size itself. `position: fixed` escapes
//! the sidebar's `overflow-hidden`; the optional `coordinate_space` id
//! compensates WebKit's `backdrop-filter` containing block when anchoring
//! inside the glass toolbar row.

use leptos::children::ChildrenFn;
use leptos::html;
use leptos::prelude::*;

use app_chrome::floating::dismiss::{DismissPolicy, DismissTrigger, use_dismiss};
use app_chrome::floating::position::{place_at_anchor, placement_options};
use app_chrome::floating::types::{PlacementSide, Size, node_within_any};
use app_chrome::hooks::use_window_event::use_window_event;

#[component]
pub fn Popover(
    open: RwSignal<bool>,
    /// NodeRef of the trigger wrapper the panel anchors to.
    anchor: NodeRef<html::Div>,
    /// Desired panel width in CSS px (custom per menu).
    #[prop(default = 256)]
    width: u32,
    /// Min distance from viewport edges.
    #[prop(default = 8)]
    margin: u32,
    /// Extra classes (padding, max-h, overflow…).
    #[prop(optional, into)]
    class: String,
    /// Preferred placement; `Auto` opens below and flips above when the
    /// bottom would overflow.
    #[prop(default = PlacementSide::Auto)]
    placement: PlacementSide,
    /// Id of an element whose viewport offset must be subtracted — WebKit
    /// makes `backdrop-filter` a containing block for `position: fixed`
    /// descendants, so panels anchored inside the glass toolbar row need
    /// row-relative coordinates.
    #[prop(default = None)]
    coordinate_space: Option<&'static str>,
    /// Called on every open-state transition. App-shell wrappers use this to
    /// hold/release chrome (titlebar pin, hide delays) instead of the
    /// primitive knowing about the shell.
    #[prop(default = None)]
    on_open_change: Option<Callback<bool>>,
    children: ChildrenFn,
) -> impl IntoView {
    let panel_ref: NodeRef<html::Div> = NodeRef::new();
    let style_sig = RwSignal::new(String::new());

    // Window-aware placement: right-aligned to the trigger, clamped into the
    // viewport, flipped ABOVE the trigger when there is no room below.
    let place = move || {
        // The trigger wrapper is the only anchor: a popover whose trigger is
        // not mounted has nothing to be placed against.
        let Some(a) = anchor.get() else { return };
        let Some(win) = web_sys::window() else {
            return;
        };
        let win_w = win
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(1280.0);
        let win_h = win
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(800.0);
        let panel = panel_ref
            .get()
            .map(|p| {
                let r = p.get_bounding_client_rect();
                Size::new(r.width().max(1.0), r.height().max(1.0))
            })
            .unwrap_or(Size::new(width as f64, 200.0));
        let opts = placement_options(placement, 4.0, margin as f64, Size::new(win_w, win_h));
        let placed = place_at_anchor(&a, panel.w, panel.h, &opts, coordinate_space);
        let rect = placed.rect;
        style_sig.set(format!(
            "left:{:.1}px;top:{:.1}px;width:{:.0}px;transform-origin:{}",
            rect.x, rect.y, width, placed.transform_origin
        ));
    };

    // Place once the panel mounts; re-clamp on window resize while open.
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        place();
        request_animation_frame(place);
        use_window_event("resize", move |_| place());
    });

    // Report open→closed transitions (holds, pins, analytics).
    let was_open = StoredValue::new_local(false);
    Effect::new(move |_| {
        let is_open = open.get();
        let was = was_open.get_value();
        if is_open != was {
            was_open.set_value(is_open);
            if let Some(cb) = on_open_change {
                cb.run(is_open);
            }
        }
    });
    on_cleanup(move || {
        if was_open.get_value()
            && let Some(cb) = on_open_change
        {
            cb.run(false);
        }
    });

    // Outside-click + Escape dismissal owned HERE so every menu gets it free.
    use_dismiss(
        open.into(),
        Callback::new(move |_| open.set(false)),
        DismissPolicy {
            escape: true,
            outside: Some(DismissTrigger::PointerDown),
            exclude_selectors: Vec::new(),
            enabled: None,
            topmost_only: true,
        },
        {
            move |target| {
                node_within_any(target, &[anchor, panel_ref])
            }
        },
    );

    // Static for the popover's lifetime, parked in a StoredValue — a Copy
    // handle to a plain scoped cell. `Show`'s children closure must be an
    // `Fn` and the class closure is moved into the element by value, so the
    // captured handle has to be Copy; a signal would compile too but would
    // pretend the class is reactive when only `style` actually is
    // (re-written by `place`).
    let panel_class: StoredValue<String, LocalStorage> =
        StoredValue::new_local(if class.is_empty() {
            format!(
                "menu-popover fixed {} rounded-lg border border-line bg-surface shadow-lg",
                app_chrome::layers::POPOVER
            )
        } else {
            format!(
                "menu-popover fixed {} rounded-lg border border-line bg-surface shadow-lg {class}",
                app_chrome::layers::POPOVER
            )
        });

    view! {
        <Show when=move || open.get()>
            <div
                node_ref=panel_ref
                class=move || panel_class.with_value(String::clone)
                style=move || style_sig.get()
            >
                {children()}
            </div>
        </Show>
    }
}
