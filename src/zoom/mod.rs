//! The zoom subsystem: one controller, one transition pipeline.
//!
//! Zoom has exactly one owner. Surfaces that want it changed post a
//! `ZoomCommand` through `viewer.zoom.post(...)`; the [`ZoomController`]
//! resolves it against the current window, mode and page and drives a single
//! [`ZoomTransition`](crate::state::reader::ZoomTransition):
//!
//! ```text
//! resolve the target (manual / fit / window constraint)
//!     ↓
//! open a transition from the scale on screen to that target
//!     ↓
//! tween the DISPLAY SCALE — relaying the layout out through the
//! actuator on every frame, so the document resizes continuously
//!     ↓
//! bring the render scale onto the target, release the freezes
//! ```
//!
//! The layout IS animated, and that is the point: each frame hands the
//! actuator ([`actuator::ZoomActuator`]) the ratio the display scale just
//! moved through; it rescales the strips and holds the document point under
//! the viewport centre where it is. Nothing about position is captured at
//! transaction open — there is no seam to hide, so nothing to restore. The
//! one-CSS-transform-over-frozen-geometry alternative was tried and dropped:
//! a transform scales the page gaps too while the layout deliberately does
//! not, so the accumulated gap error landed at once at the swap.
//!
//! What deliberately does NOT happen per animation frame:
//! `virtualizer.report_size()`, `scroll_to_index`, `page.set(...)`,
//! `engine.renderPage(...)`. Those are transaction-boundary work: the strips
//! refuse to report measured sizes mid-zoom (the rendered size belongs to a
//! scale that no longer exists), crisp rasterisation is suspended for the
//! tween and issued once at the settled scale, and pages a moving window
//! evicts are bridged briefly by zombie retention.
//!
//! Commands travel on a signal rather than a provided context because the
//! keyboard shortcuts are wired at the app root, above the reader's reactive
//! owner where `use_context` cannot reach — and a command queue gives the
//! same single-owner guarantee without a process-wide registry. `drive` runs
//! exactly as long as the reader page owns it.

pub mod actuator;
pub mod animation;
pub mod command;
pub mod config;
pub mod coordinator;
pub mod target;

pub use coordinator::ZoomController;

