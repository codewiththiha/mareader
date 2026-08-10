//! Theme/texture/noise UI effects + thumbnail panel lifecycle.
//! OWNED BY branch D (panels/settings).
//!
//! Called once from the app root. Both sidebar panels stay permanently
//! mounted (the inactive one is `invisible`, not unmounted), so
//! `ThumbnailsPanel`'s canvases remain engine-bound across toggles. This hook
//! only tracks the sidebar mode (logging the leaving-Thumbs transition) and
//! keeps a subscription to the theme/texture settings for future UI needs.

use std::cell::RefCell;
use std::rc::Rc;

use leptos::prelude::*;

use crate::core::state::{AppState, SidebarMode};

/// Must be called once from the app root.
pub fn theme_ui(state: AppState) {
    let prev = Rc::new(RefCell::new(SidebarMode::None));
    Effect::new(move || {
        let mode = state.sidebar.get();
        let settings = state.settings.get();

        let mut p = prev.borrow_mut();
        if *p == SidebarMode::Thumbs && mode != SidebarMode::Thumbs {
            // The thumbnails panel stays mounted (inactive panels are
            // `invisible`, not unmounted), so its canvases remain engine-bound
            // by design — nothing is unregistered on leaving Thumbs.
            web_sys::console::log_1(
                &"[theme_ui] sidebar left Thumbs (thumbnails stay mounted)".into(),
            );
        }
        *p = mode;

        // Keep the theme/texture reads alive for future theme-UI hooks.
        let _ = (settings.theme_id, settings.texture);
    });
}
