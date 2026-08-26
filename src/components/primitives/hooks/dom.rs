//! Small DOM lookups shared by the effects and views.
//!
//! `#page-list` (the continuous-scroll container) used to be resolved by an
//! inlined `window -> document -> get_element_by_id` chain in nine places
//! across five modules, each repeating the same three `and_then`s and each
//! free to misspell the id. These helpers make the id a single constant and
//! the lookup a single expression.
//!
//! Ids that anchor app chrome (the toolbar clusters, the viewer slot) are
//! named constants here too, and looked up through [`by_id_warn`] when a
//! miss can only be a bug — a renamed id then fails loudly (once) in the
//! console instead of silently disabling whatever measured against it.

use std::cell::RefCell;
use std::collections::HashSet;

/// Id of the continuous viewer's scroll container.
pub const PAGE_LIST_ID: &str = "page-list";

/// Id of the single-page view's container.
pub const SINGLE_PAGE_CONTAINER_ID: &str = "single-page-container";

/// Id of the toolbar's row (the flex container the title measures inside).
pub const TOOLBAR_ROW_ID: &str = "toolbar-row";

/// Id of the toolbar's leading control cluster.
pub const TOOLBAR_LEADING_ID: &str = "toolbar-leading";

/// Id of the toolbar's trailing control cluster.
pub const TOOLBAR_TRAILING_ID: &str = "toolbar-trailing";

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

/// The element with `id`, if the document is available and it exists.
pub fn by_id(id: &str) -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
}

/// [`by_id`] for chrome whose absence is a bug rather than a virtualization
/// gap: a miss is reported to the console once per id, so renaming an id in
/// the view shows up as a visible warning instead of the feature it anchors
/// quietly degrading. Page hosts (`sp-N-pg` / `cont-N-pg`) must NOT go
/// through this — they legitimately disappear whenever the virtualizer
/// unmounts their page.
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

/// Scroll `el`'s scroll parent so `el` is comfortably visible, but ONLY if it
/// is currently out of view.
///
/// WHY "only if out of view". The reader has two jobs here: following along as
/// the reader scrolls the document, and landing somewhere sensible when the
/// panel is opened. Unconditionally centring on every page change would yank
/// the list under the cursor while someone is reading down it — the row they
/// were about to click slides away. Scrolling only when the target is off
/// screen keeps the list still during normal browsing and still guarantees the
/// active row is reachable.
///
/// `margin` keeps the row off the very edge of the viewport, so there is
/// visible context above/below it rather than the row being flush against the
/// frame.
pub fn reveal_in_scroll_parent(el: &web_sys::Element, parent: &web_sys::Element, margin: f64) {
    let parent_h = parent.client_height() as f64;
    if parent_h <= 0.0 {
        return;
    }
    // offset_top is relative to the offset parent, which is not necessarily
    // the scroller, so measure through bounding rects instead — they share a
    // viewport origin and therefore always subtract correctly.
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

/// Centre `el` within its scroll `parent`, unconditionally.
///
/// Used for the deliberate "take me to where I am" gesture (re-clicking the
/// active sidebar tab), where the reader has explicitly asked to be moved and
/// the gentler `reveal_in_scroll_parent` would do nothing if the row happened
/// to already be barely on screen.
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
