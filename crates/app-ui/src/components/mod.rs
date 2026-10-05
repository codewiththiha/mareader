//! The shared component system, organized by what a component is used for:
//!
//!   * `primitives`   — generic UI (button, icon, popover, ...); must never
//!     know what a PDF reader is
//!   * `shell`        — the chrome both pages build on: the ShellController
//!     that owns layout truth and the app title bar
//!   * `menus`        — the appearance menu (the reader's own menu lives in
//!     `reader-runtime`)
//!   * `app_overlays` — transient UI (toast, drag feedback); each runtime
//!     mounts its own host inside its session
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
pub mod primitives;
pub mod shell;
