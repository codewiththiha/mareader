//! Interaction primitives: pointer-drag mechanics and the long-press gesture.
//! The window-listener wiring lives here once; domain components keep the
//! policy (what a drag writes, what a long press means).

pub mod drag;
pub mod long_press;
