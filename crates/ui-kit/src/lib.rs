//! The app's reusable UI kit: generic controls (button, slider, …), the
//! floating system (placement / dismissal / popover / context menu /
//! floating card), the motion + interaction layers, the app's typed-event
//! hook, the shared option/menu building blocks — and the window-event
//! protocol every layer of the app speaks ([`events`]).
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
//!   * [`events`] — the `mareader:*` name table and its typed dispatchers
//!
//! The chrome's own primitives — icon, icon button, tooltip, the generic
//! DOM/timer hooks, the layer tokens, and the floating placement/dismissal
//! internals — live in the `app-chrome` crate (import them from
//! `app_chrome::…`); they moved out when window chrome stopped being the
//! PDF reader's business.
//!
//! Contract: nothing here knows what a document reader is. No app state, no
//! services, no effects and no `pdf_engine` imports below this point — a
//! component that needs one of those takes it as a prop, a signal or a
//! callback from whichever app mounted it. That rule is what lets the shell
//! and the reader compile this crate independently, which is the point of
//! having it: every surface on both sides of that split renders with the
//! same button, the same popover and the same event names.

pub mod controls;
pub mod events;
pub mod feedback;
pub mod floating;
pub mod form;
pub mod hooks;
pub mod interactions;
pub mod menu;
pub mod motion;
pub mod overlay;
