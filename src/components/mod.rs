//! The application's component system, organized by what a component is used
//! for:
//!
//!   * `shell`        — the application shell: the titlebar family and the
//!     adapter that builds the layout rulebook (`app_chrome::controller`) out
//!     of this app's state
//!   * `menus`        — menu features (appearance_menu, reader_menu)
//!   * `settings`     — the reader settings modal: one module per tab
//!   * `app_overlays` — transient UI (toast, drag feedback)
//!
//! What an OPEN DOCUMENT paints is not here: the viewer and its layouts, the
//! format modules, the AI reading surfaces, the search overlays and the rail
//! are `reader_app::components`. This crate holds the window around them.
//!
//! Generic UI — the button, the popover, the overlay lanes, the pointer
//! gestures — is not a group here either: it lives in the `ui-kit` crate
//! (`ui_kit::controls::button::Button`), which knows nothing about a document
//! reader and is therefore free to reach upward into none of
//! `state`/`services`/`effects`/`pdf_engine`.
//!
//! Project rules:
//!
//! * Each conditional `class=("...", cond)` carries ONE token; a
//!   space-separated value throws a swallowed SyntaxError and never applies.
//! * `ResizeObserver::disconnect()` MUST run in `on_cleanup` BEFORE the
//!   `Closure` is dropped: the browser holds its own reference to the
//!   wasm-bindgen shim, and a queued notification during teardown invokes
//!   freed memory.
//! * Leptos effects only subscribe to signals they READ during a run; a
//!   conditional read silently drops the subscription. Read every dependency
//!   unconditionally at the top of the effect.
//! * A Leptos `.set()` always notifies, even when unchanged. Guard writes that
//!   run in a loop or animation frame.

pub mod app_overlays;
pub mod menus;
pub mod settings;
pub mod shell;
