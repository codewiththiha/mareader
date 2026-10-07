//! Internal PDF link navigation: the one place a link becomes a page change.

use leptos::prelude::*;

use app_ui::components::primitives::hooks::use_custom_event::use_raw_event_from;

use crate::pane::origin::{Origin, origin_of};

/// The event bubbles from the clicked link: only that pane turns its page.
pub fn link_navigation(state: crate::context::ReaderContext, active: Signal<bool>) {
    use_raw_event_from(app_ui::events::NAVIGATE_EVENT, move |detail, origin| {
        let active = active.try_get_untracked().unwrap_or(false);
        if origin_of(&state.reader.dom, active, origin.as_ref()) == Origin::Other {
            return;
        }
        let Some(page) = js_sys::Reflect::get(detail, &"page".into())
            .ok()
            .and_then(|v| v.as_f64())
        else {
            return;
        };
        let total = state.reader.document.num_pages.get_untracked().max(1);
        let page = (page as u32).clamp(1, total);
        state.reader.viewer.page.set(page);
    });
}
