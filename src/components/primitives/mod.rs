//! Reusable UI primitives: generic controls (button, slider, …), the
//! floating system (placement / dismissal / popover / context menu /
//! floating card), the motion + interaction layers, the app's typed-event
//! hook, and the shared option/menu building blocks.
//!
//! The chrome's own primitives — icon, icon button, tooltip, the generic
//! DOM/timer hooks, the z-index layer tokens — live in the `app-chrome`
//! crate (import them from `app_chrome::…`); they moved out when window
//! chrome stopped being the PDF reader's business.
//!
//! Contract: primitives must not know what a PDF reader is. No
//! `crate::state`, `crate::services`, `crate::effects`, or `pdf_engine`
//! imports below this point (pure math may come from `pdf_core`, which is
//! dependency-free and host-testable).

pub mod button;
pub mod feedback;
pub mod floating;
pub mod form;
pub mod hooks;
pub mod interactions;
pub mod kbd;
pub mod menu_item;
pub mod motion;
pub mod option_button;
pub mod overlay;
pub mod section_label;
pub mod separator;
pub mod switch;
pub mod shortcut_row;
pub mod toggle_button;
