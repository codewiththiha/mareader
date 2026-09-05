//! Floating-system types: the geometry primitives (re-exported from the pure
//! `ui_geom::floating` so the math is host-testable) and the DOM-adapter
//! helpers that turn live elements into those primitives. The z-index layer
//! tokens are read straight from `crate::layers`.


use leptos::html;
use leptos::prelude::*;

use wasm_bindgen::JsCast;

pub use ui_geom::floating::{
    clamp_point_to_viewport, place_context_menu, place_panel_from_anchor, FloatBox,
    PlacementOptions, PlacementSide, PlacedPanel, Point, Rect, Size,
};


/// A viewport-space [`Rect`] from a DOM element's bounding box.
pub fn rect_from_element(el: &web_sys::Element) -> Rect {
    let r = el.get_bounding_client_rect();
    Rect::new(r.left(), r.top(), r.width(), r.height())
}

/// Convert the leptos event's target into a `web_sys::Element` for
/// `closest`-style exclusion checks, if it is one.
fn target_element(ev: &web_sys::Event) -> Option<web_sys::Element> {
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
}

/// Whether `target` is inside any of the anchored refs (used as the
/// "is this press part of the surface?" test for dismissal).
pub fn node_within_any(target: &web_sys::Node, refs: &[NodeRef<html::Div>]) -> bool {
    refs.iter()
        .filter_map(|r| r.get())
        .any(|el| el.contains(Some(target)))
}

/// Whether the event target lies within an element matching any of
/// `selectors` (e.g. `".gloss-mark"`), walking up from the target itself.
pub fn target_within_selectors(ev: &web_sys::Event, selectors: &[&str]) -> bool {
    let Some(el) = target_element(ev) else {
        return false;
    };
    selectors
        .iter()
        .any(|sel| el.closest(sel).ok().flatten().is_some())
}
