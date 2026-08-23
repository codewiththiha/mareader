//! Entering continuous mode: align `scroll_top` to the current page.
//! `viewer.page` is the source of truth here, not a stale offset left
//! over from a previous continuous session.

use leptos::prelude::*;

use pdf_core::layout::{DocumentLayout, ViewMode};
use crate::state::ReaderState;

use super::navigation_sync::estimated_top;

pub(super) fn mode_flip(state: ReaderState, layout: Memo<DocumentLayout>) {
    let mut was_continuous = state.viewer.mode.get_untracked() == ViewMode::Continuous;
    Effect::new(move || {
        let continuous = state.viewer.mode.get() == ViewMode::Continuous;
        if continuous && !was_continuous {
            let page = state.viewer.page.get_untracked();
            let empty = state
                .document
                .metrics
                .css_heights
                .with_untracked(|heights| heights.is_empty());
            let top = if empty {
                estimated_top(page, state)
            } else {
                layout.with(|l| l.page_top(page.saturating_sub(1) as usize))
            };
            state.viewer.scroll_top.set(top);
        }
        was_continuous = continuous;
    });
}
