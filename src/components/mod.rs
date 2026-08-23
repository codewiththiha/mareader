//! The application's component system, organized by what a component is
//! used for:
//!
//!   * `primitives` — generic UI (button, icon, popover, …); must never
//!     know what a PDF reader is
//!   * `chrome`     — reusable structural application chrome (title bar,
//!     adaptive toolbar, document titles)
//!   * `menus`      — menu features (appearance, more)
//!   * `overlays`   — transient UI (toast, drag feedback)
//!   * `reader_controls` — reader-only controls (zoom, page indicator, …)
//!   * `panels`     — the app sidebar (composition shell + panel hosts)
//!   * `document`   — UI whose purpose is displaying PDF documents
//!   * `search`     — search presentation shared by reader surfaces
//!
//! Import discipline: callers import from the owning group
//! (`use crate::components::primitives::button::Button`), which keeps each
//! component's origin visible. `primitives` must not reach upward into
//! `state`/`services`/`effects`/`pdf_engine`.
//!
//! Project rules:
//!
//! * Each conditional `class=("…", cond)` carries ONE token. A space-
//!   separated value throws a swallowed SyntaxError and the class never
//!   applies.
//! * `ResizeObserver::disconnect()` MUST run in `on_cleanup` BEFORE the
//!   `Closure` is dropped. The browser holds its own reference to the
//!   wasm-bindgen shim; a queued notification during teardown invokes
//!   freed memory.
//! * Leptos effects only subscribe to signals they READ during a run. A
//!   conditional read silently drops the subscription. Read every
//!   dependency unconditionally at the top of the effect.
//! * A Leptos `.set()` always notifies, even when the value is unchanged.
//!   Guard writes that run in a loop or animation frame.

mod conditional_class;
pub mod chrome;
pub mod document;
pub mod menus;
pub mod overlays;
pub mod panels;
pub mod primitives;
pub mod reader_controls;
pub mod search;
