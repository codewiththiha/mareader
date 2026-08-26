//! Floating-system types: the geometry primitives (re-exported from the pure
//! `pdf_core::floating` so the math is host-testable), the layering tokens,
//! and the DOM-adapter helpers that turn live elements into those primitives.

use leptos::{html, prelude::*};

use wasm_bindgen::JsCast;

pub use pdf_core::floating::{
    clamp_point_to_viewport, place_context_menu, place_panel_from_anchor, FloatBox,
    PlacementOptions, PlacementSide, PlacedPanel, Point, Rect, Size,
};

/// Z-index layer tokens. The numeric values live in `styles/tokens.css` as
/// `--z-*` custom properties; these class-name constants are what components
/// embed, so layering is one decision instead of ten scattered numbers.
///
/// The Tailwind compiler scans source text: keep every token a static literal
/// (they are, via these constants) so `z-[var(--z-popover)]` etc. ship in
/// `styles.css`.
pub mod z {
    pub const CONTENT: &str = "z-0";
    pub const CONTROLS: &str = "z-[var(--z-controls)]";
    pub const BAR: &str = "z-[var(--z-bar)]";
    pub const POPOVER: &str = "z-[var(--z-popover)]";
    pub const SELECTION_BAR: &str = "z-[var(--z-selection-bar)]";
    pub const CONTEXT_MENU: &str = "z-[var(--z-context-menu)]";
    pub const AI_SELECTION: &str = "z-[var(--z-ai-selection)]";
    pub const DRAG_OVERLAY: &str = "z-[var(--z-drag-overlay)]";
    pub const TOAST: &str = "z-[var(--z-toast)]";
}

/// A viewport-space [`Rect`] from a DOM element's bounding box.
pub fn rect_from_element(el: &web_sys::Element) -> Rect {
    let r = el.get_bounding_client_rect();
    Rect::new(r.left(), r.top(), r.width(), r.height())
}

/// Convert the leptos event's target into a `web_sys::Element` for
/// `closest`-style exclusion checks, if it is one.
pub fn target_element(ev: &web_sys::Event) -> Option<web_sys::Element> {
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
