//! The app's effect hooks.
//!
//! The generic DOM/timer hooks (dom, use_raf, use_resize_observer,
//! use_timeout, use_viewport, use_window_event, use_content_size) live in
//! the `app-chrome` crate — they are format-agnostic, and chrome renders
//! with them; import them from `app_chrome::hooks`. The one hook that is
//! NOT format-agnostic stays here: the typed CustomEvent hook, which
//! dispatches the app's own event protocol.

pub mod use_custom_event;
