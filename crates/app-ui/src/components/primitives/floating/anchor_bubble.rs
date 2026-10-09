//! Anchored bubble: centered under its target, caret pointing back.

use leptos::children::ChildrenFn;
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use app_chrome::floating::position::{panel_size, viewport};
use app_chrome::hooks::use_window_event::use_window_event;
use ui_geom::floating::{PlacementOptions, PlacementSide, Rect, place_bubble_at_anchor};

/// Half the caret square: the center rides the bubble's own edge.
const CARET: f64 = 5.0;
/// The caret keeps this distance from the bubble's rounded corners.
const CARET_INSET: f64 = 12.0;

#[component]
pub fn AnchorBubble(
    /// The target's viewport-space rect; `None` hides the bubble.
    anchor: Signal<Option<Rect>>,
    /// The breathing room between target and bubble edge.
    #[prop(default = 10.0)]
    gap: f64,
    /// Surface classes: layer, border, background, shadow, padding.
    #[prop(optional, into)]
    class: Option<String>,
    children: ChildrenFn,
) -> impl IntoView {
    let panel_ref: NodeRef<html::Div> = NodeRef::new();
    let box_style = RwSignal::new(String::new());
    let caret_style = RwSignal::new(String::new());

    let place = move || {
        let Some(a) = anchor.try_get_untracked().flatten() else {
            return;
        };
        let size = panel_size(
            panel_ref
                .get()
                .map(|p| p.unchecked_into::<web_sys::Element>()),
            (160.0, 44.0),
        );
        let opts = PlacementOptions {
            side: PlacementSide::Auto,
            gap,
            margin: 8.0,
            viewport: viewport(),
        };
        let placed = place_bubble_at_anchor(a, size, &opts);
        let r = placed.rect;
        box_style.set(format!("left:{:.1}px;top:{:.1}px", r.x, r.y));
        // The caret rides the target's center, kept off the corners.
        let cx = (a.x + a.w / 2.0 - r.x).clamp(CARET_INSET, (r.w - CARET_INSET).max(CARET_INSET));
        let edge = if placed.transform_origin == "top center" {
            format!(
                "top:-{CARET}px;border-left:1px solid var(--color-line);\
                 border-top:1px solid var(--color-line);"
            )
        } else {
            format!(
                "bottom:-{CARET}px;border-right:1px solid var(--color-line);\
                 border-bottom:1px solid var(--color-line);"
            )
        };
        caret_style.set(format!("left:{cx:.1}px;{edge}"));
    };

    // Re-measured a frame after mount, when the bubble first has a size.
    Effect::new(move |_| {
        if anchor.get().is_none() {
            return;
        }
        let place = std::rc::Rc::new(place);
        place();
        {
            let place = std::rc::Rc::clone(&place);
            request_animation_frame(move || {
                // A frame can outlive the bubble's owner: a disposed anchor.
                if anchor.try_get_untracked().is_none() {
                    return;
                }
                place();
            });
        }
        {
            let place = std::rc::Rc::clone(&place);
            use_window_event("resize", move |_| place());
        }
    });

    let base = format!("bubble-enter fixed {}", class.unwrap_or_default());
    // Static for the bubble's lifetime: a Copy handle to a scoped cell.
    let panel_class: StoredValue<String, LocalStorage> = StoredValue::new_local(base);

    view! {
        <Show when=move || anchor.get().is_some()>
            <div
                node_ref=panel_ref
                class=move || panel_class.with_value(String::clone)
                style=move || box_style.get()
            >
                <div aria-hidden="true" class="bubble-caret" style=move || caret_style.get() />
                {children()}
            </div>
        </Show>
    }
}
