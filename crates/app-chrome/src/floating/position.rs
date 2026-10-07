//! Positioning glue: the pure maths live in `ui_geom::floating`.

use super::types::{
    PlacedPanel, PlacementOptions, Rect, Size, place_panel_from_anchor, rect_from_element,
};

/// Place a panel at an anchor within the viewport, optionally
/// compensating a container.
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

/// The measured size of `node`, or `fallback` before it has mounted.
pub fn panel_size(node: Option<web_sys::Element>, fallback: (f64, f64)) -> Size {
    match node {
        Some(el) => {
            let r = el.get_bounding_client_rect();
            Size::new(r.width().max(1.0), r.height().max(1.0))
        }
        None => Size::new(fallback.0, fallback.1),
    }
}

pub fn viewport() -> Size {
    let (w, h) = crate::hooks::use_viewport::viewport_size();
    Size::new(w, h)
}
