//! Context-menu primitive: a cursor-point menu that clamps into the viewport,
//! dismisses on Escape / outside press, and delegates the payload to the
//! caller. The domain flavor (what a right-click on a gloss mark means)
//! lives in the caller; the shell here is generic.
//!
//! Usage:
//! ```ignore
//! <ContextMenu target=menu position=|t: &ContextTarget| (t.x, t.y) on_close=move || menu.set(None)>
//!     <MenuItem icon=IconName::Close label="Remove highlight" tone=MenuItemTone::Danger
//!               on_click=move || { /* read menu.get_untracked() here */ } />
//! </ContextMenu>
//! ```

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;


use app_chrome::floating::dismiss::{DismissPolicy, DismissTrigger, use_dismiss};
use app_chrome::floating::types::{Point, place_context_menu};
use app_chrome::hooks::use_window_event::use_window_event;

/// A right-click menu for a generic target payload.
///
/// `position` derives the cursor point from the payload; the menu is clamped
/// into the viewport at that point with an 8px margin. `on_close` is fired on
/// Escape / outside press; `children` are the (static) menu items, which may
/// read `target` themselves to act on the current payload.
#[component]
pub fn ContextMenu<T: Clone + Send + Sync + 'static>(
    /// The payload the menu acts on; `None` hides the menu.
    target: RwSignal<Option<T>>,
    /// Client coordinates of the menu, derived from the payload.
    position: impl Fn(&T) -> (f64, f64) + 'static,
    on_close: Callback<()>,
    children: ChildrenFn,
    /// Minimum width before the menu measures itself.
    #[prop(default = 176)]
    min_width: u32,
    /// Extra classes (padding, shadow, domain surface name…).
    #[prop(optional, into)]
    class: Option<String>,
) -> impl IntoView {
    let panel_ref: NodeRef<html::Div> = NodeRef::new();
    let style_sig = RwSignal::new(String::new());

    let visible = Signal::derive(move || target.with(|t| t.is_some()));

    // Place (and clamp) at the target while open; re-measure after the panel
    // mounts (so the clamp uses the real size), and re-clamp on resize.
    Effect::new(move |_| {
        let Some(t) = target.get() else {
            return;
        };
        let (px, py) = position(&t);
        let place = move || {
            let size = app_chrome::floating::position::panel_size(
                panel_ref.get().map(|p| p.unchecked_into::<web_sys::Element>()),
                (min_width as f64, 48.0),
            );
            let vp = app_chrome::floating::position::viewport();
            let placed = place_context_menu(Point::new(px, py), size, vp, 8.0);
            style_sig.set(format!(
                "left:{:.1}px;top:{:.1}px;min-width:{min_width}px",
                placed.rect.x, placed.rect.y
            ));
        };
        let place = std::rc::Rc::new(place);
        place();
        {
            let place = std::rc::Rc::clone(&place);
            request_animation_frame(move || place());
        }
        {
            let place = std::rc::Rc::clone(&place);
            use_window_event("resize", move |_| place());
        }
    });

    use_dismiss(
        visible,
        on_close,
        DismissPolicy {
            escape: true,
            outside: Some(DismissTrigger::PointerDown),
            exclude_selectors: Vec::new(),
            enabled: None,
            topmost_only: true,
        },
        {
            move |target| {
                panel_ref
                    .get()
                    .map(|p| p.contains(Some(target)))
                    .unwrap_or(false)
            }
        },
    );

    let base_class = format!(
        "menu-popover context-menu fixed {} min-w-[{min_width}px] surface-popover",
        app_chrome::layers::CONTEXT_MENU
    );
    let panel_class = match class {
        Some(extra) => format!("{base_class} {extra}"),
        None => base_class,
    };
    // Static for the menu's lifetime, parked in a StoredValue — a Copy
    // handle to a plain scoped cell. `Show`'s children closure must be an
    // `Fn` and the class closure is moved into the element by value, so the
    // captured handle has to be Copy; a signal would compile too but would
    // pretend the class is reactive when only `style` actually is
    // (re-written by the placement effect).
    let panel_class: StoredValue<String, LocalStorage> = StoredValue::new_local(panel_class);

    view! {
        <Show when=move || visible.get()>
            <div node_ref=panel_ref class=move || panel_class.with_value(String::clone) style=move || style_sig.get() role="menu">
                {children()}
            </div>
        </Show>
    }
}
