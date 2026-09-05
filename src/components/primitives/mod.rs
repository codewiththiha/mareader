//! Reusable UI primitives: generic controls (button, slider, …), the
//! floating system (placement / dismissal / popover / context menu /
//! floating card), the motion + interaction layers, the app's typed-event
//! hook, and the shared option/menu building blocks.
//!
//! Grouped by role, not by file count:
//!   * [`controls`] — pressables: button, toggle, option-group, switch
//!   * [`menu`] — menu chrome: item, separator, section label, key cap
//!   * [`form`] — input widgets: range, slider, text
//!   * [`feedback`] — loading/shimmer feedback
//!   * [`overlay`] — toast + overlay-lane policy
//!   * [`floating`] — the anchored floating surfaces (popover, context
//!     menu, floating card)
//!   * [`interactions`] / [`motion`] — pointer drag, long-press, springs
//!   * [`hooks`] — the app's typed CustomEvent hook
//!
//! The chrome's own primitives — icon, icon button, tooltip, the generic
//! DOM/timer hooks, the layer tokens, and the floating placement/dismissal
//! internals — live in the `app-chrome` crate (import them from
//! `app_chrome::…`); they moved out when window chrome stopped being the
//! PDF reader's business.
//!
//! Contract: primitives must not know what a PDF reader is. No
//! `crate::state`, `crate::services`, `crate::effects`, or `pdf_engine`
//! imports below this point (pure math may come from `pdf_core`, which is
//! dependency-free and host-testable).

pub mod controls;
pub mod feedback;
pub mod floating;
pub mod form;
pub mod hooks;
pub mod interactions;
pub mod menu;
pub mod motion;
pub mod overlay;
