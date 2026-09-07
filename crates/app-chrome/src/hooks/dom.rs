//! Small DOM lookups shared by the effects and views. `#page-list` (the
//! continuous-scroll container) used to be resolved by an inlined
//! `window -> document -> get_element_by_id` chain in nine places across five
//! modules, each free to misspell the id; these helpers make the id a single
//! constant and the lookup a single expression. Ids that anchor app chrome
//! (toolbar clusters, viewer slot) are named constants too, looked up through
//! [`by_id_warn`] when a miss can only be a bug — a renamed id then fails
//! loudly in the console instead of silently disabling whatever measured
//! against it.

use std::cell::RefCell;
use std::collections::HashSet;

/// Id of the continuous viewer's scroll container.
pub const PAGE_LIST_ID: &str = "page-list";

/// Id of the single-page view's container.
pub const SINGLE_PAGE_CONTAINER_ID: &str = "single-page-container";

/// Id of the horizontal strip's scroll container.
pub const H_PAGE_LIST_ID: &str = "h-page-list";

/// Id of the dual-page (spread) view's container.
pub const DUAL_PAGE_CONTAINER_ID: &str = "dual-page-container";

/// Id of the toolbar's row (the flex container the title measures inside).
pub const TOOLBAR_ROW_ID: &str = "toolbar-row";

/// Id of the toolbar's leading control cluster.
pub const TOOLBAR_LEADING_ID: &str = "toolbar-leading";

/// Id of the toolbar's trailing control cluster.
pub const TOOLBAR_TRAILING_ID: &str = "toolbar-trailing";

/// Id of the center slot's content (the centered title). The shell reads its
/// natural width to decide between exact-row-center and free-stretch
/// placement, and observes it to re-measure when the name changes.
pub const TOOLBAR_CENTER_TITLE_ID: &str = "toolbar-center-title";

/// Id of the slot that frames the page column (the floating document title
/// budgets its width against this element's rect).
pub const VIEWER_SLOT_ID: &str = "viewer-slot";

thread_local! {
    /// Ids [`by_id_warn`] has already reported missing. A miss is worth one
    /// console line, not one per rAF re-measure — the warn is for catching a
    /// renamed id, not for narrating mount-order races.
    static WARNED_MISSING: RefCell<HashSet<&'static str>> =
        RefCell::new(HashSet::new());
}

/// The client rects a `Range` covers, as `(left, top, right, bottom)` tuples
/// in viewport CSS px. One range can report several rects (a span that wraps
/// a line or crosses inline boxes) and everything painting over text needs
/// all of them: a gloss stroke unions them, a search hit paints one box per
/// rect. Both format families call this, so it lives with the shared lookups.
/// An empty list (a range the browser will not give rects for) is a normal
/// answer: "nothing to place".
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

/// The element with `id`, if the document is available and it exists.
pub fn by_id(id: &str) -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
}

/// [`by_id`] for chrome whose absence is a bug rather than a virtualization
/// gap: a miss is reported to the console once per id, so renaming an id
/// shows up as a warning instead of the feature quietly degrading. Page hosts
/// (`sp-N-pg` / `cont-N-pg`) must NOT go through this — they legitimately
/// disappear whenever the virtualizer unmounts their page.
pub fn by_id_warn(id: &'static str) -> Option<web_sys::Element> {
    let el = by_id(id);
    if el.is_none() {
        WARNED_MISSING.with(|seen| {
            if seen.borrow_mut().insert(id) {
                web_sys::console::warn_1(
                    &format!("[dom] element #{id} not found — renamed id?").into(),
                );
            }
        });
    }
    el
}

/// The continuous viewer's scroll container, if it is mounted.
pub fn page_list() -> Option<web_sys::Element> {
    by_id(PAGE_LIST_ID)
}

/// The horizontal strip's scroll container, if it is mounted.
pub fn h_page_list() -> Option<web_sys::Element> {
    by_id(H_PAGE_LIST_ID)
}

/// Scroll `el`'s scroll parent so `el` is comfortably visible, but ONLY if it
/// is currently out of view.
///
/// The reader has two jobs here: following along as the document scrolls, and
/// landing somewhere sensible when the panel opens. Unconditionally centring
/// on every page change would yank the list under the cursor mid-read;
/// scrolling only when the target is off screen keeps the list still during
/// normal browsing and still guarantees the active row is reachable.
/// `margin` keeps the row off the very edge so there is visible context
/// above/below it.
pub fn reveal_in_scroll_parent(el: &web_sys::Element, parent: &web_sys::Element, margin: f64) {
    let parent_h = parent.client_height() as f64;
    if parent_h <= 0.0 {
        return;
    }
    // offset_top is relative to the offset parent, which is not necessarily
    // the scroller: measure through bounding rects instead — they share a
    // viewport origin and always subtract correctly.
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

/// Centre `el` within its scroll `parent`, unconditionally — the deliberate
/// "take me to where I am" gesture (re-clicking the active sidebar tab),
/// where the reader explicitly asked to be moved and the gentler
/// `reveal_in_scroll_parent` would do nothing if the row were barely on
/// screen already.
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
