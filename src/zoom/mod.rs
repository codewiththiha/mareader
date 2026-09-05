//! The zoom subsystem: one controller, one transition pipeline.
//!
//! The contract for the whole viewer is that zoom has exactly one owner.
//! Surfaces that want the zoom to change post a `ZoomCommand` through
//! `viewer.zoom.post(...)`; the [`ZoomController`] resolves it against the
//! current window, mode and page, and drives a single
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
//! The layout IS animated, and that is the point. Each frame hands the actuator
//! ([`actuator::ZoomActuator`]) the ratio the display scale just moved through;
//! it rescales the strips and holds the document point under the viewport
//! centre exactly where it is. That is also why nothing about position is captured at
//! transaction open — there is no seam to hide, so there is nothing to
//! restore.
//!
//! Scaling a frozen surface with one CSS transform instead was tried and
//! dropped: a transform scales the page gaps along with the pages while the
//! layout deliberately does not, so the whole accumulated gap error landed at
//! once when the transform was swapped for real geometry.
//!
//! What deliberately does NOT happen per animation frame:
//!
//! ```text
//! virtualizer.report_size()
//! scroll_to_index
//! page.set(...)
//! engine.renderPage(...)
//! ```
//!
//! Those are transaction-boundary work. The strips refuse to report measured
//! sizes mid-zoom (the rendered size belongs to a scale that no longer
//! exists), the crisp rasterisation is suspended for the whole tween and
//! issued once at the settled scale, and pages a moving window evicts are
//! bridged briefly by the virtualizer's zombie retention.
//!
//! Commands travel on a signal rather than a provided context because the
//! keyboard shortcuts are wired at the app root, above the reader's reactive
//! owner, where `use_context` cannot reach — and because a command queue
//! gives the same single-owner guarantee without any process-wide registry.
//! The old thread-local controller registration is gone; `drive` runs for
//! exactly as long as the reader page owns it.

pub mod actuator;
pub mod animation;
pub mod command;
pub mod config;
pub mod coordinator;
pub mod target;

pub use coordinator::ZoomController;

