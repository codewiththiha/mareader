//! Reusable UI primitives: generic controls (button, icon, slider, …),
//! the floating system (placement / dismissal / popover / context menu /
//! floating card), the motion + interaction layers, the generic hooks, and
//! the shared option/menu building blocks.
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
pub mod icon;
pub mod icon_button;
pub mod interactions;
pub mod kbd;
pub mod menu_item;
pub mod motion;
pub mod option_button;
pub mod overlay;
pub mod section_label;
pub mod segmented;
pub mod separator;
pub mod shortcut_row;
pub mod toggle_button;
pub mod tooltip;
