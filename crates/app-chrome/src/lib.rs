//! Format-agnostic window chrome, shared by every document format.
//!
//! The app is the only place that wires chrome to a format: this crate
//! renders the window frame — the platform probe, the frameless caption
//! cluster, the native macOS traffic lights, the generic hover/pin titlebar
//! shell — plus the small UI primitives and DOM hooks those chrome surfaces
//! render with, and it depends on nothing format-specific: no `pdf_engine`,
//! no document state. Format crates in turn depend on nothing here; the
//! boundary is the app.
//!
//! Layout:
//!   - [`platform`] — the desktop the webview is running on
//!   - [`window`] — the window commands, caption cluster, traffic lights
//!   - [`titlebar`] — the generic hover/pin titlebar shell + its context
//!   - [`icon`], [`icon_button`], [`tooltip`] — the shared controls
//!   - [`hooks`] — the generic DOM/timer hooks those surfaces use
//!   - [`floating`] — placement glue + dismissal mechanics for anchored surfaces
//!   - [`layers`] — the z-index layer tokens (re-exported by the app)

pub mod floating;
pub mod hooks;
pub mod icon;
pub mod icon_button;
pub mod platform;
pub mod titlebar;
pub mod tooltip;
pub mod window;
pub mod layers;

pub use titlebar::TITLE_BAR_H;
