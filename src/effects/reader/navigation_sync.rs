//! Navigation sync: keeps `viewer.page` and the continuous scroll position
//! in sync. Wired once from ReaderPage. The four effects live in sibling
//! modules; this file holds the shared state and the coordinator.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::layout::{DocumentLayout, PAGE_GAP};
use crate::state::ReaderState;
use crate::components::document::dom_helpers::page_list;

use super::nav_exact_landing::exact_landing;
use super::nav_mode_flip::mode_flip;
use super::nav_page_to_scroll::page_to_scroll;
use super::nav_scroll_to_page::scroll_to_page;

/// How long a smooth jump is allowed to be in flight. The browser owns the
/// animation and doesn't tell us when it finishes, so exact-landing takeover
/// detection is gated for this long.
pub(super) const JUMP_SETTLE_MS: f64 = 450.0;

/// Shared suppression flag + landing bookkeeping for the two one-way syncs.
pub(super) struct NavSyncState {
    /// Effect 2 sets this just before writing `page`; effect 3 clears it
    /// on its next run so they don't ping-pong.
    pub suppress: Rc<Cell<bool>>,
    pub pending: Rc<Cell<Option<u32>>>,
    pub last_ours: Rc<Cell<f64>>,
    pub jump_settle: Rc<Cell<f64>>,
    pub settle_timer: StoredValue<Option<TimeoutHandle>>,
    pub settle_wake: RwSignal<u32>,
}

impl NavSyncState {
    fn new() -> Self {
        Self {
            suppress: Rc::new(Cell::new(false)),
            pending: Rc::new(Cell::new(None)),
            last_ours: Rc::new(Cell::new(f64::NAN)),
            jump_settle: Rc::new(Cell::new(0.0)),
            settle_timer: StoredValue::new(None),
            settle_wake: RwSignal::new(0),
        }
    }
}

/// Scroll `#page-list` to `top`, smoothly for nearby targets and instantly
/// for far ones. The `2 * viewport` threshold matches the thumbnail glide.
pub(super) fn scroll_to(list: &web_sys::Element, top: f64, smooth: bool) {
    let opts = web_sys::ScrollToOptions::new();
    opts.set_top(top);
    opts.set_behavior(if smooth {
        web_sys::ScrollBehavior::Smooth
    } else {
        web_sys::ScrollBehavior::Instant
    });
    list.scroll_to_with_scroll_to_options(&opts);
}

/// Uniform page height used when real heights haven't been measured yet.
pub(super) fn estimated_top(page: u32, state: ReaderState) -> f64 {
    let est = state
        .document
        .page1_size
        .get_untracked()
        .map(|s| s.height)
        .unwrap_or(0.0)
        * state.viewer.zoom.render.get_untracked();
    (page.saturating_sub(1)) as f64 * (est + PAGE_GAP)
}

pub(super) fn list_or_container_h(state: ReaderState) -> f64 {
    page_list()
        .map(|el| el.client_height() as f64)
        .filter(|h| *h > 1.0)
        .unwrap_or_else(|| state.viewer.container_size.get_untracked().1)
}

/// Must be called once from the app root (ReaderPage), alongside `fit_effect`.
pub fn navigation_sync(state: ReaderState, layout: Memo<DocumentLayout>) {
    let nav = NavSyncState::new();
    mode_flip(state, layout);
    scroll_to_page(state, layout, nav.suppress.clone());
    page_to_scroll(state, layout, &nav);
    exact_landing(state, layout, &nav);
}
