//! The gloss box's adapter to the generic spring.
//!
//! `crate::components::primitives::motion::spring` drives any five-field
//! [`SpringValue`] and knows nothing about what the fields mean. This is the
//! seam that tells it what a gloss box is: `ai_core::gloss` owns the maths (and
//! `FloatBox`, the primitive's other rider, delegates to the same
//! `ui_geom::spring` integrator), so the adapter is three forwards and a
//! magnitude test.
//!
//! It lives here rather than beside the primitive because the dependency has to
//! point one way. A generic primitive that imported a feature crate's type
//! could be broken by that crate — and would quietly make every other consumer
//! of the primitive depend on the AI feature too. The trait is public, so the
//! type that needs it supplies the adapter; the primitive stays generic over
//! `SpringValue` and nothing else.

use ai_core::gloss::GlossBox;

use crate::components::primitives::motion::spring::SpringValue;

impl SpringValue for GlossBox {
    fn zero() -> Self {
        GlossBox::default()
    }
    fn close(&self, other: &Self, epsilon: f64) -> bool {
        ai_core::gloss::boxes_close(*self, *other, epsilon)
    }
    fn step(&self, vel: &Self, target: &Self, dt: f64) -> (Self, Self) {
        ai_core::gloss::step_spring(*self, *vel, *target, dt)
    }
    fn all_small(&self, epsilon: f64) -> bool {
        self.w.abs() < epsilon
            && self.x.abs() < epsilon
            && self.y.abs() < epsilon
            && self.h.abs() < epsilon
            && self.r.abs() < epsilon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gloss(x: f64, y: f64, w: f64, h: f64, r: f64) -> GlossBox {
        GlossBox { x, y, w, h, r }
    }

    #[test]
    fn gloss_all_small_covers_every_field() {
        // Each field above epsilon on its own must break "all small": the
        // check is hand-rolled for GlossBox, and a dropped field would let
        // a still-moving spring tear its rAF loop down early.
        for above in [
            gloss(1.0, 0.0, 0.0, 0.0, 0.0),
            gloss(0.0, 1.0, 0.0, 0.0, 0.0),
            gloss(0.0, 0.0, 1.0, 0.0, 0.0),
            gloss(0.0, 0.0, 0.0, 1.0, 0.0),
            gloss(0.0, 0.0, 0.0, 0.0, 1.0),
        ] {
            assert!(!above.all_small(0.6), "{above:?} read as small");
        }
        assert!(gloss(0.0, 0.0, 0.0, 0.0, 0.0).all_small(0.6));
    }
}
