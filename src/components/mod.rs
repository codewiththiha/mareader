//! The application's component system, organized by what a component is used
//! for:
//!
//!   * `ai`           — AI-assisted reading (selection pill, explanation
//!     popover)
//!   * `primitives`   — generic UI (button, icon, popover, ...); must never
//!     know what a PDF reader is
//!   * `shell`        — the application shell (the ShellController that owns
//!     layout truth, the titlebar family, the sidebar rail family)
//!   * `menus`        — menu features (appearance_menu, reader_menu)
//!   * `settings`     — the reader settings modal: one module per tab
//!   * `app_overlays` — transient UI (toast, drag feedback)
//!   * `viewer`       — the viewing machinery: which layout, which shell, and
//!     the reader-only controls around them
//!   * `formats`      — one module per format, plus the page host that picks
//!     between them
//!   * `search`       — search presentation shared by reader surfaces
//!
//! `viewer` and `formats` point in one direction only: a viewer layout may ask
//! the page host for a page, never a format module directly — which keeps the
//! two growth axes (shapes of viewing, kinds of document) from being
//! multiplied into each other. Callers import from the owning group
//! (`crate::components::primitives::controls::button::Button`), and
//! `primitives` must not reach upward into
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

pub mod ai;
pub mod app_overlays;
pub mod formats;
pub mod menus;
pub mod primitives;
pub mod search;
pub mod settings;
pub mod shell;
pub mod viewer;
