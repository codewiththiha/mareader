//! Format-agnostic window chrome, shared by every document format.

pub mod appearance_hooks;
pub mod dialog;
pub mod floating;
pub mod hooks;
pub mod icon;
pub mod icon_button;
pub mod layers;
pub mod platform;
pub mod titlebar;
pub mod tooltip;
pub mod window;

pub use titlebar::TITLE_BAR_H;
