//! Generic DOM/timer hooks: each owns one listener/effect family.

pub mod dom;
pub mod frame_active;
pub mod hover_reveal;
pub mod use_raf;
pub mod use_resize_observer;
pub mod use_timeout;
pub mod use_viewport;
pub mod use_window_event;
pub mod verified_switch;

// Re-exported flat: `hooks::use_hover_reveal` is the name callers reach for.
pub use hover_reveal::{
    DEFAULT_HOVER_DELAY, HoverConfig, HoverReveal, use_drag_hold, use_hover_reveal,
    use_hover_reveal_with,
};
pub use verified_switch::{VerifiedSwitch, use_verified_switch};
