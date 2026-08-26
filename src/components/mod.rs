//! The application's component system, organized by what a component is
//! used for:
//!
//!   * `ai`         — AI-assisted reading (selection-anchored menu,
//!     explanation popover)
//!   * `primitives` — generic UI (button, icon, popover, …); must never
//!     know what a PDF reader is
//!   * `app_shell` — reusable structural application shell (title bar,
//!     adaptive toolbar, traffic lights, document titles)
//!   * `menus`      — menu features (appearance_menu, app_menu)
//!   * `overlays`   — transient UI (toast, drag feedback)
//!   * `viewer_controls` — reader-only controls (zoom, page indicator, …)
//!   * `sidebar`     — the app sidebar (composition shell + panel hosts)
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

pub mod ai;
pub mod app_shell;
pub mod document;
pub mod menus;
pub mod overlays;
pub mod sidebar;
pub mod primitives;
pub mod viewer_controls;
pub mod search;



