//! Reusable UI primitives: generic controls (button, icon, slider, …),
//! the window-aware popover container, and the shared option/menu
//! building blocks.
//!
//! Contract: primitives must not know what a PDF reader is. No
//! `crate::state`, `crate::services`, `crate::effects`, or `pdf_engine`
//! imports below this point.

pub mod button;
pub mod icon;
pub mod icon_button;
pub mod kbd;
pub mod menu_item;
pub mod option_button;
pub mod popover;
pub mod section_label;
pub mod segmented;
pub mod separator;
pub mod shortcut_row;
pub mod slider;
pub mod tooltip;
