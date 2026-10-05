//! The app's effect hooks.
//!
//! The generic DOM/timer hooks (dom, use_raf, use_resize_observer,
//! use_timeout, use_viewport, use_window_event) live in the `app-chrome`
//! crate — they are format-agnostic, and chrome renders with them; import
//! them from `app_chrome::hooks`. The hooks shaped by a reader feature
//! live next to that feature (the gloss card's twin measure hook sits in
//! `ai::gloss::hooks`), and the one hook that is NOT format-agnostic stays
//! here: the typed CustomEvent hook, which dispatches the app's own event
//! protocol.

pub mod use_custom_event;
