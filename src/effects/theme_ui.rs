//! Theme/texture/noise UI effects + thumbnail panel lifecycle.
//! OWNED BY branch D (panels/settings).
//!
//! Called once from the app root. `ThumbnailsPanel` renders its own canvases
//! when the sidebar switches to `Thumbs` and unregisters them in its
//! `on_cleanup` when it unmounts, so this hook only tracks the sidebar mode
//! (logging the leaving-Thumbs transition) and keeps a subscription to the
//! theme/texture settings for future UI needs.

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
            // ThumbnailsPanel just unmounted; its on_cleanup already unregistered
            // every thumbnail canvas, so nothing stale remains in the engine.
            web_sys::console::log_1(&"[theme_ui] thumbnails panel closed".into());
        }
        *p = mode;

        // Keep the theme/texture reads alive for future theme-UI hooks.
        let _ = (settings.theme_id, settings.texture);
    });
}
