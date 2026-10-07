//! Small DOM lookups shared by the effects and views.

/// Id of the continuous viewer's scroll container.
pub const PAGE_LIST_ID: &str = "page-list";

pub const SINGLE_PAGE_CONTAINER_ID: &str = "single-page-container";

/// Id of the horizontal strip's scroll container.
pub const H_PAGE_LIST_ID: &str = "h-page-list";

/// Id of the spread view's container.
pub const DUAL_PAGE_CONTAINER_ID: &str = "dual-page-container";

/// Id of the toolbar's row (the flex container the title measures inside).
pub const TOOLBAR_ROW_ID: &str = "toolbar-row";

pub const TOOLBAR_LEADING_ID: &str = "toolbar-leading";

pub const TOOLBAR_TRAILING_ID: &str = "toolbar-trailing";

/// Id of the center slot's content, the centered title.
pub const TOOLBAR_CENTER_TITLE_ID: &str = "toolbar-center-title";

/// Id of the slot that frames the page column.
pub const VIEWER_SLOT_ID: &str = "viewer-slot";

/// The client rects a `Range` covers, as edge tuples.
pub fn range_rects(range: &web_sys::Range) -> Vec<(f64, f64, f64, f64)> {
    let Some(rects) = range.get_client_rects() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(rects.length() as usize);
    for index in 0..rects.length() {
        if let Some(rect) = rects.get(index) {
            out.push((rect.left(), rect.top(), rect.right(), rect.bottom()));
        }
    }
    out
}

pub fn by_id(id: &str) -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
}

pub fn page_list() -> Option<web_sys::Element> {
    by_id(PAGE_LIST_ID)
}

pub fn h_page_list() -> Option<web_sys::Element> {
    by_id(H_PAGE_LIST_ID)
}

/// Scroll `el`'s parent so `el` is visible, only if it is out of view.
pub fn reveal_in_scroll_parent(el: &web_sys::Element, parent: &web_sys::Element, margin: f64) {
    let parent_h = parent.client_height() as f64;
    if parent_h <= 0.0 {
        return;
    }
    // Measure through bounding rects: they share a viewport origin.
    let er = el.get_bounding_client_rect();
    let pr = parent.get_bounding_client_rect();
    let scroll_top = parent.scroll_top() as f64;

    // Position of the row within the scrollable content.
    let top = er.top() - pr.top() + scroll_top;
    let bottom = top + er.height();

    let view_top = scroll_top + margin;
    let view_bottom = scroll_top + parent_h - margin;

    let target = if top < view_top {
        // Above the fold: bring it to the top edge (plus margin).
        Some(top - margin)
    } else if bottom > view_bottom {
        // Below the fold: bring it to the bottom edge (minus margin).
        Some(bottom - parent_h + margin)
    } else {
        None
    };

    if let Some(t) = target {
        let max = (parent.scroll_height() as f64 - parent_h).max(0.0);
        parent.set_scroll_top(t.clamp(0.0, max) as i32);
    }
}

/// Centre `el` within its scroll parent, unconditionally.
pub fn center_in_scroll_parent(el: &web_sys::Element, parent: &web_sys::Element) {
    let parent_h = parent.client_height() as f64;
    if parent_h <= 0.0 {
        return;
    }
    let er = el.get_bounding_client_rect();
    let pr = parent.get_bounding_client_rect();
    let scroll_top = parent.scroll_top() as f64;
    let top = er.top() - pr.top() + scroll_top;
    let target = top - (parent_h - er.height()) / 2.0;
    let max = (parent.scroll_height() as f64 - parent_h).max(0.0);
    parent.set_scroll_top(target.clamp(0.0, max) as i32);
}
