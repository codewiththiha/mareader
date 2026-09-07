//! Format-agnostic window chrome, shared by every document format.
//!
//! This crate renders the window frame — the platform probe, the frameless
//! caption cluster, the native macOS traffic lights, the generic hover/pin
//! titlebar shell — plus the small UI primitives and DOM hooks those surfaces
//! use. It depends on nothing format-specific (no `pdf_engine`, no document
//! state); format crates depend on nothing here. The app is the only place
//! that wires chrome to a format.
//!
//! Layout:
//!   - [`platform`] — the desktop the webview is running on
//!   - [`window`] — window commands, caption cluster, traffic lights
//!   - [`titlebar`] — the generic hover/pin titlebar shell + its context
//!   - [`icon`], [`icon_button`], [`tooltip`] — shared controls
//!   - [`hooks`] — generic DOM/timer hooks
//!   - [`floating`] — placement glue + dismissal mechanics
//!   - [`layers`] — z-index layer tokens (re-exported by the app)

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
