//! The shell's component system, organized by what a component is used
//! for:
//!
//!   * `primitives`   — generic UI (button, icon, popover, ...); must never
//!     know what a PDF reader is
//!   * `shell`        — the application shell (the ShellController that owns
//!     layout truth, the titlebar family)
//!   * `menus`        — menu features (appearance_menu, reader_menu)
//!   * `settings`     — the reader settings modal: one module per tab
//!   * `app_overlays` — transient UI (toast, drag feedback)
//!
//! The reader's own components — viewer, formats, search, the AI selection
//! surfaces and the rail — live in the `reader-app` crate
//! (`reader_app::components`), and the shell reaches them there directly.
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
// The generic widget kit is a crate now (`ui-kit`) — both wasm builds render
// with it. Re-exported at the old path so the shell's callers keep reading
// `crate::components::primitives::…`.
pub use ui_kit::primitives;
