//! What the reader paints, organized by what a component is a view OF:
//!
//!   * `viewer`  — the viewing machinery: which layout, which shell, and the
//!     reader-only controls around them
//!   * `formats` — one module per format, plus the page host that picks
//!     between them
//!   * `ai`      — AI-assisted reading (the selection pill, the gloss card
//!     and its marks)
//!   * `search`  — search presentation shared by the reader surfaces
//!   * `rail`    — the reading rail: the aside container, its two mount
//!     points, and the outline and thumbnail panels
//!
//! `viewer` and `formats` point in one direction only: a viewer layout may
//! ask the page host for a page, never a format module directly — which keeps
//! the two growth axes (shapes of viewing, kinds of document) from being
//! multiplied into each other. Generic UI — the button, the popover, the
//! overlay lanes, the pointer gestures — is not a group here at all: it lives
//! in the `ui-kit` crate (`ui_kit::controls::button::Button`), which knows
//! nothing about a document reader and is therefore free to reach upward into
//! none of `state`/`effects`/`pdf_engine`.
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
pub mod formats;
pub mod rail;
pub mod search;
pub mod viewer;
