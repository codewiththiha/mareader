//! Motion primitives: the generic spring loop and the reduced-motion signal.
//!
//! These used to live inside `ai/gloss` (they were born there), but nothing
//! about them is gloss-specific — the same mechanics serve future floating
//! panels, draggable cards, animated popovers, side sheets and overlay
//! transitions.

pub mod reduced_motion;
pub mod spring;
