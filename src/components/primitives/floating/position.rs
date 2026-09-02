//! Positioning glue: the pure placement math lives in `pdf_core::floating`
//! (host-testable); this module adapts it to living DOM nodes and viewport
//! reads, plus the coordinate-space compensation that WebKit's
//! `backdrop-filter` containing blocks force on us.

use super::types::{
    place_panel_from_anchor, rect_from_element, PlacementOptions, PlacementSide, PlacedPanel, Rect,
    Size,
};

/// Placement options with sensible floating-UI defaults.
pub fn placement_options(side: PlacementSide, gap: f64, margin: f64, viewport: Size) -> PlacementOptions {
    PlacementOptions {
        side,
        gap,
        margin,
        viewport,
    }
}

/// Place a panel of `panel_w` x `panel_h` at the given anchor element within
/// the viewport, optionally compensating a transformed/backdrop container.
///
/// `coordinate_space` names an element whose viewport offset must be
/// subtracted (WebKit treats `backdrop-filter` as a containing block for
/// `position: fixed` descendants, so a panel anchored inside a
/// `backdrop-blur` row would otherwise be positioned relative to that row,
/// not the viewport). When the anchor is inside the named element, both axes
/// are shifted so the coordinates become row-relative; otherwise the viewport
/// placement stands.
pub fn place_at_anchor(
    anchor: &web_sys::Element,
    panel_w: f64,
    panel_h: f64,
    opts: &PlacementOptions,
    coordinate_space: Option<&str>,
) -> PlacedPanel {
    let ar = rect_from_element(anchor);
    let placed = place_panel_from_anchor(ar, Size::new(panel_w, panel_h), opts);

    let Some(space_id) = coordinate_space else {
        return placed;
    };
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return placed;
    };
    let Some(space) = doc.get_element_by_id(space_id) else {
        return placed;
    };
    if !space.contains(Some(anchor)) {
        return placed;
    }
    let sr = space.get_bounding_client_rect();
    // Row-relative: the panel's fixed origin becomes the row's origin.
    PlacedPanel {
        rect: Rect::new(
            placed.rect.x - sr.left(),
            placed.rect.y - sr.top(),
            placed.rect.w,
            placed.rect.h,
        ),
        transform_origin: placed.transform_origin,
    }
}

/// Viewport-read helper for the panel height at placement time.
pub fn panel_size(node: Option<web_sys::Element>, fallback: (f64, f64)) -> Size {
    match node {
        Some(el) => {
            let r = el.get_bounding_client_rect();
            Size::new(r.width().max(1.0), r.height().max(1.0))
        }
        None => Size::new(fallback.0, fallback.1),
    }
}

/// The viewport as a [`Size`], via the shared reactive helper.
pub fn viewport() -> Size {
    let (w, h) = app_chrome::hooks::use_viewport::viewport_size();
    Size::new(w, h)
}
