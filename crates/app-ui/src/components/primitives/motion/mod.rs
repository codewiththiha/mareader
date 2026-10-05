//! Motion primitives: the generic spring loop, the reduced-motion signal, and
//! the frame delta every per-frame loop reads.
//!
//! These used to live inside `ai/gloss` (they were born there), but nothing
//! about them is gloss-specific — the same mechanics serve future floating
//! panels, draggable cards, animated popovers, side sheets and overlay
//! transitions.

pub mod frame;
pub mod reduced_motion;
pub mod spring;
